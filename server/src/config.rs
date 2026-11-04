#![deny(warnings)]
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::collections::HashSet;
use std::env;
use std::fs;
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
    let mut o = toml::from_str::<Config>(content).unwrap();
    o.hosts_map = HashMap::new();

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

    eprintln!("✨ admin_user: {}", o.admin_user.as_ref()?);
    eprintln!("✨ admin_pass: {}", o.admin_pass.as_ref()?);

    Some(o)
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
}
