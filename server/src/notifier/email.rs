#![deny(warnings)]
use anyhow::Result;
use lettre::{
    message::{header, Mailboxes, MultiPart, SinglePart},
    transport::smtp::authentication::Credentials,
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};
use log::{error, info};
use minijinja::context;
use serde::{Deserialize, Serialize};

use crate::jinja::{add_template, render_template};
use crate::notifier::{get_tag, select_custom_content, AlertDispatch, Event, HostStat, NotifyOutcome, NOTIFIER_HANDLE};

const KIND: &str = "email";

fn default_recovery_tpl() -> String {
    "{% for recovery in recoveries %}✅ {{host.name}} {{recovery.alert_type}} 已恢复，当前 {{recovery.value_percent | round}}%，恢复阈值 {{recovery.recover_threshold}}%{% endfor %}".to_string()
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Config {
    pub enabled: bool,
    pub server: String,
    pub username: String,
    pub password: String,
    pub to: String,
    pub subject: String,
    pub title: String,
    pub online_tpl: String,
    pub offline_tpl: String,
    pub custom_tpl: String,
    #[serde(default = "default_recovery_tpl")]
    pub recovery_tpl: String,
}

pub struct Email {
    config: &'static Config,
    alerts: &'static crate::config::AlertsConfig,
}

impl Email {
    pub fn new(cfg: &'static Config, alerts: &'static crate::config::AlertsConfig) -> Self {
        let o = Self { config: cfg, alerts };
        add_template(KIND, get_tag(&Event::NodeUp), o.config.online_tpl.clone());
        add_template(KIND, get_tag(&Event::NodeDown), o.config.offline_tpl.clone());
        add_template(KIND, get_tag(&Event::Custom), o.config.custom_tpl.clone());
        add_template(KIND, get_tag(&Event::AlertRecovered), o.config.recovery_tpl.clone());
        o
    }
}

impl crate::notifier::Notifier for Email {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn send_notify(&self, html_content: String) -> Result<()> {
        self.send_notify_with_context(html_content, "NotifyTest", "-")
    }

    fn send_notify_with_context(&self, html_content: String, event: &str, host: &str) -> Result<()> {
        let mut builder = Message::builder()
            .subject(self.config.subject.clone())
            .from(format!("ServerStatus <{}>", self.config.username).parse().unwrap());

        let mailboxes: Mailboxes = self.config.to.parse().expect("Invalid email addresses");
        for mailbox in mailboxes.iter() {
            builder = builder.to(mailbox.clone());
        }

        let email = builder
            .multipart(
                MultiPart::alternative().singlepart(
                    SinglePart::builder()
                        .header(header::ContentType::TEXT_HTML)
                        .body(html_content),
                ),
            )
            .unwrap();

        let creds = Credentials::new(self.config.username.clone(), self.config.password.clone());

        let smtp_server = self.config.server.clone();
        let event = event.to_string();
        let host = host.to_string();
        let handle = NOTIFIER_HANDLE.lock().unwrap().as_ref().unwrap().clone();
        handle.spawn(async move {
            let mailer: AsyncSmtpTransport<Tokio1Executor> =
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(smtp_server.as_str())
                    .unwrap()
                    .credentials(creds)
                    .build();

            // Send the email
            match mailer.send(email).await {
                Ok(_) => {
                    info!("alert delivery sent notifier=email event={} host={}", event, host);
                }
                Err(err) => {
                    error!("alert delivery failed notifier=email event={} host={} error={err}", event, host);
                }
            }
        });

        Ok(())
    }

    fn notify(&self, e: &Event, stat: &HostStat, dispatch: &AlertDispatch) -> Result<NotifyOutcome> {
        let content = render_template(
            self.kind(),
            get_tag(e),
            context!(host => stat, config => self.config, alerts => self.alerts, recoveries => &dispatch.recovered, ip_info => stat.ip_info, sys_info => stat.sys_info),
            true,
        )?;
        let content = if matches!(e, Event::Custom) {
            select_custom_content(
                &content,
                dispatch.allowed_rule_ids.as_deref(),
                &dispatch.suppressed_rule_ids,
            )
        } else {
            content
        };
        info!("render.alert.tpl notifier={} event={e:?} host={} content_empty={}", self.kind(), stat.name, content.is_empty());
        if content.is_empty() {
            info!("alert delivery skipped notifier={} event={e:?} host={} reason=empty_template", self.kind(), stat.name);
            return Ok(NotifyOutcome::Skipped { reason: "empty_template" });
        }
        info!("alert delivery attempt notifier={} event={e:?} host={} gid={}", self.kind(), stat.name, stat.gid);
        self.send_notify_with_context(format!("{}\n{}", self.config.title, content), get_tag(e), &stat.name)?;
        Ok(NotifyOutcome::Queued)
    }
}
