use anyhow::Result;
use ::log::info;
use once_cell::sync::Lazy;
use serde::Serialize;
use std::sync::Mutex;
use tokio::runtime::Handle;

use crate::config::AlertsConfig;
use crate::jinja::render_source;
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
    AlertRecovered,
}

#[derive(Debug, Clone, Serialize)]
pub struct AlertRecovery {
    pub alert_type: String,
    pub rule_id: String,
    pub value_percent: f64,
    pub recover_threshold: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AlertFiring {
    pub alert_type: String,
    pub rule_id: String,
    pub value_percent: f64,
    pub threshold: f64,
    pub recover_threshold: f64,
}

/// Context attached to a notification. Marked Custom blocks can be selected
/// with `allowed_rule_ids` or removed with `suppressed_rule_ids`; resource
/// recovery details are carried in `recovered`.
#[derive(Debug, Clone, Default)]
pub struct AlertDispatch {
    /// Event route keys, such as `disk_usage_high` or
    /// `disk_usage_recovered`.
    pub route_keys: Vec<String>,
    pub firing: Vec<AlertFiring>,
    pub allowed_rule_ids: Option<Vec<String>>,
    pub suppressed_rule_ids: Vec<String>,
    pub recovered: Vec<AlertRecovery>,
}

/// Channel-neutral notification payload. Alert rules produce title/body; a
/// notifier only adapts them to its transport protocol.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotificationMessage {
    pub title: String,
    pub body: String,
}

fn log_preview(value: &str) -> String {
    const LIMIT: usize = 240;
    let normalized = value.replace(['\n', '\r', '\t'], " ");
    if normalized.chars().count() <= LIMIT {
        normalized
    } else {
        format!("{}…", normalized.chars().take(LIMIT).collect::<String>())
    }
}

pub fn log_message_assembly(
    notifier: &str,
    event: &Event,
    format: &str,
    route_keys: &[String],
    message: &NotificationMessage,
) {
    info!(
        "alert content assembled notifier={} event={event:?} format={} route_keys={:?} title={:?} body_len={} body_preview={:?}",
        notifier,
        format,
        route_keys,
        message.title,
        message.body.len(),
        log_preview(&message.body)
    );
}

impl NotificationMessage {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
        }
    }

    pub fn text(&self) -> String {
        match (self.title.trim(), self.body.trim()) {
            ("", body) => body.to_string(),
            (title, "") => title.to_string(),
            (title, body) if body == title || body.starts_with(&format!("{title}\n")) => body.to_string(),
            (title, body) => format!("{title}\n{body}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyOutcome {
    /// The notifier accepted the event and queued the channel operation.
    Queued,
    /// The notifier intentionally did not send anything (for example an
    /// empty or disabled template).
    Skipped { reason: &'static str },
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

pub fn render_routed_format(
    alerts: &AlertsConfig,
    route_keys: &[String],
    format: &str,
    context: minijinja::value::Value,
) -> Result<Option<String>> {
    let normalized_format = format.trim().to_ascii_lowercase();
    let format = match normalized_format.as_str() {
        "json" | "webhook" => "structured",
        "txt" | "plain" => "text",
        value => value,
    };
    let mut rendered_templates = Vec::new();
    for route_key in route_keys {
        let Some(policy) = alerts.events.get(route_key) else {
            continue;
        };
        let source = match format {
            "html" => &policy.templates.html,
            "text" => &policy.templates.text,
            "structured" => &policy.templates.structured,
            _ => continue,
        };
        if source.trim().is_empty() {
            continue;
        }
        let rendered = render_source(
            &format!("alert.{route_key}.{format}"),
            source,
            context.clone(),
            true,
        )?;
        if !rendered.trim().is_empty() {
            rendered_templates.push(rendered);
        }
    }
    if rendered_templates.is_empty() {
        Ok(None)
    } else if format == "structured" && rendered_templates.len() > 1 {
        let values = rendered_templates
            .iter()
            .map(|value| serde_json::from_str::<serde_json::Value>(value))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(Some(serde_json::to_string(&values)?))
    } else {
        Ok(Some(rendered_templates.join("\n")))
    }
}

pub fn render_notification_message(
    alerts: &AlertsConfig,
    route_keys: &[String],
    format: &str,
    fallback_title: &str,
    fallback_body: String,
    context: minijinja::value::Value,
) -> Result<NotificationMessage> {
    let title = render_routed_title(alerts, route_keys, context.clone())?
        .unwrap_or_else(|| fallback_title.to_string());
    let body = render_routed_format(alerts, route_keys, format, context)?
        .unwrap_or(fallback_body);
    Ok(NotificationMessage::new(title, body))
}

/// Render the first non-empty event title in route order. A Custom event may
/// contain several rule routes; its first configured title is the stable
/// summary while all matching bodies are still combined separately.
pub fn render_routed_title(
    alerts: &AlertsConfig,
    route_keys: &[String],
    context: minijinja::value::Value,
) -> Result<Option<String>> {
    for route_key in route_keys {
        let Some(source) = alerts
            .events
            .get(route_key)
            .map(|policy| policy.templates.title.as_str())
            .filter(|source| !source.trim().is_empty())
        else {
            continue;
        };
        let rendered = render_source(
            &format!("alert.{route_key}.title"),
            source,
            context.clone(),
            true,
        )?;
        if !rendered.trim().is_empty() {
            return Ok(Some(rendered));
        }
    }
    Ok(None)
}

fn get_tag(e: &Event) -> &'static str {
    match *e {
        Event::NodeUp => "NodeUp",
        Event::NodeDown => "NodeDown",
        Event::Custom => "Custom",
        Event::AlertRecovered => "AlertRecovered",
    }
}

pub trait Notifier {
    fn kind(&self) -> &'static str;
    fn notify(&self, e: &Event, stat: &HostStat, dispatch: &AlertDispatch) -> Result<NotifyOutcome>;
    // send notify impl
    fn send_notify(&self, content: String) -> Result<()>;
    fn send_notify_with_context(&self, content: String, _event: &str, _host: &str) -> Result<()> {
        self.send_notify(content)
    }
    fn notify_test(&self) -> Result<()> {
        self.send_notify("❗ServerStatus test msg".to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use minijinja::context;

    use crate::config::{AlertEventConfig, AlertEventTemplates, AlertsConfig};
    use crate::jinja::{add_template, render_template};
    use crate::payload::HostStat;

    use super::{get_tag, render_routed_format, AlertRecovery, Event, NotificationMessage};
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

    #[test]
    fn recovery_template_receives_structured_recovery_context() {
        add_template(
            "notifier-recovery-test",
            get_tag(&Event::AlertRecovered),
            "{% for recovery in recoveries %}{{host.name}} {{recovery.alert_type}} {{recovery.value_percent}} {{recovery.recover_threshold}}{% endfor %}".to_string(),
        );
        let host = HostStat {
            name: "node-1".to_string(),
            ..HostStat::default()
        };
        let recoveries = vec![AlertRecovery {
            alert_type: "disk_usage_high".to_string(),
            rule_id: "resource-default".to_string(),
            value_percent: 70.0,
            recover_threshold: 75.0,
        }];
        let rendered = render_template(
            "notifier-recovery-test",
            get_tag(&Event::AlertRecovered),
            context!(host => host, recoveries => recoveries),
            true,
        )
        .unwrap();
        assert_eq!(rendered, "node-1 disk_usage_high 70.0 75.0");
    }

    #[test]
    fn routed_template_uses_requested_format_and_aliases() {
        let mut alerts = AlertsConfig::default();
        alerts.events = HashMap::from([(
            "disk_usage_high".to_string(),
            AlertEventConfig {
                templates: AlertEventTemplates {
                    title: "{{ host.name }} 告警".to_string(),
                    html: "<b>{{ host.name }}</b>".to_string(),
                    text: "text {{ host.name }}".to_string(),
                    structured: "{\"host\":\"{{ host.name }}\"}".to_string(),
                },
                ..AlertEventConfig::default()
            },
        )]);
        let host = HostStat { name: "node-1".to_string(), ..HostStat::default() };
        let html = render_routed_format(
            &alerts,
            &["disk_usage_high".to_string()],
            "html",
            context!(host => host.clone()),
        )
        .unwrap();
        assert_eq!(html.as_deref(), Some("<b>node-1</b>"));
        let structured = render_routed_format(
            &alerts,
            &["disk_usage_high".to_string()],
            "webhook",
            context!(host => host),
        )
        .unwrap();
        assert_eq!(structured.as_deref(), Some("{\"host\":\"node-1\"}"));
        let message = super::render_notification_message(
            &alerts,
            &["disk_usage_high".to_string()],
            "html",
            "ServerStatus",
            "fallback".to_string(),
            context!(host => HostStat { name: "node-1".to_string(), ..HostStat::default() }),
        )
        .unwrap();
        assert_eq!(message.title, "node-1 告警");
        assert_eq!(message.body, "<b>node-1</b>");
    }

    #[test]
    fn routed_templates_combine_multiple_text_alerts_and_json_alerts() {
        let mut alerts = AlertsConfig::default();
        alerts.events = HashMap::from([
            (
                "disk_usage_high".to_string(),
                AlertEventConfig {
                    templates: AlertEventTemplates {
                        text: "disk".to_string(),
                        structured: "{\"type\":\"disk\"}".to_string(),
                        ..AlertEventTemplates::default()
                    },
                    ..AlertEventConfig::default()
                },
            ),
            (
                "memory_usage_high".to_string(),
                AlertEventConfig {
                    templates: AlertEventTemplates {
                        text: "memory".to_string(),
                        structured: "{\"type\":\"memory\"}".to_string(),
                        ..AlertEventTemplates::default()
                    },
                    ..AlertEventConfig::default()
                },
            ),
        ]);
        let routes = ["disk_usage_high".to_string(), "memory_usage_high".to_string()];
        let text = render_routed_format(&alerts, &routes, "text", context!()).unwrap();
        assert_eq!(text.as_deref(), Some("disk\nmemory"));
        let structured = render_routed_format(&alerts, &routes, "structured", context!()).unwrap();
        assert_eq!(structured.as_deref(), Some(r#"[{"type":"disk"},{"type":"memory"}]"#));
    }

    #[test]
    fn notification_message_keeps_channel_contract_to_title_and_body() {
        let message = NotificationMessage::new("ServerStatus", "主机已离线");
        assert_eq!(message.title, "ServerStatus");
        assert_eq!(message.body, "主机已离线");
        assert_eq!(message.text(), "ServerStatus\n主机已离线");
        let legacy = NotificationMessage::new("ServerStatus", "ServerStatus\n主机已离线");
        assert_eq!(legacy.text(), "ServerStatus\n主机已离线");
    }
}
