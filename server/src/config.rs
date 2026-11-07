#![deny(warnings)]
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::notifier;

fn default_as_true() -> bool {
    true
}
fn default_grpc_addr() -> String {
    "0.0.0.0:9394".to_string()
}
fn default_http_addr() -> String {
    "0.0.0.0:8080".to_string()
}
fn default_workspace() -> String {
    "/opt/ServerStatus".to_string()
}
fn default_tls_dir() -> String {
    "tls".to_string()
}
fn default_history_backend() -> String {
    "sqlite".to_string()
}
fn default_history_path() -> String {
    "data/history.db".to_string()
}
fn default_history_sample_interval() -> u64 {
    1
}
fn default_history_allowed_lateness() -> u64 {
    300
}
fn default_history_maintenance_interval() -> u64 {
    30
}
fn default_history_max_query_points() -> usize {
    1200
}
fn default_history_retention_days() -> u64 {
    30
}
fn default_history_cleanup_interval() -> u64 {
    3600
}
fn default_history_delete_batch_size() -> usize {
    1000
}
fn default_history_compression() -> String {
    "latest".to_string()
}

fn default_alerts_state_path() -> String {
    "data/alerts.db".to_string()
}

fn default_alerts_true() -> bool {
    true
}

fn default_disk_alert_threshold() -> f64 {
    80.0
}

fn default_disk_alert_recover_threshold() -> f64 {
    75.0
}

fn default_memory_alert_threshold() -> f64 {
    80.0
}

fn default_memory_alert_recover_threshold() -> f64 {
    75.0
}

fn default_alert_convergence() -> Vec<AlertConvergenceWindow> {
    vec![
        AlertConvergenceWindow {
            max_notifications: Some(3),
            interval: "5m".to_string(),
        },
        AlertConvergenceWindow {
            max_notifications: Some(3),
            interval: "10m".to_string(),
        },
        AlertConvergenceWindow {
            max_notifications: None,
            interval: "60m".to_string(),
        },
    ]
}

fn default_alert_convergence_rules() -> Vec<AlertConvergenceRule> {
    vec![AlertConvergenceRule {
        id: "resource-default".to_string(),
        alert_types: vec!["disk_usage_high".to_string(), "memory_usage_high".to_string()],
        windows: default_alert_convergence(),
    }]
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AlertConvergenceWindow {
    /// Maximum number of notifications in this stage. `None` means unlimited.
    #[serde(default)]
    pub max_notifications: Option<u32>,
    /// Duration such as `5m`, `10m` or `60m`.
    pub interval: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AlertConvergenceRule {
    pub id: String,
    /// Alert identifiers to which this rule applies. `*` applies to all
    /// alert identifiers that do not have a more specific rule.
    #[serde(default)]
    pub alert_types: Vec<String>,
    #[serde(default = "default_alert_convergence")]
    pub windows: Vec<AlertConvergenceWindow>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AlertEventConfig {
    #[serde(default = "default_alerts_true")]
    pub enabled: bool,
    /// `None` means all enabled channels; an empty list explicitly disables
    /// delivery while keeping the event state and logs active.
    #[serde(default)]
    pub channels: Option<Vec<String>>,
    #[serde(default)]
    pub exclude_channels: Vec<String>,
    #[serde(default)]
    pub templates: AlertEventTemplates,
}

impl Default for AlertEventConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            channels: None,
            exclude_channels: Vec::new(),
            templates: AlertEventTemplates::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AlertEventTemplates {
    /// Optional event title. Empty titles fall back to the channel title.
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub html: String,
    #[serde(default, alias = "txt", alias = "plain")]
    pub text: String,
    /// Structured JSON or a Jinja template that renders JSON for Webhook.
    #[serde(default, alias = "json", alias = "webhook")]
    pub structured: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiskUsageAlertConfig {
    #[serde(default = "default_alerts_true")]
    pub enabled: bool,
    #[serde(default = "default_disk_alert_threshold")]
    pub threshold: f64,
    #[serde(default = "default_disk_alert_recover_threshold")]
    pub recover_threshold: f64,
}

impl Default for DiskUsageAlertConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold: default_disk_alert_threshold(),
            recover_threshold: default_disk_alert_recover_threshold(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MemoryUsageAlertConfig {
    #[serde(default = "default_alerts_true")]
    pub enabled: bool,
    #[serde(default = "default_memory_alert_threshold")]
    pub threshold: f64,
    #[serde(default = "default_memory_alert_recover_threshold")]
    pub recover_threshold: f64,
}

impl Default for MemoryUsageAlertConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold: default_memory_alert_threshold(),
            recover_threshold: default_memory_alert_recover_threshold(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AlertsConfig {
    #[serde(default = "default_alerts_true")]
    pub enabled: bool,
    #[serde(default = "default_alerts_state_path")]
    pub state_path: String,
    #[serde(default = "default_alerts_true")]
    pub log_suppressed: bool,
    #[serde(default = "default_alerts_true")]
    pub log_delivery: bool,
    #[serde(default = "default_alerts_true")]
    pub offline_log: bool,
    #[serde(default)]
    pub disk_usage: DiskUsageAlertConfig,
    #[serde(default)]
    pub memory_usage: MemoryUsageAlertConfig,
    #[serde(default = "default_alert_convergence")]
    pub convergence: Vec<AlertConvergenceWindow>,
    #[serde(default)]
    pub convergence_rules: Vec<AlertConvergenceRule>,
    /// Event-to-channel routing. Missing events retain the legacy broadcast
    /// behavior; configured events can target `all` or named channels.
    #[serde(default)]
    pub events: HashMap<String, AlertEventConfig>,
}

impl Default for AlertsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            state_path: default_alerts_state_path(),
            log_suppressed: true,
            log_delivery: true,
            offline_log: true,
            disk_usage: DiskUsageAlertConfig::default(),
            memory_usage: MemoryUsageAlertConfig::default(),
            convergence: default_alert_convergence(),
            convergence_rules: default_alert_convergence_rules(),
            events: HashMap::new(),
        }
    }
}

impl AlertsConfig {
    pub fn normalize(&mut self) {
        if self.convergence.is_empty() {
            self.convergence = default_alert_convergence();
        }
        if self.convergence_rules.is_empty() {
            self.convergence_rules = vec![AlertConvergenceRule {
                id: "resource-default".to_string(),
                alert_types: vec!["disk_usage_high".to_string(), "memory_usage_high".to_string()],
                windows: self.convergence.clone(),
            }];
        }
        for rule in &mut self.convergence_rules {
            if rule.id.trim().is_empty() {
                rule.id = "unnamed".to_string();
            }
            if rule.windows.is_empty() {
                rule.windows = default_alert_convergence();
            }
            for alert_type in &mut rule.alert_types {
                *alert_type = alert_type.trim().to_ascii_lowercase();
            }
        }
        let mut normalized_events = HashMap::new();
        for (event, mut policy) in std::mem::take(&mut self.events) {
            let event = normalize_alert_event_name(&event);
            if let Some(channels) = policy.channels.as_mut() {
                for channel in channels.iter_mut() {
                    *channel = channel.trim().to_ascii_lowercase();
                }
                channels.sort();
                channels.dedup();
            }
            for channel in &mut policy.exclude_channels {
                *channel = channel.trim().to_ascii_lowercase();
            }
            policy.exclude_channels.sort();
            policy.exclude_channels.dedup();
            normalized_events.insert(event, policy);
        }
        self.events = normalized_events;
        self.disk_usage.threshold = self.disk_usage.threshold.clamp(0.0, 100.0);
        self.disk_usage.recover_threshold = self.disk_usage.recover_threshold.clamp(0.0, 100.0);
        if self.disk_usage.recover_threshold > self.disk_usage.threshold {
            self.disk_usage.recover_threshold = self.disk_usage.threshold;
        }
        self.memory_usage.threshold = self.memory_usage.threshold.clamp(0.0, 100.0);
        self.memory_usage.recover_threshold = self.memory_usage.recover_threshold.clamp(0.0, 100.0);
        if self.memory_usage.recover_threshold > self.memory_usage.threshold {
            self.memory_usage.recover_threshold = self.memory_usage.threshold;
        }
    }

    pub fn allowed_event_types(&self, event_types: &[String], channel: &str) -> Vec<String> {
        if self.events.is_empty() {
            return event_types.to_vec();
        }
        let channel = channel.trim().to_ascii_lowercase();
        event_types
            .iter()
            .filter(|event| {
                let Some(policy) = self.events.get(&normalize_alert_event_name(event)) else {
                    return true;
                };
                policy.enabled
                    && !policy.exclude_channels.iter().any(|value| value == &channel)
                    && policy
                        .channels
                        .as_ref()
                        .is_none_or(|channels| channels.iter().any(|value| value == "all" || value == &channel))
            })
            .cloned()
            .collect()
    }
}

pub fn normalize_alert_event_name(event: &str) -> String {
    match event.trim().to_ascii_lowercase().as_str() {
        "nodeup" | "node_up" => "node_up".to_string(),
        "nodedown" | "node_down" => "node_down".to_string(),
        "custom" => "custom".to_string(),
        "alertrecovered" | "alert_recovered" => "alert_recovered".to_string(),
        value => value.to_string(),
    }
}

fn default_history_tiers() -> Vec<HistoryTierConfig> {
    [
        ("1s", "15m"),
        ("1m", "30d"),
        ("5m", "90d"),
        ("1h", "1y"),
        ("1d", "3y"),
        ("1mo", "10y"),
    ]
    .into_iter()
    .map(|(resolution, retention)| HistoryTierConfig {
        resolution: resolution.to_string(),
        retention: retention.to_string(),
    })
    .collect()
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct HistoryTierConfig {
    pub resolution: String,
    pub retention: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HistoryConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_history_backend")]
    pub backend: String,
    #[serde(default = "default_history_path")]
    pub path: String,
    /// `none` stores every received sample; `latest` keeps the latest sample in each bucket.
    #[serde(default = "default_history_compression")]
    pub compression: String,
    #[serde(default = "default_history_sample_interval")]
    pub sample_interval: u64,
    /// Maximum accepted delay between the agent event timestamp and server receive time.
    #[serde(default = "default_history_allowed_lateness")]
    pub allowed_lateness: u64,
    #[serde(default = "default_history_maintenance_interval")]
    pub maintenance_interval: u64,
    #[serde(default = "default_history_max_query_points")]
    pub max_query_points: usize,
    // Empty when omitted from an existing configuration. The history parser
    // then falls back to legacy single-tier fields; Default still supplies
    // the new six-tier policy for programmatic callers.
    #[serde(default)]
    pub tiers: Vec<HistoryTierConfig>,
    #[serde(default = "default_history_retention_days")]
    pub retention_days: u64,
    #[serde(default = "default_history_cleanup_interval")]
    pub cleanup_interval: u64,
    #[serde(default = "default_history_delete_batch_size")]
    pub delete_batch_size: usize,
    #[serde(default)]
    pub vacuum_after_cleanup: bool,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            // Keep legacy server configurations side-effect free.  The
            // distributed config.toml explicitly enables history.
            enabled: false,
            backend: default_history_backend(),
            path: default_history_path(),
            compression: default_history_compression(),
            sample_interval: default_history_sample_interval(),
            allowed_lateness: default_history_allowed_lateness(),
            maintenance_interval: default_history_maintenance_interval(),
            max_query_points: default_history_max_query_points(),
            tiers: default_history_tiers(),
            retention_days: default_history_retention_days(),
            cleanup_interval: default_history_cleanup_interval(),
            delete_batch_size: default_history_delete_batch_size(),
            vacuum_after_cleanup: false,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Host {
    pub name: String,
    pub password: String,
    #[serde(default = "Default::default")]
    pub alias: String,
    #[serde(default = "Default::default")]
    pub location: String,
    #[serde(default = "Default::default")]
    pub r#type: String,
    #[serde(default = "u32::default")]
    pub monthstart: u32,
    #[serde(default = "default_as_true")]
    pub notify: bool,
    #[serde(default = "bool::default")]
    pub disabled: bool,
    #[serde(default = "Default::default")]
    pub labels: String,
    /// Operator-managed grouping tags. These are never accepted from agents.
    #[serde(default = "Default::default")]
    pub tags: Vec<String>,

    #[serde(skip_deserializing)]
    pub last_network_in: u64,
    #[serde(skip_deserializing)]
    pub last_network_out: u64,

    // user data
    #[serde(skip_serializing, skip_deserializing)]
    pub pos: usize,
    #[serde(default = "Default::default", skip_serializing)]
    pub weight: u64,
    #[serde(default = "Default::default")]
    pub gid: String,
    #[serde(default = "Default::default")]
    pub latest_ts: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HostGroup {
    pub gid: String,
    pub password: String,
    #[serde(default = "Default::default")]
    pub location: String,
    #[serde(default = "Default::default")]
    pub r#type: String,
    #[serde(default = "default_as_true")]
    pub notify: bool,
    // user data
    #[serde(skip_serializing, skip_deserializing)]
    pub pos: usize,
    #[serde(default = "Default::default", skip_serializing)]
    pub weight: u64,
    #[serde(default = "Default::default")]
    pub labels: String,
    #[serde(default = "Default::default")]
    pub tags: Vec<String>,
}

impl HostGroup {
    pub fn inst_host(&self, name: &str) -> Host {
        Host {
            name: name.to_owned(),
            gid: self.gid.clone(),
            password: self.password.clone(),
            location: self.location.clone(),
            r#type: self.r#type.clone(),
            monthstart: 1,
            notify: self.notify,
            pos: self.pos,
            weight: self.weight,
            labels: self.labels.clone(),
            tags: self.tags.clone(),
            ..Default::default()
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    #[serde(default = "default_http_addr")]
    pub http_addr: String,
    #[serde(default = "default_grpc_addr")]
    pub grpc_addr: String,
    #[serde(default = "Default::default")]
    pub notify_interval: u64,
    #[serde(default = "Default::default")]
    pub offline_threshold: u64,
    #[serde(default = "Default::default")]
    pub grpc_tls: u32,
    #[serde(default = "default_tls_dir")]
    pub tls_dir: String,
    #[serde(default)]
    pub history: HistoryConfig,
    #[serde(default)]
    pub alerts: AlertsConfig,
    // admin user & pass
    pub admin_user: Option<String>,
    pub admin_pass: Option<String>,
    pub jwt_secret: Option<String>,

    #[serde(default = "Default::default")]
    pub tgbot: notifier::tgbot::Config,
    #[serde(default = "Default::default")]
    pub wechat: notifier::wechat::Config,
    #[serde(default = "Default::default")]
    pub email: notifier::email::Config,
    #[serde(default = "Default::default")]
    pub log: notifier::log::Config,
    #[serde(default = "Default::default")]
    pub webhook: notifier::webhook::Config,

    #[serde(default = "Default::default")]
    pub hosts: Vec<Host>,
    #[serde(default = "Default::default")]
    pub hosts_group: Vec<HostGroup>,
    #[serde(default = "Default::default")]
    pub group_gc: u64,

    // deploy
    #[serde(default = "Default::default")]
    pub server_url: String,
    #[serde(default = "default_workspace")]
    pub workspace: String,

    #[serde(skip_deserializing)]
    pub hosts_map: HashMap<String, Host>,

    #[serde(skip_deserializing)]
    pub hosts_group_map: HashMap<String, HostGroup>,
}

impl Config {
    pub fn auth(&self, user: &str, pass: &str) -> bool {
        if let Some(o) = self.hosts_map.get(user) {
            return pass.eq(o.password.as_str());
        }
        false
    }
    pub fn group_auth(&self, gid: &str, pass: &str) -> bool {
        if let Some(o) = self.hosts_group_map.get(gid) {
            return pass.eq(o.password.as_str());
        }
        false
    }
    pub fn admin_auth(&self, user: &str, pass: &str) -> bool {
        if let (Some(u), Some(p)) = (self.admin_user.as_ref(), self.admin_pass.as_ref()) {
            return user.eq(u.as_str()) && pass.eq(p.as_str());
        }
        false
    }

    pub fn to_json_value(&self) -> Result<Value> {
        serde_json::to_value(self).map_err(anyhow::Error::new)
    }

    // pub fn to_string(&self) -> Result<String> {
    //     serde_json::to_string(&self).map_err(anyhow::Error::new)
    // }
}

pub fn from_str(content: &str) -> Option<Config> {
    let raw = toml::from_str::<toml::Value>(content).ok();
    let mut o = toml::from_str::<Config>(content).unwrap();
    o.alerts.normalize();
    o.hosts_map = HashMap::new();

    log_alert_config_sources(raw.as_ref(), &o.alerts);
    log_custom_template_mapping(&o);

    for (idx, host) in o.hosts.iter_mut().enumerate() {
        normalize_tags(&mut host.tags);
        host.pos = idx;
        if host.alias.is_empty() {
            host.alias = host.name.clone();
        }
        if host.monthstart < 1 || host.monthstart > 31 {
            host.monthstart = 1;
        }
        host.weight = 10000_u64 - idx as u64;
        o.hosts_map.insert(host.name.clone(), host.clone());
    }

    for (idx, group) in o.hosts_group.iter_mut().enumerate() {
        normalize_tags(&mut group.tags);
        group.pos = idx;
        group.weight = (10000 - (1 + idx) * 100) as u64;
        o.hosts_group_map.insert(group.gid.clone(), group.clone());
    }

    if o.offline_threshold < 30 {
        o.offline_threshold = 30;
    }
    if o.notify_interval < 30 {
        o.notify_interval = 30;
    }
    if o.group_gc < 30 {
        o.group_gc = 30;
    }

    if o.admin_user.is_none() || o.admin_user.as_ref()?.is_empty() {
        o.admin_user = Some("admin".to_string());
    }
    if o.admin_pass.is_none() || o.admin_pass.as_ref()?.is_empty() {
        o.admin_pass = Some(Uuid::new_v4().to_string());
    }
    if o.jwt_secret.is_none() || o.jwt_secret.as_ref()?.is_empty() {
        o.jwt_secret = Some(Uuid::new_v4().to_string());
    }

    if content
        .parse::<toml::Value>()
        .ok()
        .and_then(|value| value.get("admin_pass").cloned())
        .and_then(|value| value.as_str().map(str::is_empty))
        .unwrap_or(true)
    {
        log::warn!("config generated key=admin_pass source=generated value=redacted");
    }
    if content
        .parse::<toml::Value>()
        .ok()
        .and_then(|value| value.get("jwt_secret").cloned())
        .and_then(|value| value.as_str().map(str::is_empty))
        .unwrap_or(true)
    {
        log::warn!("config generated key=jwt_secret source=generated value=redacted");
    }

    Some(o)
}

fn log_alert_config_sources(raw: Option<&toml::Value>, alerts: &AlertsConfig) {
    let Some(alerts_value) = raw.and_then(|value| value.get("alerts")) else {
        log::info!(
            "config preset selected section=alerts key=enabled value={} source=default reason=missing",
            alerts.enabled
        );
        log::info!(
            "config preset selected section=alerts key=state_path value={} source=default reason=missing",
            alerts.state_path
        );
        log::info!("config preset selected section=alerts key=log_suppressed value=true source=default reason=missing");
        log::info!("config preset selected section=alerts key=log_delivery value=true source=default reason=missing");
        log::info!("config preset selected section=alerts key=offline_log value=true source=default reason=missing");
        log::info!("config preset selected section=alerts key=disk_usage.enabled value=true source=default reason=missing");
        log::info!(
            "config preset selected section=alerts key=disk_usage.threshold value={} source=default reason=missing",
            alerts.disk_usage.threshold
        );
        log::info!("config preset selected section=alerts key=disk_usage.recover_threshold value={} source=default reason=missing", alerts.disk_usage.recover_threshold);
        log::info!("config preset selected section=alerts key=memory_usage.enabled value=true source=default reason=missing");
        log::info!(
            "config preset selected section=alerts key=memory_usage.threshold value={} source=default reason=missing",
            alerts.memory_usage.threshold
        );
        log::info!("config preset selected section=alerts key=memory_usage.recover_threshold value={} source=default reason=missing", alerts.memory_usage.recover_threshold);
        for (rule_idx, rule) in alerts.convergence_rules.iter().enumerate() {
            for (window_idx, window) in rule.windows.iter().enumerate() {
                log::info!("config preset selected section=alerts key=convergence_rules[{rule_idx}].windows[{window_idx}] value={} source=default reason=missing", window.interval);
            }
        }
        log::info!(
            "config effective section=alerts enabled={} state_path={} convergence_rules={}",
            alerts.enabled,
            alerts.state_path,
            alerts.convergence_rules.len()
        );
        return;
    };

    let Some(table) = alerts_value.as_table() else {
        log::warn!("config adjusted path=alerts reason=expected_table action=defaults");
        return;
    };
    for (key, value) in [
        ("enabled", alerts.enabled.to_string()),
        ("state_path", alerts.state_path.clone()),
        ("log_suppressed", alerts.log_suppressed.to_string()),
        ("log_delivery", alerts.log_delivery.to_string()),
        ("offline_log", alerts.offline_log.to_string()),
    ] {
        if !table.contains_key(key) {
            log::info!("config preset selected section=alerts key={key} value={value} source=default reason=missing");
        }
    }
    log_alert_metric_sources(
        table,
        "disk_usage",
        alerts.disk_usage.enabled,
        alerts.disk_usage.threshold,
        alerts.disk_usage.recover_threshold,
    );
    log_alert_metric_sources(
        table,
        "memory_usage",
        alerts.memory_usage.enabled,
        alerts.memory_usage.threshold,
        alerts.memory_usage.recover_threshold,
    );
    if !table.contains_key("convergence_rules") && !table.contains_key("convergence") {
        log::info!("config preset selected section=alerts key=convergence_rules source=default reason=missing");
    }
    log::info!(
        "config effective section=alerts enabled={} state_path={} convergence_rules={}",
        alerts.enabled,
        alerts.state_path,
        alerts.convergence_rules.len()
    );
    for (event, policy) in &alerts.events {
        let channels = policy
            .channels
            .as_ref()
            .map(|values| values.join(","))
            .unwrap_or_else(|| "all".to_string());
        log::info!(
            "config effective alert_route event={} enabled={} channels={} excluded={}",
            event,
            policy.enabled,
            channels,
            policy.exclude_channels.join(",")
        );
    }
}

fn log_alert_metric_sources(
    alerts_table: &toml::map::Map<String, toml::Value>,
    section: &str,
    enabled: bool,
    threshold: f64,
    recover_threshold: f64,
) {
    let metric_table = alerts_table.get(section).and_then(toml::Value::as_table);
    let fields = [
        ("enabled", enabled.to_string()),
        ("threshold", threshold.to_string()),
        ("recover_threshold", recover_threshold.to_string()),
    ];
    for (key, value) in fields {
        if metric_table.is_none_or(|table| !table.contains_key(key)) {
            log::info!(
                "config preset selected section=alerts key={section}.{key} value={value} source=default reason=missing"
            );
        }
    }
}

fn log_custom_template_mapping(config: &Config) {
    let known_rule_types = config
        .alerts
        .convergence_rules
        .iter()
        .flat_map(|rule| rule.alert_types.iter())
        .map(|value| value.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    for (kind, template) in [
        ("tgbot", &config.tgbot.custom_tpl),
        ("wechat", &config.wechat.custom_tpl),
        ("email", &config.email.custom_tpl),
    ] {
        if !template.trim().is_empty() && !template.contains("ss-alert:") {
            log::warn!(
                "custom template is unclassified notifier={} action=legacy_whole_event; add <!-- ss-alert:<rule_id> --> blocks for exact convergence",
                kind
            );
        }
        for marker in template.lines().filter_map(parse_alert_marker) {
            if !known_rule_types.contains(&marker) && !known_rule_types.contains("*") {
                log::warn!(
                    "custom template marker has no convergence rule notifier={} alert_type={} action=send_without_convergence",
                    kind,
                    marker
                );
            }
        }
    }
}

fn parse_alert_marker(line: &str) -> Option<String> {
    line.trim()
        .strip_prefix("<!-- ss-alert:")
        .and_then(|value| value.strip_suffix(" -->"))
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

fn normalize_tags(tags: &mut Vec<String>) {
    let mut seen = HashSet::new();
    tags.retain_mut(|tag| {
        *tag = tag.trim().to_string();
        !tag.is_empty() && seen.insert(tag.to_lowercase())
    });
}

pub fn from_env() -> Option<Config> {
    from_str(
        env::var("SRV_CONF")
            .expect("can't load config from env `SRV_CONF")
            .as_str(),
    )
}

pub fn from_file(cfg: &str) -> Option<Config> {
    fs::read_to_string(cfg)
        .map(|contents| from_str(contents.as_str()))
        .ok()?
}

pub fn test_from_file(cfg: &str) -> Result<Config> {
    fs::read_to_string(cfg)
        .map(|contents| toml::from_str::<Config>(&contents))
        .unwrap()
        .map_err(anyhow::Error::new)
}

#[derive(Debug)]
pub struct MigrationReport {
    pub message: String,
    pub backup: Option<PathBuf>,
}

pub fn migrate_file(input: &str, output: Option<&str>, dry_run: bool, force: bool) -> Result<MigrationReport> {
    let input_path = Path::new(input);
    let original = fs::read_to_string(input_path)
        .map_err(|error| anyhow::anyhow!("read config {}: {error}", input_path.display()))?;
    toml::from_str::<toml::Value>(&original)
        .map_err(|error| anyhow::anyhow!("invalid TOML in {}: {error}", input_path.display()))?;
    let (migrated, changes) = migrate_config_text(&original);
    toml::from_str::<toml::Value>(&migrated)
        .map_err(|error| anyhow::anyhow!("generated migrated TOML is invalid: {error}"))?;

    let destination = output.map(PathBuf::from).unwrap_or_else(|| input_path.to_path_buf());
    if destination.exists() && destination != input_path && !force && !dry_run {
        anyhow::bail!("output {} already exists; pass --force to replace it", destination.display());
    }
    if dry_run {
        eprintln!("--- migrated config preview ---\n{migrated}--- end preview ---");
        return Ok(MigrationReport { message: format!("migration preview complete; changes={changes}"), backup: None });
    }

    let backup = backup_path(input_path);
    fs::copy(input_path, &backup).map_err(|error| {
        anyhow::anyhow!("backup config {} -> {} failed: {error}", input_path.display(), backup.display())
    })?;
    if destination != input_path || migrated != original {
        atomic_write(&destination, migrated.as_bytes())?;
    }
    Ok(MigrationReport {
        message: if migrated == original { "configuration already uses the current alert format; changes=0".to_string() } else { format!("configuration migrated successfully; changes={changes}") },
        backup: Some(backup),
    })
}

fn backup_path(input: &Path) -> PathBuf {
    let stamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let base = PathBuf::from(format!("{}.bak.{stamp}", input.display()));
    if !base.exists() {
        return base;
    }
    for suffix in 1_u32.. {
        let candidate = PathBuf::from(format!("{}.bak.{stamp}.{suffix}", input.display()));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!("backup suffix range exhausted")
}

fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("config.toml");
    let temporary = parent.join(format!(".{file_name}.migrate.tmp.{}", std::process::id()));
    fs::write(&temporary, content).map_err(|error| anyhow::anyhow!("write temporary config {} failed: {error}", temporary.display()))?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(anyhow::anyhow!("replace config {} failed: {error}", path.display()));
    }
    Ok(())
}

fn migrate_config_text(original: &str) -> (String, usize) {
    let mut text = original.to_string();
    let mut changes = 0;
    if !has_table(&text, "alerts") {
        let block = "# Alert configuration migrated by stat_server config migrate.\n[alerts]\nenabled = true\nstate_path = \"data/alerts.db\"\nlog_suppressed = true\nlog_delivery = true\noffline_log = true\n\n[alerts.disk_usage]\nenabled = true\nthreshold = 80.0\nrecover_threshold = 75.0\n\n[alerts.memory_usage]\nenabled = true\nthreshold = 80.0\nrecover_threshold = 75.0\n\n[[alerts.convergence_rules]]\nid = \"resource-default\"\nalert_types = [\"disk_usage_high\", \"memory_usage_high\"]\n\n[[alerts.convergence_rules.windows]]\nmax_notifications = 3\ninterval = \"5m\"\n\n[[alerts.convergence_rules.windows]]\nmax_notifications = 3\ninterval = \"10m\"\n\n[[alerts.convergence_rules.windows]]\ninterval = \"60m\"\n\n";
        let insert_at = text.find("[tgbot]").or_else(|| text.find("[wechat]")).or_else(|| text.find("[email]")).unwrap_or(text.len());
        text.insert_str(insert_at, block);
        changes += 1;
    } else {
        let (updated, root_changes) = migrate_alert_root_fields(&text);
        text = updated;
        changes += root_changes;

        let (updated, disk_changes) = migrate_alert_metric_section(&text, "disk_usage", "disk");
        text = updated;
        changes += disk_changes;
        let (updated, memory_changes) = migrate_alert_metric_section(&text, "memory_usage", "memory");
        text = updated;
        changes += memory_changes;
    }
    if !text.contains("[[alerts.convergence_rules]]") {
        text.push_str("\n# Alert convergence rule added by stat_server config migrate.\n[[alerts.convergence_rules]]\nid = \"resource-default\"\nalert_types = [\"disk_usage_high\", \"memory_usage_high\"]\n\n[[alerts.convergence_rules.windows]]\nmax_notifications = 3\ninterval = \"5m\"\n\n[[alerts.convergence_rules.windows]]\nmax_notifications = 3\ninterval = \"10m\"\n\n[[alerts.convergence_rules.windows]]\ninterval = \"60m\"\n");
        changes += 1;
    }
    if !text.contains("[alerts.events.") {
        text.push_str("\n# Event routing added by stat_server config migrate.\n# `all` preserves the legacy broadcast behavior; narrow these lists as needed.\n[alerts.events.disk_usage_high]\nchannels = [\"all\"]\n\n[alerts.events.disk_usage_recovered]\nchannels = [\"all\"]\n\n[alerts.events.memory_usage_high]\nchannels = [\"all\"]\n\n[alerts.events.memory_usage_recovered]\nchannels = [\"all\"]\n\n[alerts.events.node_down]\nchannels = [\"all\"]\n\n[alerts.events.node_up]\nchannels = [\"all\"]\n");
        changes += 1;
    }
    for (kind, field) in [("memory", "memory_usage"), ("disk", "disk_usage")] {
        let before = text.clone();
        text = migrate_templates(&text, kind, field);
        if text != before { changes += 1; }
    }
    let before = text.clone();
    text = migrate_recovery_templates(&text);
    if text != before { changes += 1; }
    (text, changes)
}

fn migrate_recovery_templates(content: &str) -> String {
    let recovery = "{% for recovery in recoveries %}✅ {{host.name}} {{recovery.alert_type}} 已恢复，当前 {{recovery.value_percent | round}}%，恢复阈值 {{recovery.recover_threshold}}%{% endfor %}";
    let sections = ["tgbot", "wechat", "email"];
    let lines = content.lines().collect::<Vec<_>>();
    let mut output = Vec::with_capacity(lines.len() + sections.len());
    let mut current_section = false;
    let mut has_recovery = false;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if current_section && !has_recovery {
                output.push(format!("recovery_tpl = \"{recovery}\""));
            }
            current_section = trimmed == "[tgbot]" || trimmed == "[wechat]" || trimmed == "[email]";
            has_recovery = false;
        }
        if current_section && trimmed.starts_with("recovery_tpl") {
            has_recovery = true;
        }
        output.push(line.to_string());
    }
    if current_section && !has_recovery {
        output.push(format!("recovery_tpl = \"{recovery}\""));
    }
    let mut result = output.join("\n");
    if content.ends_with('\n') { result.push('\n'); }
    result
}

fn migrate_alert_root_fields(content: &str) -> (String, usize) {
    let lines = content.lines().collect::<Vec<_>>();
    let Some(root_index) = lines.iter().position(|line| line.trim() == "[alerts]") else {
        // A dotted table such as [alerts.disk_usage] creates the parent table
        // implicitly. Insert an explicit parent section before it so root
        // defaults do not accidentally become children of a nested section.
        let Some(nested_index) = lines.iter().position(|line| line.trim().starts_with("[alerts.")) else {
            return (content.to_string(), 0);
        };
        let block = "[alerts]\nenabled = true\nstate_path = \"data/alerts.db\"\nlog_suppressed = true\nlog_delivery = true\noffline_log = true\n\n";
        let mut output = lines[..nested_index].join("\n");
        if !output.is_empty() { output.push('\n'); }
        output.push_str(block);
        output.push_str(&lines[nested_index..].join("\n"));
        if content.ends_with('\n') { output.push('\n'); }
        return (output, 1);
    };

    let end = lines[root_index + 1..]
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .map(|offset| root_index + 1 + offset)
        .unwrap_or(lines.len());
    let keys = lines[root_index + 1..end]
        .iter()
        .filter_map(|line| line.split_once('=').map(|(key, _)| key.trim()))
        .collect::<HashSet<_>>();
    let defaults = [
        ("enabled", "true"),
        ("state_path", "\"data/alerts.db\""),
        ("log_suppressed", "true"),
        ("log_delivery", "true"),
        ("offline_log", "true"),
    ];
    let missing = defaults
        .iter()
        .filter(|(key, _)| !keys.contains(key))
        .map(|(key, value)| format!("{key} = {value}"))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return (content.to_string(), 0);
    }
    let mut updated = lines[..end].join("\n");
    if !updated.ends_with('\n') { updated.push('\n'); }
    updated.push_str(&missing.join("\n"));
    updated.push('\n');
    updated.push_str(&lines[end..].join("\n"));
    if content.ends_with('\n') && !updated.ends_with('\n') { updated.push('\n'); }
    (updated, missing.len())
}

fn migrate_alert_metric_section(content: &str, section: &str, kind: &str) -> (String, usize) {
    let header = format!("[alerts.{section}]");
    let lines = content.lines().collect::<Vec<_>>();
    let Some(start) = lines.iter().position(|line| line.trim() == header) else {
        let defaults = metric_section_defaults(section);
        let insertion = format!("\n{defaults}\n");
        let mut updated = content.trim_end_matches('\n').to_string();
        updated.push_str(&insertion);
        if content.ends_with('\n') { updated.push('\n'); }
        return (updated, 1);
    };
    let end = lines[start + 1..]
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .map(|offset| start + 1 + offset)
        .unwrap_or(lines.len());
    let keys = lines[start + 1..end]
        .iter()
        .filter_map(|line| line.split_once('=').map(|(key, _)| key.trim()))
        .collect::<HashSet<_>>();
    let defaults = metric_defaults(section, kind);
    let missing = defaults
        .iter()
        .filter(|(key, _)| !keys.contains(key))
        .map(|(key, value)| format!("{key} = {value}"))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return (content.to_string(), 0);
    }
    let mut updated = lines[..end].join("\n");
    if !updated.ends_with('\n') { updated.push('\n'); }
    updated.push_str(&missing.join("\n"));
    updated.push('\n');
    updated.push_str(&lines[end..].join("\n"));
    if content.ends_with('\n') && !updated.ends_with('\n') { updated.push('\n'); }
    (updated, missing.len())
}

fn metric_defaults(section: &str, kind: &str) -> Vec<(&'static str, &'static str)> {
    match kind {
        "disk" => vec![("enabled", "true"), ("threshold", "80.0"), ("recover_threshold", "75.0")],
        "memory" => vec![("enabled", "true"), ("threshold", "80.0"), ("recover_threshold", "75.0")],
        _ => unreachable!("unsupported alert metric section {section}"),
    }
}

fn metric_section_defaults(section: &str) -> String {
    let kind = if section == "disk_usage" { "disk" } else { "memory" };
    let mut output = format!("[alerts.{section}]\n");
    for (key, value) in metric_defaults(section, kind) {
        output.push_str(&format!("{key} = {value}\n"));
    }
    output.trim_end().to_string()
}

fn has_table(content: &str, name: &str) -> bool {
    let header = format!("[{name}]");
    content.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == header || trimmed.starts_with(format!("[{name}.").as_str()) || trimmed.starts_with(format!("[[{name}.").as_str())
    })
}

fn migrate_templates(content: &str, kind: &str, field: &str) -> String {
    let mut output = Vec::new();
    let mut active = false;
    let alert_type = match kind {
        "memory" => "memory_usage_high",
        "disk" => "disk_usage_high",
        _ => field,
    };
    let start_marker = format!("<!-- ss-alert:{alert_type} -->");
    let lines = content.lines().collect::<Vec<_>>();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let is_condition = trimmed.starts_with("{% if") && trimmed.contains("host.") && trimmed.contains(field_prefix(kind)) && !trimmed.contains("ss-alert:");
        if is_condition {
            let indentation = &line[..line.len() - line.trim_start().len()];
            let normalized = match kind {
                "memory" => format!("{indentation}{{% if alerts.memory_usage.enabled and host.memory_total > 0 and (host.memory_used * 100.0 / host.memory_total) >= alerts.memory_usage.threshold %}}"),
                "disk" => format!("{indentation}{{% if alerts.disk_usage.enabled and host.hdd_total > 0 and (host.hdd_used * 100.0 / host.hdd_total) >= alerts.disk_usage.threshold %}}"),
                _ => line.to_string(),
            };
            output.push(normalized);
            if lines.get(index + 1).is_none_or(|next| !next.trim().starts_with("<!-- ss-alert:")) {
                output.push(start_marker.clone());
                active = true;
            }
        } else if active && trimmed == "{% endif %}" {
            output.push("<!-- /ss-alert -->".to_string());
            output.push(line.to_string());
            active = false;
        } else {
            output.push(line.to_string());
        }
    }
    let mut result = output.join("\n");
    if content.ends_with('\n') { result.push('\n'); }
    result
}

fn field_prefix(kind: &str) -> &'static str {
    match kind { "memory" => "memory_", "disk" => "hdd_", _ => "" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_config_does_not_enable_history_implicitly() {
        let cfg: Config = toml::from_str("").unwrap();
        assert!(!cfg.history.enabled);
        assert_eq!(cfg.history.backend, "sqlite");
    }

    #[test]
    fn legacy_history_section_does_not_implicitly_enable_new_tiers() {
        let cfg: Config = toml::from_str(
            r#"
                [history]
                enabled = true
                sample_interval = 60
                retention_days = 0
            "#,
        )
        .unwrap();
        assert!(cfg.history.enabled);
        assert!(cfg.history.tiers.is_empty());
        assert_eq!(cfg.history.sample_interval, 60);
        assert_eq!(cfg.history.retention_days, 0);
    }

    #[test]
    fn host_tags_are_trimmed_deduplicated_and_inherited_by_groups() {
        let mut tags = vec![
            " AWS ".to_string(),
            "mysql".to_string(),
            "aws".to_string(),
            String::new(),
        ];
        normalize_tags(&mut tags);
        assert_eq!(tags, vec!["AWS", "mysql"]);

        let group = HostGroup {
            gid: "mail".to_string(),
            password: "secret".to_string(),
            location: String::new(),
            r#type: String::new(),
            notify: true,
            pos: 0,
            weight: 0,
            labels: String::new(),
            tags: vec!["MAILSVR".to_string(), "AWS".to_string()],
        };
        assert_eq!(group.inst_host("node-1").tags, group.tags);
    }

    #[test]
    fn distributed_config_keeps_root_hosts_outside_history_table() {
        let cfg: Config = toml::from_str(include_str!("../../config.toml")).unwrap();
        assert!(cfg.history.enabled);
        assert!(!cfg.hosts.is_empty());
        assert_eq!(cfg.hosts[0].tags, vec!["AWS", "MYSQL"]);
    }

    #[test]
    fn fixed_hosts_accept_directory_gid() {
        let cfg = from_str(
            r#"
                hosts = [{name = "mail-1", password = "secret", gid = "MAILSVR"}]
            "#,
        )
        .unwrap();
        assert_eq!(cfg.hosts[0].gid, "MAILSVR");
        assert_eq!(cfg.hosts_map["mail-1"].gid, "MAILSVR");
    }

    #[test]
    fn old_alert_config_gets_default_disk_rule() {
        let cfg = from_str("").unwrap();
        assert_eq!(cfg.alerts.convergence_rules.len(), 1);
        assert_eq!(cfg.alerts.convergence_rules[0].id, "resource-default");
        assert_eq!(
            cfg.alerts.convergence_rules[0].alert_types,
            vec!["disk_usage_high", "memory_usage_high"]
        );
        assert_eq!(cfg.alerts.convergence_rules[0].windows[0].interval, "5m");
    }

    #[test]
    fn convergence_rule_can_target_multiple_alert_types() {
        let cfg = from_str(
            r#"
                [alerts]
                [[alerts.convergence_rules]]
                id = "resource"
                alert_types = ["disk_usage_high", "memory_usage_high"]
                [[alerts.convergence_rules.windows]]
                max_notifications = 1
                interval = "2m"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.alerts.convergence_rules[0].alert_types.len(), 2);
        assert_eq!(cfg.alerts.convergence_rules[0].windows[0].interval, "2m");
    }

    #[test]
    fn event_routes_select_named_channels_and_keep_legacy_defaults() {
        let cfg = from_str(
            r#"
                [alerts.events.disk_usage_high]
                channels = [" WeChat ", "tgbot"]
                [alerts.events.disk_usage_recovered]
                channels = []
            "#,
        )
        .unwrap();
        assert_eq!(
            cfg.alerts.allowed_event_types(&["disk_usage_high".to_string()], "wechat"),
            vec!["disk_usage_high"]
        );
        assert!(cfg.alerts.allowed_event_types(&["disk_usage_high".to_string()], "email").is_empty());
        assert!(cfg.alerts.allowed_event_types(&["disk_usage_recovered".to_string()], "wechat").is_empty());
        // An event without an explicit policy retains the old broadcast behavior.
        assert_eq!(
            cfg.alerts.allowed_event_types(&["node_down".to_string()], "email"),
            vec!["node_down"]
        );
    }

    #[test]
    fn migrate_old_alert_config_adds_complete_defaults_and_is_idempotent() {
        let old = r#"
http_addr = "0.0.0.0:8080"

[alerts]
enabled = false

[tgbot]
enabled = false
custom_tpl = """
{% if host.hdd_used / host.hdd_total > 0.8 %}
disk is high
{% endif %}
"""
"#;
        let (migrated, changes) = migrate_config_text(old);
        assert!(changes > 0);
        let value = toml::from_str::<toml::Value>(&migrated).unwrap();
        let alerts = value.get("alerts").unwrap().as_table().unwrap();
        assert_eq!(alerts.get("enabled").and_then(toml::Value::as_bool), Some(false));
        assert_eq!(alerts.get("state_path").and_then(toml::Value::as_str), Some("data/alerts.db"));
        assert!(alerts.get("convergence_rules").is_some());
        assert!(migrated.contains("recovery_tpl ="));
        let disk = alerts.get("disk_usage").unwrap().as_table().unwrap();
        assert_eq!(disk.get("threshold").and_then(toml::Value::as_float), Some(80.0));
        assert!(migrated.contains("alerts.disk_usage.threshold"));
        assert!(migrated.contains("<!-- ss-alert:disk_usage_high -->"));

        let (again, second_changes) = migrate_config_text(&migrated);
        assert_eq!(again, migrated);
        assert_eq!(second_changes, 0);
    }

    #[test]
    fn migrate_file_creates_backup_and_replaces_input_atomically() {
        let unique = format!(
            "server-status-config-migrate-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        let path = std::env::temp_dir().join(format!("{unique}.toml"));
        let original = "[tgbot]\nenabled = false\ncustom_tpl = \"\"\"\n{% if host.hdd_used / host.hdd_total > 0.8 %}\ndisk\n{% endif %}\n\"\"\"\n";
        fs::write(&path, original).unwrap();
        let report = migrate_file(path.to_str().unwrap(), None, false, false).unwrap();
        let backup = report.backup.unwrap();
        assert!(backup.exists());
        assert_eq!(fs::read_to_string(&backup).unwrap(), original);
        let migrated = fs::read_to_string(&path).unwrap();
        assert!(migrated.contains("[alerts]"));
        assert!(toml::from_str::<toml::Value>(&migrated).is_ok());
        fs::remove_file(path).unwrap();
        fs::remove_file(backup).unwrap();
    }

    #[test]
    fn migrate_dry_run_does_not_create_backup_or_modify_input() {
        let unique = format!(
            "server-status-config-dry-run-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        let path = std::env::temp_dir().join(format!("{unique}.toml"));
        let original = "[tgbot]\nenabled = false\n";
        fs::write(&path, original).unwrap();
        let report = migrate_file(path.to_str().unwrap(), None, true, false).unwrap();
        assert!(report.backup.is_none());
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        fs::remove_file(path).unwrap();
    }
}
