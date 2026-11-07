#![deny(warnings)]
// #![allow(unused)]
use anyhow::Result;
use chrono::Local;
use log::warn;
use log::{error, info};
use minijinja::context;
use reqwest;
use rhai::serde::{from_dynamic, to_dynamic};
use rhai::{Array, Dynamic, Engine, ImmutableString, Scope, AST};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::time::Duration;

use crate::notifier::{get_tag, log_message_assembly, render_notification_message, AlertDispatch, Event, HostStat, NotificationMessage, NotifyOutcome, NOTIFIER_HANDLE};

const KIND: &str = "webhook";

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct Receiver {
    pub enabled: bool,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub timeout: u32,
    pub script: String,
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct Config {
    pub enabled: bool,
    #[serde(default)]
    pub title: String,
    /// Event templates are rendered as a structured request body. Existing
    /// Rhai receiver scripts remain the fallback when no event template is set.
    #[serde(default = "default_format")]
    pub format: String,
    pub receiver: Vec<Receiver>,
}

fn default_format() -> String { "structured".to_string() }

pub struct Webhook {
    config: &'static Config,
    alerts: &'static crate::config::AlertsConfig,
    http_client: reqwest::Client,
    engine: Engine,
    ast_list: Vec<Option<AST>>,
}

#[allow(clippy::needless_pass_by_value)]
fn join(arr: Array, sep: ImmutableString) -> ImmutableString {
    arr.iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join(sep.as_str())
        .into()
}

fn now_str() -> ImmutableString {
    Local::now().format("%Y-%m-%d %H:%M:%S %Z").to_string().into()
}

#[allow(clippy::needless_pass_by_value)]
fn to_json(o: Dynamic) -> ImmutableString {
    serde_json::to_string(&o).map(std::convert::Into::into).unwrap_or_default()
}

impl Webhook {
    pub fn new(cfg: &'static Config, alerts: &'static crate::config::AlertsConfig) -> Self {
        let mut o = Self {
            config: cfg,
            alerts,
            http_client: reqwest::Client::new(),
            engine: Engine::new(),
            ast_list: Vec::new(),
        };

        o.engine.register_fn("to_json", to_json);
        o.engine.register_fn("join", join);
        o.engine.register_fn("now_str", now_str);

        for r in &o.config.receiver {
            if r.enabled {
                let ast = o.engine.compile(&r.script).unwrap();
                o.ast_list.push(Some(ast));
            } else {
                o.ast_list.push(None);
            }
        }

        o
    }
    fn call_webhook(&self, r: &'static Receiver, content: String, event: &str, host: &str) {
        if content.is_empty() {
            return;
        }

        let handle = NOTIFIER_HANDLE.lock().unwrap().as_ref().unwrap().clone();
        let http_client = self.http_client.clone();
        let event = event.to_string();
        let host = host.to_string();
        info!(
            "alert channel request assembled notifier=webhook event={} host={} url_configured={} headers={} basic_auth={} timeout_seconds={} body_len={}",
            event,
            host,
            !r.url.trim().is_empty(),
            r.headers.len(),
            r.username.as_deref().is_some_and(|value| !value.is_empty())
                && r.password.as_deref().is_some_and(|value| !value.is_empty()),
            r.timeout,
            content.len()
        );
        handle.spawn(async move {
            let mut http_client_builder = http_client
                .post(&r.url)
                .timeout(Duration::from_secs(r.timeout.into()))
                .body(reqwest::Body::from(content.into_bytes()));

            for (k, v) in &r.headers {
                http_client_builder = http_client_builder.header(k, v);
            }

            if let (Some(user), Some(pass)) = (r.username.as_ref(), r.password.as_ref()) {
                if !user.is_empty() && !pass.is_empty() {
                    http_client_builder = http_client_builder.basic_auth(user, Some(pass));
                }
            }

            //
            match http_client_builder.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        info!("alert delivery sent notifier=webhook event={} host={} status={status}", event, host);
                    } else {
                        error!("alert delivery failed notifier=webhook event={} host={} status={status}", event, host);
                    }
                }
                Err(err) => {
                    error!("alert delivery failed notifier=webhook event={} host={} error={err}", event, host);
                }
            }
        });
    }
}
impl crate::notifier::Notifier for Webhook {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn send_notify(&self, content: String) -> Result<()> {
        info!("{content}");
        Ok(())
    }

    fn notify_test(&self) -> Result<()> {
        for r in &self.config.receiver {
            if !r.enabled {
                continue;
            }
            self.call_webhook(r, "❗ServerStatus test msg".into(), "NotifyTest", "-");
        }
        Ok(())
    }

    fn notify(&self, e: &Event, stat: &HostStat, dispatch: &AlertDispatch) -> Result<NotifyOutcome> {
        if matches!(e, Event::Custom) && !dispatch.suppressed_rule_ids.is_empty() {
            warn!(
                "webhook notifier suppressed unstructured custom event rules={:?} host={}",
                dispatch.suppressed_rule_ids, stat.name
            );
            return Ok(NotifyOutcome::Skipped { reason: "suppressed" });
        }
        let mut queued = false;
        let mut enabled_receivers = 0;

        // A structured event template is channel-wide: all enabled webhook
        // receivers receive the same rendered body, while their existing
        // headers/authentication remain receiver-specific. If no structured
        // template is configured, preserve the legacy Rhai path below.
        let routed_context = context!(
            event => get_tag(e),
            host => stat,
            config => self.config,
            title => self.config.title.clone(),
            alerts => self.alerts,
            recoveries => &dispatch.recovered,
            firing => &dispatch.firing,
            ip_info => stat.ip_info,
            sys_info => stat.sys_info
        );
        let structured_message = render_notification_message(
            self.alerts,
            &dispatch.route_keys,
            &self.config.format,
            &self.config.title,
            String::new(),
            routed_context,
        )?;
        let structured_content = if structured_message.body.trim().is_empty() {
            None
        } else {
            let body = serde_json::from_str::<serde_json::Value>(&structured_message.body)
                .unwrap_or_else(|_| serde_json::Value::String(structured_message.body.clone()));
            Some(serde_json::json!({
                "title": structured_message.title,
                "body": body,
            }).to_string())
        };
        log_message_assembly(self.kind(), e, &self.config.format, &dispatch.route_keys, &structured_message);

        for (idx, r) in self.config.receiver.iter().enumerate() {
            if !r.enabled {
                continue;
            }
            enabled_receivers += 1;

            if let Some(content) = structured_content.as_ref() {
                info!("alert render selected notifier=webhook event={e:?} host={} format={}", stat.name, self.config.format);
                info!("alert delivery attempt notifier=webhook event={e:?} host={} receiver={idx} mode=structured", stat.name);
                self.call_webhook(r, content.clone(), get_tag(e), &stat.name);
                queued = true;
                continue;
            }

            let mut scope = Scope::new();
            scope.push("event", get_tag(e));
            scope.push("host", to_dynamic(stat)?);
            scope.push("config", to_dynamic(r)?);
            scope.push("alerts", to_dynamic(self.alerts)?);
            scope.push("recoveries", to_dynamic(&dispatch.recovered)?);
            scope.push("firing", to_dynamic(&dispatch.firing)?);
            scope.push("ip_info", to_dynamic(stat.ip_info.as_ref())?);
            scope.push("sys_info", to_dynamic(stat.sys_info.as_ref())?);

            let res: Dynamic = self
                .engine
                .eval_ast_with_scope(&mut scope, self.ast_list[idx].as_ref().unwrap())?;

            // [notify, json_body/content]
            if let Ok(v) = from_dynamic::<Array>(&res) {
                if v.len() >= 2 && from_dynamic::<bool>(&v[0]).unwrap_or_default() {
                    info!("alert delivery attempt notifier=webhook event={e:?} host={} receiver={idx}", stat.name);
                    let body = serde_json::to_string(&v[1]).unwrap_or_default();
                    let legacy_message = NotificationMessage::new(&self.config.title, &body);
                    log_message_assembly(self.kind(), e, "rhai", &dispatch.route_keys, &legacy_message);
                    self.call_webhook(r, body, get_tag(e), &stat.name);
                    queued = true;
                }
            }
        }

        if queued {
            Ok(NotifyOutcome::Queued)
        } else if enabled_receivers == 0 {
            info!("alert delivery skipped notifier=webhook event={e:?} host={} reason=no_enabled_receiver", stat.name);
            Ok(NotifyOutcome::Skipped { reason: "no_enabled_receiver" })
        } else {
            info!("alert delivery skipped notifier=webhook event={e:?} host={} reason=script_filtered", stat.name);
            Ok(NotifyOutcome::Skipped { reason: "script_filtered" })
        }
    }
}
