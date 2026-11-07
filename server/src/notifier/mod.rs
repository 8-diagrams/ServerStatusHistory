use anyhow::Result;
use once_cell::sync::Lazy;
use serde::Serialize;
use std::sync::Mutex;
use tokio::runtime::Handle;

use crate::payload::HostStat;

pub mod email;
pub mod log;
pub mod tgbot;
pub mod webhook;
pub mod wechat;

pub static NOTIFIER_HANDLE: Lazy<Mutex<Option<Handle>>> = Lazy::new(Default::default);

#[derive(Debug, Serialize, Clone)]
pub enum Event {
    NodeUp,
    NodeDown,
    Custom,
}

/// Context attached to a Custom notification. Marked blocks can be selected
/// with `allowed_rule_ids` or removed with `suppressed_rule_ids`.
#[derive(Debug, Clone, Default)]
pub struct AlertDispatch {
    pub allowed_rule_ids: Option<Vec<String>>,
    pub suppressed_rule_ids: Vec<String>,
}

/// Split a custom template into explicit alert blocks. A block is written as:
///
/// ```text
/// <!-- ss-alert:disk_usage_high -->
/// message body
/// <!-- /ss-alert -->
/// ```
///
/// Existing templates without markers are returned unchanged unless a rule is
/// being suppressed; in that case the whole legacy event is suppressed because
/// its individual branches cannot be identified safely.
pub fn select_custom_content(
    content: &str,
    allowed_rule_ids: Option<&[String]>,
    suppressed_rule_ids: &[String],
) -> String {
    const START: &str = "<!-- ss-alert:";
    const END: &str = "<!-- /ss-alert -->";
    let mut blocks = Vec::<(String, String)>::new();
    let mut current: Option<(String, String)> = None;
    let mut found_marker = false;

    for line in content.lines() {
        if let Some(rest) = line.trim().strip_prefix(START) {
            if let Some(id) = rest.strip_suffix(" -->") {
                found_marker = true;
                if let Some(previous) = current.take() {
                    warn!("custom alert marker nested or unclosed rule={}", previous.0);
                    blocks.push(previous);
                }
                current = Some((id.trim().to_ascii_lowercase(), String::new()));
                continue;
            }
        }
        if line.trim() == END {
            found_marker = true;
            if let Some(block) = current.take() {
                blocks.push(block);
            } else {
                warn!("custom alert marker has closing tag without opening tag");
            }
            continue;
        }
        if let Some((_, body)) = current.as_mut() {
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(line);
        }
    }
    if let Some(block) = current.take() {
        warn!("custom alert marker unclosed rule={}", block.0);
        blocks.push(block);
    }
    if !found_marker {
        if !suppressed_rule_ids.is_empty() {
            warn!(
                "legacy custom template has no ss-alert markers; suppressing the whole custom event rules={:?}",
                suppressed_rule_ids
            );
            return String::new();
        }
        return content.to_string();
    }

    blocks
        .into_iter()
        .filter(|(id, _)| {
            !suppressed_rule_ids
                .iter()
                .any(|value| value.eq_ignore_ascii_case(id))
                && allowed_rule_ids.is_none_or(|allowed| allowed.iter().any(|value| value.eq_ignore_ascii_case(id)))
        })
        .map(|(_, body)| body.trim().to_string())
        .filter(|body| !body.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn get_tag(e: &Event) -> &'static str {
    match *e {
        Event::NodeUp => "NodeUp",
        Event::NodeDown => "NodeDown",
        Event::Custom => "Custom",
    }
}

pub trait Notifier {
    fn kind(&self) -> &'static str;
    fn notify(&self, e: &Event, stat: &HostStat, dispatch: &AlertDispatch) -> Result<()>;
    // send notify impl
    fn send_notify(&self, content: String) -> Result<()>;
    fn notify_test(&self) -> Result<()> {
        self.send_notify("❗ServerStatus test msg".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::select_custom_content;

    #[test]
    fn marked_custom_content_selects_exact_rule_blocks() {
        let content = "<!-- ss-alert:disk_usage_high -->\ndisk\n<!-- /ss-alert -->\n<!-- ss-alert:memory_usage_high -->\nmemory\n<!-- /ss-alert -->";
        let allowed = vec!["memory_usage_high".to_string()];
        assert_eq!(select_custom_content(content, Some(&allowed), &[]), "memory");
    }

    #[test]
    fn unmarked_custom_content_remains_legacy_compatible() {
        assert_eq!(select_custom_content("legacy message", Some(&[]), &[]), "legacy message");
        let suppressed = vec!["disk_usage_high".to_string()];
        assert_eq!(select_custom_content("legacy message", None, &suppressed), "");
    }

    #[test]
    fn suppressed_rule_is_removed_without_removing_other_marked_rules() {
        let content = "<!-- ss-alert:disk_usage_high -->\ndisk\n<!-- /ss-alert -->\n<!-- ss-alert:memory_usage_high -->\nmemory\n<!-- /ss-alert -->";
        let suppressed = vec!["disk_usage_high".to_string()];
        assert_eq!(select_custom_content(content, None, &suppressed), "memory");
    }
}
