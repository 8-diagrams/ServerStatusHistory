#![deny(warnings)]

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;

use crate::config::{AlertConvergenceRule, AlertConvergenceWindow, AlertsConfig};
use crate::payload::HostStat;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertDecision {
    /// The alert may be delivered. The value is the number of suppressed
    /// occurrences since the previous delivery.
    Send { suppressed_count: u64 },
    /// The alert is currently within its convergence window.
    Suppress,
}

#[derive(Debug, Default)]
pub struct CustomEvaluation {
    pub should_send: bool,
    pub suppressed_rule_ids: Vec<String>,
}

#[derive(Debug, Clone)]
struct AlertState {
    sent_count: u32,
    last_sent_at: u64,
    suppressed_count: u64,
}

enum StateStore {
    Sqlite(Connection),
    Memory(HashMap<String, AlertState>),
}

pub struct AlertManager {
    config: AlertsConfig,
    store: StateStore,
}

impl AlertManager {
    pub fn new(config: &AlertsConfig) -> Self {
        let store = match open_store(&config.state_path) {
            Ok(connection) => StateStore::Sqlite(connection),
            Err(error) => {
                warn!(
                    "alert state store unavailable path={} error={error:#}; using in-memory state (convergence resets after restart)",
                    config.state_path
                );
                StateStore::Memory(HashMap::new())
            }
        };
        info!(
            "alert manager enabled={} disk_usage_enabled={} disk_threshold={} recover_threshold={} state_path={}",
            config.enabled,
            config.disk_usage.enabled,
            config.disk_usage.threshold,
            config.disk_usage.recover_threshold,
            config.state_path
        );
        Self {
            config: config.clone(),
            store,
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn disk_usage_percent(stat: &HostStat) -> Option<f64> {
        (stat.hdd_total > 0).then(|| stat.hdd_used as f64 * 100.0 / stat.hdd_total as f64)
    }

    #[cfg(test)]
    fn evaluate_disk(&mut self, stat: &HostStat, now: u64) -> AlertDecision {
        if !self.config.disk_usage.enabled {
            return AlertDecision::Send { suppressed_count: 0 };
        }
        let Some(usage) = Self::disk_usage_percent(stat) else {
            return AlertDecision::Send { suppressed_count: 0 };
        };
        self.evaluate_metric(
            "disk_usage_high",
            usage,
            self.config.disk_usage.threshold,
            self.config.disk_usage.recover_threshold,
            stat,
            now,
        )
    }

    /// Evaluate all built-in percentage resources independently. This keeps a
    /// suppressed disk block from suppressing an independently firing memory
    /// block in the same custom template.
    pub fn evaluate_custom(&mut self, stat: &HostStat, now: u64) -> CustomEvaluation {
        if !self.enabled() {
            return CustomEvaluation { should_send: true, suppressed_rule_ids: Vec::new() };
        }
        let candidates = [
            (
                "disk_usage_high",
                Self::disk_usage_percent(stat),
                self.config.disk_usage.enabled,
                self.config.disk_usage.threshold,
                self.config.disk_usage.recover_threshold,
            ),
            (
                "memory_usage_high",
                (stat.memory_total > 0).then(|| stat.memory_used as f64 * 100.0 / stat.memory_total as f64),
                self.config.memory_usage.enabled,
                self.config.memory_usage.threshold,
                self.config.memory_usage.recover_threshold,
            ),
        ];
        let mut known_active = false;
        let mut should_send = false;
        let mut suppressed_rule_ids = Vec::new();
        for (alert_type, value, enabled, threshold, recover_threshold) in candidates {
            if !enabled || self.rule_for(alert_type).is_none() {
                continue;
            }
            let Some(value) = value else {
                continue;
            };
            if value >= threshold {
                known_active = true;
                match self.evaluate_metric(alert_type, value, threshold, recover_threshold, stat, now) {
                    AlertDecision::Send { .. } => should_send = true,
                    AlertDecision::Suppress => suppressed_rule_ids.push(alert_type.to_string()),
                }
            } else {
                // Clear a previous state after recovery even when the value is
                // below the trigger threshold on this check.
                let _ = self.evaluate_metric(alert_type, value, threshold, recover_threshold, stat, now);
            }
        }
        if !known_active {
            // No structured resource is currently active. Preserve the old
            // behavior so a legacy custom template can still emit content.
            should_send = true;
        }
        CustomEvaluation { should_send, suppressed_rule_ids }
    }

    /// Apply a configured convergence rule to a percentage-based alert. This
    /// is intentionally generic so future detectors (memory, swap, CPU, ...)
    /// can reuse the same rule without changing the persistence model.
    pub fn evaluate_metric(
        &mut self,
        alert_type: &str,
        value_percent: f64,
        threshold: f64,
        recover_threshold: f64,
        stat: &HostStat,
        now: u64,
    ) -> AlertDecision {
        if !self.enabled() {
            return AlertDecision::Send { suppressed_count: 0 };
        }
        let Some(rule) = self.rule_for(alert_type) else {
            return AlertDecision::Send { suppressed_count: 0 };
        };
        let rule_id = rule.id.clone();
        let windows = rule.windows.clone();
        let fingerprint = format!("{}:{}:{}:{}", rule_id, alert_type, stat.name, stat.gid);
        if value_percent <= recover_threshold {
            match self.remove_state(&fingerprint) {
                Ok(true) => info!(
                    "alert resolved rule={} alert_type={} host={} gid={} value_percent={value_percent:.2} recover_threshold={recover_threshold}",
                    rule_id, alert_type, stat.name, stat.gid
                ),
                Ok(false) => {}
                Err(error) => error!(
                    "alert state delete failed rule={} alert_type={} host={} error={error:#}",
                    rule_id, alert_type, stat.name
                ),
            }
            return AlertDecision::Send { suppressed_count: 0 };
        }
        if value_percent < threshold {
            return AlertDecision::Send { suppressed_count: 0 };
        }

        let state = match self.load_state(&fingerprint) {
            Ok(state) => state,
            Err(error) => {
                error!(
                    "alert state read failed rule={} alert_type={} host={} error={error:#}",
                    rule_id, alert_type, stat.name
                );
                None
            }
        };
        let Some(mut state) = state else {
            let new_state = AlertState {
                sent_count: 1,
                last_sent_at: now,
                suppressed_count: 0,
            };
            if let Err(error) = self.save_state(&fingerprint, &new_state) {
                error!(
                    "alert state write failed rule={} alert_type={} host={} error={error:#}",
                    rule_id, alert_type, stat.name
                );
            }
            info!(
                "alert sent rule={} alert_type={} host={} gid={} value_percent={value_percent:.2} notification_count=1 suppressed_count=0",
                rule_id, alert_type, stat.name, stat.gid
            );
            return AlertDecision::Send { suppressed_count: 0 };
        };

        let interval = convergence_interval(&windows, state.sent_count);
        if now.saturating_sub(state.last_sent_at) >= interval {
            let suppressed_count = state.suppressed_count;
            state.sent_count = state.sent_count.saturating_add(1);
            state.last_sent_at = now;
            state.suppressed_count = 0;
            if let Err(error) = self.save_state(&fingerprint, &state) {
                error!(
                    "alert state write failed rule={} alert_type={} host={} error={error:#}",
                    rule_id, alert_type, stat.name
                );
            }
            info!(
                "alert sent rule={} alert_type={} host={} gid={} value_percent={value_percent:.2} notification_count={} suppressed_count={} interval={}s",
                rule_id, alert_type, stat.name, stat.gid, state.sent_count, suppressed_count, interval
            );
            AlertDecision::Send { suppressed_count }
        } else {
            let was_suppressed = state.suppressed_count > 0;
            state.suppressed_count = state.suppressed_count.saturating_add(1);
            if let Err(error) = self.save_state(&fingerprint, &state) {
                error!(
                    "alert state write failed rule={} alert_type={} host={} error={error:#}",
                    rule_id, alert_type, stat.name
                );
            }
            if self.config.log_suppressed && !was_suppressed {
                info!(
                    "alert suppressed rule={} alert_type={} host={} gid={} value_percent={value_percent:.2} notification_count={} interval={}s",
                    rule_id, alert_type, stat.name, stat.gid, state.sent_count, interval
                );
            }
            AlertDecision::Suppress
        }
    }

    fn rule_for(&self, alert_type: &str) -> Option<&AlertConvergenceRule> {
        let alert_type = alert_type.to_ascii_lowercase();
        self.config
            .convergence_rules
            .iter()
            .find(|rule| rule.alert_types.iter().any(|value| value == &alert_type))
            .or_else(|| {
                self.config
                    .convergence_rules
                    .iter()
                    .find(|rule| rule.alert_types.iter().any(|value| value == "*"))
            })
    }

    fn load_state(&self, fingerprint: &str) -> Result<Option<AlertState>> {
        match &self.store {
            StateStore::Sqlite(connection) => connection
                .query_row(
                    "SELECT sent_count, last_sent_at, suppressed_count FROM alert_state WHERE fingerprint = ?1",
                    params![fingerprint],
                    |row| {
                        Ok(AlertState {
                            sent_count: row.get(0)?,
                            last_sent_at: row.get(1)?,
                            suppressed_count: row.get(2)?,
                        })
                    },
                )
                .optional()
                .context("load alert state"),
            StateStore::Memory(states) => Ok(states.get(fingerprint).cloned()),
        }
    }

    fn save_state(&mut self, fingerprint: &str, state: &AlertState) -> Result<()> {
        match &mut self.store {
            StateStore::Sqlite(connection) => {
                connection.execute(
                    "INSERT INTO alert_state (fingerprint, sent_count, last_sent_at, suppressed_count) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(fingerprint) DO UPDATE SET sent_count=excluded.sent_count, last_sent_at=excluded.last_sent_at, suppressed_count=excluded.suppressed_count",
                    params![fingerprint, state.sent_count, state.last_sent_at, state.suppressed_count],
                )?;
            }
            StateStore::Memory(states) => {
                states.insert(fingerprint.to_string(), state.clone());
            }
        }
        Ok(())
    }

    fn remove_state(&mut self, fingerprint: &str) -> Result<bool> {
        match &mut self.store {
            StateStore::Sqlite(connection) => {
                Ok(connection.execute("DELETE FROM alert_state WHERE fingerprint = ?1", params![fingerprint])? > 0)
            }
            StateStore::Memory(states) => Ok(states.remove(fingerprint).is_some()),
        }
    }
}

fn open_store(path: &str) -> Result<Connection> {
    if path != ":memory:" {
        if let Some(parent) = std::path::Path::new(path)
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create alert state directory {}", parent.display()))?;
        }
    }
    let connection = Connection::open(path).with_context(|| format!("open alert state database {path}"))?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS alert_state (
            fingerprint TEXT PRIMARY KEY NOT NULL,
            sent_count INTEGER NOT NULL,
            last_sent_at INTEGER NOT NULL,
            suppressed_count INTEGER NOT NULL DEFAULT 0
        )",
    )?;
    Ok(connection)
}

fn convergence_interval(windows: &[AlertConvergenceWindow], sent_count: u32) -> u64 {
    let mut remaining = sent_count;
    for window in windows {
        if window.max_notifications.is_none_or(|max| remaining < max) {
            return parse_duration_seconds(&window.interval).unwrap_or(300);
        }
        remaining = remaining.saturating_sub(window.max_notifications.unwrap_or(0));
    }
    windows
        .last()
        .and_then(|window| parse_duration_seconds(&window.interval).ok())
        .unwrap_or(300)
}

fn parse_duration_seconds(value: &str) -> Result<u64> {
    let value = value.trim();
    let (number, unit) = value.split_at(value.find(|ch: char| !ch.is_ascii_digit()).unwrap_or(value.len()));
    let amount: u64 = number.parse().context("duration amount")?;
    let multiplier = match unit.trim().to_ascii_lowercase().as_str() {
        "s" => 1,
        "m" => 60,
        "h" => 60 * 60,
        "d" => 24 * 60 * 60,
        _ => anyhow::bail!("unsupported duration unit in `{value}`"),
    };
    Ok(amount.saturating_mul(multiplier))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AlertsConfig;

    fn stat(used: u64, total: u64) -> HostStat {
        HostStat {
            name: "node-1".to_string(),
            hdd_used: used,
            hdd_total: total,
            ..HostStat::default()
        }
    }

    fn manager() -> AlertManager {
        let mut config = AlertsConfig::default();
        config.state_path = ":memory:".to_string();
        AlertManager::new(&config)
    }

    #[test]
    fn disk_alert_uses_five_ten_and_sixty_minute_windows() {
        let mut manager = manager();
        let high = stat(90, 100);
        assert_eq!(
            manager.evaluate_disk(&high, 0),
            AlertDecision::Send { suppressed_count: 0 }
        );
        assert_eq!(manager.evaluate_disk(&high, 60), AlertDecision::Suppress);
        assert_eq!(
            manager.evaluate_disk(&high, 300),
            AlertDecision::Send { suppressed_count: 1 }
        );
        assert_eq!(
            manager.evaluate_disk(&high, 600),
            AlertDecision::Send { suppressed_count: 0 }
        );
        assert_eq!(manager.evaluate_disk(&high, 1_000), AlertDecision::Suppress);
        assert_eq!(
            manager.evaluate_disk(&high, 1_200),
            AlertDecision::Send { suppressed_count: 1 }
        );
        assert_eq!(manager.evaluate_disk(&high, 1_500), AlertDecision::Suppress);
        assert_eq!(
            manager.evaluate_disk(&high, 1_800),
            AlertDecision::Send { suppressed_count: 1 }
        );
        assert_eq!(
            manager.evaluate_disk(&high, 2_400),
            AlertDecision::Send { suppressed_count: 0 }
        );
        assert_eq!(manager.evaluate_disk(&high, 3_000), AlertDecision::Suppress);
        assert_eq!(
            manager.evaluate_disk(&high, 6_000),
            AlertDecision::Send { suppressed_count: 1 }
        );
    }

    #[test]
    fn recovery_clears_state_and_allows_immediate_alert_again() {
        let mut manager = manager();
        let high = stat(90, 100);
        let recovered = stat(70, 100);
        assert_eq!(
            manager.evaluate_disk(&high, 0),
            AlertDecision::Send { suppressed_count: 0 }
        );
        assert_eq!(
            manager.evaluate_disk(&recovered, 1),
            AlertDecision::Send { suppressed_count: 0 }
        );
        assert_eq!(
            manager.evaluate_disk(&high, 2),
            AlertDecision::Send { suppressed_count: 0 }
        );
    }

    #[test]
    fn duration_parser_supports_common_units() {
        assert_eq!(parse_duration_seconds("5m").unwrap(), 300);
        assert_eq!(parse_duration_seconds("1h").unwrap(), 3600);
        assert!(parse_duration_seconds("five minutes").is_err());
    }

    #[test]
    fn one_rule_can_be_bound_to_multiple_alert_types() {
        let mut config = AlertsConfig::default();
        config.state_path = ":memory:".to_string();
        config.convergence_rules[0].alert_types =
            vec!["disk_usage_high".to_string(), "disk_usage_critical".to_string()];
        let mut manager = AlertManager::new(&config);
        let high = stat(90, 100);
        assert_eq!(
            manager.evaluate_metric("disk_usage_critical", 90.0, 80.0, 75.0, &high, 0),
            AlertDecision::Send { suppressed_count: 0 }
        );
        assert_eq!(
            manager.evaluate_metric("disk_usage_critical", 90.0, 80.0, 75.0, &high, 1),
            AlertDecision::Suppress
        );
    }

    #[test]
    fn sqlite_state_survives_manager_restart() {
        let path = std::env::temp_dir().join(format!(
            "server-status-alert-test-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut config = AlertsConfig::default();
        config.state_path = path.to_string_lossy().to_string();
        let high = stat(90, 100);
        {
            let mut manager = AlertManager::new(&config);
            assert_eq!(
                manager.evaluate_disk(&high, 0),
                AlertDecision::Send { suppressed_count: 0 }
            );
        }
        {
            let mut manager = AlertManager::new(&config);
            assert_eq!(manager.evaluate_disk(&high, 1), AlertDecision::Suppress);
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn custom_evaluation_keeps_memory_when_disk_is_converged() {
        let mut manager = manager();
        let disk_only = HostStat {
            name: "node-1".to_string(),
            hdd_used: 90,
            hdd_total: 100,
            memory_used: 10,
            memory_total: 100,
            ..HostStat::default()
        };
        let both_high = HostStat {
            memory_used: 90,
            ..disk_only.clone()
        };
        let first = manager.evaluate_custom(&disk_only, 0);
        assert!(first.should_send);
        assert!(first.suppressed_rule_ids.is_empty());

        let second = manager.evaluate_custom(&both_high, 1);
        assert!(second.should_send);
        assert_eq!(second.suppressed_rule_ids, vec!["disk_usage_high"]);
    }
}
