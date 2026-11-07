#![deny(warnings)]
use anyhow::Result;
use chrono::Local;
use log::{info, warn};
use minijinja::context;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

use crate::jinja::{add_template, render_template};
use crate::notifier::{render_notification_message, AlertDispatch, Event, HostStat, NotifyOutcome, NOTIFIER_HANDLE};

const KIND: &str = "log";

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Config {
    pub enabled: bool,
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default)]
    pub title: String,
    pub log_dir: String,
    pub tpl: String,
}

fn default_format() -> String { "text".to_string() }

pub struct Log {
    config: &'static Config,
    alerts: &'static crate::config::AlertsConfig,
}

impl Log {
    pub fn new(cfg: &'static Config, alerts: &'static crate::config::AlertsConfig) -> Self {
        let o = Self { config: cfg, alerts };

        add_template(KIND, "tpl", o.config.tpl.clone());

        // build dir
        fs::create_dir_all(&cfg.log_dir).unwrap_or_else(|_| panic!("can't create dir `{}", cfg.log_dir));
        o
    }
}

impl crate::notifier::Notifier for Log {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn send_notify(&self, content: String) -> Result<()> {
        self.send_notify_with_context(content, "NotifyTest", "-")
    }

    fn send_notify_with_context(&self, content: String, event: &str, host: &str) -> Result<()> {
        if content.is_empty() {
            return Ok(());
        }

        let dt = Local::now().format("%Y-%m-%d").to_string();
        let log_file = Path::new(&self.config.log_dir)
            .join(format!("ssr.log.{dt}"))
            .to_string_lossy()
            .to_string();
        let event = event.to_string();
        let host = host.to_string();

        let handle = NOTIFIER_HANDLE.lock().unwrap().as_ref().unwrap().clone();
        handle.spawn(async move {
            //
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .append(true)
                .open(&log_file)
                .await
                .unwrap_or_else(|_| panic!("can't create log `{log_file}"));

            let _ = file
                .write(content.as_bytes())
                .await
                .unwrap_or_else(|_| panic!("can't write log `{log_file}"));

            if !content.ends_with('\n') {
                let _ = file
                    .write(b"\n")
                    .await
                    .unwrap_or_else(|_| panic!("can't write log `{log_file}"));
            }

            file.flush()
                .await
                .unwrap_or_else(|_| panic!("can't flush log `{log_file}"));
            info!("alert delivery sent notifier=log event={} host={} file={}", event, host, log_file);
        });
        Ok(())
    }

    fn notify(&self, e: &Event, stat: &HostStat, dispatch: &AlertDispatch) -> Result<NotifyOutcome> {
        if matches!(e, Event::Custom) && !dispatch.suppressed_rule_ids.is_empty() {
            warn!(
                "log notifier suppressed unstructured custom event rules={:?} host={}",
                dispatch.suppressed_rule_ids, stat.name
            );
            return Ok(NotifyOutcome::Skipped { reason: "suppressed" });
        }
        let context = context!(event => crate::notifier::get_tag(e), host => stat, config => self.config, alerts => self.alerts, firing => &dispatch.firing, recoveries => &dispatch.recovered, ip_info => stat.ip_info, sys_info => stat.sys_info);
        let fallback_body = render_template(self.kind(), "tpl", context.clone(), true)?;
        let message = render_notification_message(
            self.alerts,
            &dispatch.route_keys,
            &self.config.format,
            &self.config.title,
            fallback_body,
            context,
        )?;
        if message.body.is_empty() {
            info!("alert delivery skipped notifier={} event={e:?} host={} reason=empty_template", self.kind(), stat.name);
            return Ok(NotifyOutcome::Skipped { reason: "empty_template" });
        }
        info!("alert delivery attempt notifier={} event={e:?} host={} gid={}", self.kind(), stat.name, stat.gid);
        self.send_notify_with_context(message.text(), crate::notifier::get_tag(e), &stat.name)?;
        Ok(NotifyOutcome::Queued)
    }
}
