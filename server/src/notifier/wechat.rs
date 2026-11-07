#![deny(warnings)]
use anyhow::Result;
use log::{error, info};
use minijinja::context;
use reqwest;
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;
use tokio::time::Duration;

use crate::jinja::{add_template, render_template};
use crate::notifier::{get_tag, render_notification_message, select_custom_content, AlertDispatch, Event, HostStat, NotifyOutcome, NOTIFIER_HANDLE};

// https://qydev.weixin.qq.com/wiki/index.php?title=%E4%B8%BB%E5%8A%A8%E8%B0%83%E7%94%A8
// https://qydev.weixin.qq.com/wiki/index.php?title=%E5%8F%91%E9%80%81%E6%8E%A5%E5%8F%A3%E8%AF%B4%E6%98%8E
static TOKEN_URL: &str = "https://qyapi.weixin.qq.com/cgi-bin/gettoken";
const KIND: &str = "wechat";

fn default_recovery_tpl() -> String {
    "{% for recovery in recoveries %}✅ {{host.name}} {{recovery.alert_type}} 已恢复，当前 {{recovery.value_percent | round}}%，恢复阈值 {{recovery.recover_threshold}}%{% endfor %}".to_string()
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Config {
    pub enabled: bool,
    #[serde(default = "default_format")]
    pub format: String,
    pub corp_id: String,
    pub corp_secret: String,
    pub agent_id: String,
    pub title: String,
    pub online_tpl: String,
    pub offline_tpl: String,
    pub custom_tpl: String,
    #[serde(default = "default_recovery_tpl")]
    pub recovery_tpl: String,
}

fn default_format() -> String { "text".to_string() }

pub struct WeChat {
    config: &'static Config,
    alerts: &'static crate::config::AlertsConfig,
    http_client: reqwest::Client,
}

impl WeChat {
    pub fn new(cfg: &'static Config, alerts: &'static crate::config::AlertsConfig) -> Self {
        let o = Self {
            config: cfg,
            alerts,
            http_client: reqwest::Client::new(),
        };
        add_template(KIND, get_tag(&Event::NodeUp), o.config.online_tpl.clone());
        add_template(KIND, get_tag(&Event::NodeDown), o.config.offline_tpl.clone());
        add_template(KIND, get_tag(&Event::Custom), o.config.custom_tpl.clone());
        add_template(KIND, get_tag(&Event::AlertRecovered), o.config.recovery_tpl.clone());

        o
    }
}

impl crate::notifier::Notifier for WeChat {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn send_notify(&self, text_content: String) -> Result<()> {
        self.send_notify_with_context(text_content, "NotifyTest", "-")
    }

    fn send_notify_with_context(&self, text_content: String, event: &str, host: &str) -> Result<()> {
        // get access_token
        let mut data = HashMap::new();
        data.insert("corpid", self.config.corp_id.clone());
        data.insert("corpsecret", self.config.corp_secret.clone());

        let http_client = self.http_client.clone();
        let handle = NOTIFIER_HANDLE.lock().unwrap().as_ref().unwrap().clone();
        let agent_id = self.config.agent_id.clone();
        let event = event.to_string();
        let host = host.to_string();
        handle.spawn(async move {
            match http_client
                .post(TOKEN_URL)
                .timeout(Duration::from_secs(5))
                .json(&data)
                .send()
                .await
            {
                Ok(resp) => {
                    let status = resp.status();
                    info!("alert delivery token_response notifier=wechat event={} host={} status={status}", event, host);
                    let json_res = resp.json::<HashMap<String, serde_json::Value>>().await;
                    if let Ok(json_data) = json_res {
                        if let Some(access_token) = json_data.get("access_token") {
                            if let Some(token) = access_token.as_str() {
                                let req_url =
                                    format!("https://qyapi.weixin.qq.com/cgi-bin/message/send?access_token={token}");
                                let req_data = serde_json::json!({
                                    "touser": "@all",
                                    "agentid": agent_id,
                                    "msgtype": "text",
                                    "text": {
                                        "content": text_content,
                                    },
                                    "safe": 0
                                });

                                match http_client
                                    .post(&req_url)
                                    .timeout(Duration::from_secs(5))
                                    .json(&req_data)
                                    .send()
                                    .await
                                {
                                    Ok(resp) => {
                                        info!("alert delivery sent notifier=wechat event={} host={} status={}", event, host, resp.status());
                                    }
                                    Err(err) => {
                                        error!("alert delivery failed notifier=wechat event={} host={} error={err}", event, host);
                                    }
                                }
                            }
                        }
                    }
                }
                Err(err) => {
                    error!("alert delivery failed notifier=wechat event={} host={} stage=access_token error={err}", event, host);
                }
            }
        });

        Ok(())
    }

    fn notify(&self, e: &Event, stat: &HostStat, dispatch: &AlertDispatch) -> Result<NotifyOutcome> {
        let context = context!(event => get_tag(e), host => stat, config => self.config, alerts => self.alerts, firing => &dispatch.firing, recoveries => &dispatch.recovered, ip_info => stat.ip_info, sys_info => stat.sys_info);
        let fallback_body = render_template(self.kind(), get_tag(e), context.clone(), true)?;
        let mut message = render_notification_message(
            self.alerts,
            &dispatch.route_keys,
            &self.config.format,
            &self.config.title,
            fallback_body,
            context,
        )?;
        if matches!(e, Event::Custom) {
            message.body = select_custom_content(
                &message.body,
                dispatch.allowed_rule_ids.as_deref(),
                &dispatch.suppressed_rule_ids,
            );
        }
        info!("render.alert.tpl notifier={} event={e:?} host={} content_empty={}", self.kind(), stat.name, message.body.is_empty());
        if message.body.is_empty() {
            info!("alert delivery skipped notifier={} event={e:?} host={} reason=empty_template", self.kind(), stat.name);
            return Ok(NotifyOutcome::Skipped { reason: "empty_template" });
        }
        info!("alert delivery attempt notifier={} event={e:?} host={} gid={}", self.kind(), stat.name, stat.gid);
        self.send_notify_with_context(message.text(), get_tag(e), &stat.name)?;
        Ok(NotifyOutcome::Queued)
    }
}
