//! Historical metric storage.
//!
//! The rest of the server only depends on `HistoryStore`.  This keeps the
//! ingestion and HTTP layers independent from a concrete database and makes
//! remote backends (VictoriaMetrics, InfluxDB, ...) possible later.

use anyhow::{anyhow, Context, Result};
use chrono::{Datelike, Months, TimeZone, Utc};
use clap::{Args, Subcommand};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::{HistoryConfig, HistoryTierConfig};
use crate::payload::HostStat;

const METRICS: &[&str] = &[
    "online4",
    "online6",
    "cpu",
    "load_1",
    "load_5",
    "load_15",
    "memory_used",
    "memory_total",
    "swap_used",
    "swap_total",
    "hdd_used",
    "hdd_total",
    "tcp",
    "udp",
    "process",
    "thread",
    "network_in",
    "network_out",
    "traffic_in",
    "traffic_out",
    "network_rx",
    "network_tx",
    "ping_10010",
    "ping_189",
    "ping_10086",
    "time_10010",
    "time_189",
    "time_10086",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryPoint {
    pub ts: u64,
    pub received_ts: u64,
    pub host_id: String,
    pub gid: String,
    pub online4: bool,
    pub online6: bool,
    pub cpu: f64,
    pub load_1: f64,
    pub load_5: f64,
    pub load_15: f64,
    pub memory_used: u64,
    pub memory_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub hdd_used: u64,
    pub hdd_total: u64,
    pub tcp: u32,
    pub udp: u32,
    pub process: u32,
    pub thread: u32,
    pub network_in: u64,
    pub network_out: u64,
    pub network_rx: u64,
    pub network_tx: u64,
    pub ping_10010: f64,
    pub ping_189: f64,
    pub ping_10086: f64,
    pub time_10010: f64,
    pub time_189: f64,
    pub time_10086: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RollupValue {
    sum: f64,
    count: u64,
    min: f64,
    max: f64,
    last: f64,
    last_ts: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RollupData {
    sample_count: u64,
    gap_count: u64,
    reset_count: u64,
    metrics: HashMap<String, RollupValue>,
}

impl RollupData {
    fn add_raw(&mut self, point: &HistoryPoint) {
        self.sample_count = self.sample_count.saturating_add(1);
        for (name, value) in raw_metric_values(point) {
            self.add_value(name, value, point.ts, 1);
        }
    }

    fn add_rollup(&mut self, data: &RollupData) {
        self.sample_count = self.sample_count.saturating_add(data.sample_count);
        self.gap_count = self.gap_count.saturating_add(data.gap_count);
        self.reset_count = self.reset_count.saturating_add(data.reset_count);
        for (name, value) in &data.metrics {
            let Some(existing) = self.metrics.get_mut(name) else {
                self.metrics.insert(name.clone(), value.clone());
                continue;
            };
            existing.sum += value.sum;
            existing.count = existing.count.saturating_add(value.count);
            existing.min = existing.min.min(value.min);
            existing.max = existing.max.max(value.max);
            if value.last_ts >= existing.last_ts {
                existing.last = value.last;
                existing.last_ts = value.last_ts;
            }
        }
    }

    fn add_value(&mut self, name: &str, value: f64, ts: u64, count: u64) {
        if !value.is_finite() {
            return;
        }
        let entry = self.metrics.entry(name.to_string()).or_insert(RollupValue {
            sum: 0.0,
            count: 0,
            min: value,
            max: value,
            last: value,
            last_ts: ts,
        });
        entry.sum += value * count as f64;
        entry.count = entry.count.saturating_add(count);
        entry.min = entry.min.min(value);
        entry.max = entry.max.max(value);
        if ts >= entry.last_ts {
            entry.last = value;
            entry.last_ts = ts;
        }
    }

    fn add_sum(&mut self, name: &str, value: f64, ts: u64) {
        if !value.is_finite() {
            return;
        }
        let entry = self.metrics.entry(name.to_string()).or_insert(RollupValue {
            sum: 0.0,
            count: 0,
            min: value,
            max: value,
            last: value,
            last_ts: ts,
        });
        entry.sum += value;
        entry.count = entry.count.saturating_add(1);
        entry.min = entry.min.min(value);
        entry.max = entry.max.max(value);
        if ts >= entry.last_ts {
            entry.last = value;
            entry.last_ts = ts;
        }
    }

    fn value(&self, name: &str) -> Option<f64> {
        let value = self.metrics.get(name)?;
        if matches!(name, "traffic_in" | "traffic_out") {
            Some(value.sum)
        } else if matches!(
            name,
            "network_in" | "network_out" | "memory_total" | "swap_total" | "hdd_total"
        ) {
            Some(value.last)
        } else if value.count == 0 {
            None
        } else {
            Some(value.sum / value.count as f64)
        }
    }
}

fn raw_metric_values(point: &HistoryPoint) -> Vec<(&'static str, f64)> {
    vec![
        ("online4", f64::from(point.online4)),
        ("online6", f64::from(point.online6)),
        ("cpu", point.cpu),
        ("load_1", point.load_1),
        ("load_5", point.load_5),
        ("load_15", point.load_15),
        ("memory_used", point.memory_used as f64),
        ("memory_total", point.memory_total as f64),
        ("swap_used", point.swap_used as f64),
        ("swap_total", point.swap_total as f64),
        ("hdd_used", point.hdd_used as f64),
        ("hdd_total", point.hdd_total as f64),
        ("tcp", point.tcp as f64),
        ("udp", point.udp as f64),
        ("process", point.process as f64),
        ("thread", point.thread as f64),
        ("network_in", point.network_in as f64),
        ("network_out", point.network_out as f64),
        ("network_rx", point.network_rx as f64),
        ("network_tx", point.network_tx as f64),
        ("ping_10010", point.ping_10010),
        ("ping_189", point.ping_189),
        ("ping_10086", point.ping_10086),
        ("time_10010", point.time_10010),
        ("time_189", point.time_189),
        ("time_10086", point.time_10086),
    ]
}

impl HistoryPoint {
    #[must_use]
    pub fn from_stat(stat: &HostStat) -> Self {
        Self {
            ts: stat.latest_ts,
            received_ts: now_secs(),
            host_id: stat.name.clone(),
            gid: stat.gid.clone(),
            online4: stat.online4,
            online6: stat.online6,
            cpu: stat.cpu,
            load_1: stat.load_1,
            load_5: stat.load_5,
            load_15: stat.load_15,
            memory_used: stat.memory_used,
            memory_total: stat.memory_total,
            swap_used: stat.swap_used,
            swap_total: stat.swap_total,
            hdd_used: stat.hdd_used,
            hdd_total: stat.hdd_total,
            tcp: stat.tcp_count,
            udp: stat.udp_count,
            process: stat.process_count,
            thread: stat.thread_count,
            network_in: stat.network_in,
            network_out: stat.network_out,
            network_rx: stat.network_rx,
            network_tx: stat.network_tx,
            ping_10010: stat.ping_10010,
            ping_189: stat.ping_189,
            ping_10086: stat.ping_10086,
            time_10010: stat.time_10010,
            time_189: stat.time_189,
            time_10086: stat.time_10086,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct HistoryQuery {
    pub host: String,
    pub gid: Option<String>,
    pub from: Option<u64>,
    pub to: Option<u64>,
    pub step: Option<u64>,
    pub metrics: Option<String>,
    pub resolution: Option<String>,
    pub max_points: Option<usize>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum AggregateKind {
    Average,
    Maximum,
    Minimum,
    Last,
    CounterDelta,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct MetricDefinition {
    pub name: &'static str,
    pub aggregate: AggregateKind,
}

#[allow(dead_code)]
pub trait HistoryStore: Send + Sync {
    fn write(&self, point: &HistoryPoint) -> Result<()>;
    fn query(&self, query: &HistoryQuery) -> Result<Value>;
    fn delete_before(&self, before: u64, batch_size: usize) -> Result<u64>;
    fn maintain(&self, now: u64, exhaustive: bool) -> Result<MaintenanceReport>;
    fn status(&self) -> Result<Value>;
    fn verify(&self) -> Result<Value>;
    fn prune(&self, before: u64, host: Option<&str>, tier: Option<&str>, apply: bool) -> Result<Value>;
    fn vacuum(&self) -> Result<()>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MaintenanceReport {
    pub compacted: u64,
    pub deleted: u64,
    pub dirty_remaining: u64,
}

#[derive(Debug, Args)]
pub struct HistoryCli {
    #[command(subcommand)]
    pub action: HistoryAction,
}

#[derive(Debug, Subcommand)]
pub enum HistoryAction {
    /// Show row counts, ranges, and pending rollups.
    Status,
    /// Build or repair all pending lower-resolution buckets.
    Compact {
        #[arg(long)]
        apply: bool,
    },
    /// Apply configured retention limits after completing rollups.
    EnforceRetention {
        #[arg(long)]
        apply: bool,
    },
    /// Delete history older than a timestamp or YYYY-MM-DD date.
    Prune {
        /// Unix timestamp or UTC date (YYYY-MM-DD); matching buckets are older than this value.
        #[arg(long)]
        before: String,
        /// Limit deletion to one host id.
        #[arg(long)]
        host: Option<String>,
        /// Limit deletion to raw/1s or one rollup tier such as 1m, 5m, 1h, 1d, or 1mo.
        #[arg(long)]
        tier: Option<String>,
        /// Perform deletion; without this flag the command is a dry run.
        #[arg(long)]
        apply: bool,
    },
    /// Verify SQLite integrity, rollup queues, configured tiers, and expired rows.
    Verify,
    /// Checkpoint WAL and reclaim SQLite file space. Stop the server first.
    Vacuum {
        #[arg(long)]
        apply: bool,
    },
}

pub fn run_history_cli(cfg: &HistoryConfig, cli: &HistoryCli) -> Result<()> {
    let Some(store) = create_store(cfg)? else {
        return Err(anyhow!("history storage is disabled"));
    };
    let output = match &cli.action {
        HistoryAction::Status => store.status()?,
        HistoryAction::Compact { apply } => {
            if *apply {
                let mut total = MaintenanceReport::default();
                loop {
                    let report = store.maintain(now_secs(), true)?;
                    total.compacted = total.compacted.saturating_add(report.compacted);
                    total.deleted = total.deleted.saturating_add(report.deleted);
                    total.dirty_remaining = report.dirty_remaining;
                    if report.dirty_remaining == 0 || report.compacted == 0 {
                        break;
                    }
                }
                json!({"applied": true, "compacted": total.compacted, "deleted": total.deleted, "dirty": total.dirty_remaining})
            } else {
                json!({"applied": false, "message": "dry run; pass --apply to compact", "status": store.status()?})
            }
        }
        HistoryAction::EnforceRetention { apply } => {
            if *apply {
                let mut total = MaintenanceReport::default();
                loop {
                    let report = store.maintain(now_secs(), true)?;
                    total.compacted = total.compacted.saturating_add(report.compacted);
                    total.deleted = total.deleted.saturating_add(report.deleted);
                    total.dirty_remaining = report.dirty_remaining;
                    if report.dirty_remaining == 0 && report.deleted == 0 {
                        break;
                    }
                    if report.compacted == 0 && report.deleted == 0 {
                        break;
                    }
                }
                json!({"applied": true, "compacted": total.compacted, "deleted": total.deleted, "dirty": total.dirty_remaining})
            } else {
                json!({"applied": false, "message": "dry run; pass --apply to enforce retention", "status": store.status()?})
            }
        }
        HistoryAction::Prune {
            before,
            host,
            tier,
            apply,
        } => store.prune(parse_cli_timestamp(before)?, host.as_deref(), tier.as_deref(), *apply)?,
        HistoryAction::Verify => store.verify()?,
        HistoryAction::Vacuum { apply } => {
            if *apply {
                store.vacuum()?;
            }
            json!({"applied": apply, "message": if *apply { "vacuum completed" } else { "dry run; pass --apply to vacuum" }})
        }
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    if matches!(&cli.action, HistoryAction::Verify) && output.get("ok") != Some(&Value::Bool(true)) {
        return Err(anyhow!("history verification failed"));
    }
    Ok(())
}

fn parse_cli_timestamp(input: &str) -> Result<u64> {
    if let Ok(value) = input.parse::<u64>() {
        return Ok(value);
    }
    let date = chrono::NaiveDate::parse_from_str(input, "%Y-%m-%d")
        .with_context(|| format!("invalid timestamp/date `{input}`"))?;
    date.and_hms_opt(0, 0, 0)
        .and_then(|value| u64::try_from(value.and_utc().timestamp()).ok())
        .ok_or_else(|| anyhow!("invalid timestamp/date `{input}`"))
}

/// Storage plugins use the same canonical point/query model.  Remote backends
/// may use this catalog to map gauges and monotonically increasing counters to
/// their native aggregation/query language.
#[allow(dead_code)]
pub const METRIC_DEFINITIONS: &[MetricDefinition] = &[
    MetricDefinition {
        name: "cpu",
        aggregate: AggregateKind::Average,
    },
    MetricDefinition {
        name: "load_1",
        aggregate: AggregateKind::Average,
    },
    MetricDefinition {
        name: "load_5",
        aggregate: AggregateKind::Average,
    },
    MetricDefinition {
        name: "load_15",
        aggregate: AggregateKind::Average,
    },
    MetricDefinition {
        name: "memory_used",
        aggregate: AggregateKind::Average,
    },
    MetricDefinition {
        name: "memory_total",
        aggregate: AggregateKind::Last,
    },
    MetricDefinition {
        name: "swap_used",
        aggregate: AggregateKind::Average,
    },
    MetricDefinition {
        name: "swap_total",
        aggregate: AggregateKind::Last,
    },
    MetricDefinition {
        name: "hdd_used",
        aggregate: AggregateKind::Average,
    },
    MetricDefinition {
        name: "hdd_total",
        aggregate: AggregateKind::Last,
    },
    MetricDefinition {
        name: "network_in",
        aggregate: AggregateKind::Last,
    },
    MetricDefinition {
        name: "network_out",
        aggregate: AggregateKind::Last,
    },
    MetricDefinition {
        name: "traffic_in",
        aggregate: AggregateKind::CounterDelta,
    },
    MetricDefinition {
        name: "traffic_out",
        aggregate: AggregateKind::CounterDelta,
    },
    MetricDefinition {
        name: "network_rx",
        aggregate: AggregateKind::Average,
    },
    MetricDefinition {
        name: "network_tx",
        aggregate: AggregateKind::Average,
    },
    MetricDefinition {
        name: "ping_10010",
        aggregate: AggregateKind::Average,
    },
    MetricDefinition {
        name: "ping_189",
        aggregate: AggregateKind::Average,
    },
    MetricDefinition {
        name: "ping_10086",
        aggregate: AggregateKind::Average,
    },
    MetricDefinition {
        name: "time_10010",
        aggregate: AggregateKind::Average,
    },
    MetricDefinition {
        name: "time_189",
        aggregate: AggregateKind::Average,
    },
    MetricDefinition {
        name: "time_10086",
        aggregate: AggregateKind::Average,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BucketResolution {
    Fixed(u64),
    CalendarMonth,
}

impl BucketResolution {
    fn parse(value: &str) -> Result<Self> {
        if value.eq_ignore_ascii_case("1mo") {
            return Ok(Self::CalendarMonth);
        }
        let seconds = parse_fixed_duration(value)?;
        if seconds == 0 {
            return Err(anyhow!("history resolution cannot be zero"));
        }
        Ok(Self::Fixed(seconds))
    }

    fn bucket_start(self, ts: u64) -> u64 {
        match self {
            Self::Fixed(seconds) => ts / seconds * seconds,
            Self::CalendarMonth => Utc
                .timestamp_opt(i64::try_from(ts).unwrap_or(i64::MAX), 0)
                .single()
                .and_then(|value| Utc.with_ymd_and_hms(value.year(), value.month(), 1, 0, 0, 0).single())
                .and_then(|value| u64::try_from(value.timestamp()).ok())
                .unwrap_or(ts),
        }
    }

    fn bucket_end(self, start: u64) -> u64 {
        match self {
            Self::Fixed(seconds) => start.saturating_add(seconds),
            Self::CalendarMonth => Utc
                .timestamp_opt(i64::try_from(start).unwrap_or(i64::MAX), 0)
                .single()
                .and_then(|value| value.checked_add_months(Months::new(1)))
                .and_then(|value| u64::try_from(value.timestamp()).ok())
                .unwrap_or(start.saturating_add(31 * 86_400)),
        }
    }

    fn approximate_seconds(self) -> u64 {
        match self {
            Self::Fixed(seconds) => seconds,
            Self::CalendarMonth => 30 * 86_400,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Retention {
    Fixed(u64),
    CalendarMonths(u32),
    Forever,
}

impl Retention {
    fn parse(value: &str) -> Result<Self> {
        if value.eq_ignore_ascii_case("forever") || value == "0" {
            return Ok(Self::Forever);
        }
        if let Some(years) = value.strip_suffix('y') {
            let years = years.parse::<u32>().context("invalid history retention years")?;
            if years == 0 {
                return Err(anyhow!("history retention cannot be zero"));
            }
            return Ok(Self::CalendarMonths(years.saturating_mul(12)));
        }
        if let Some(months) = value.strip_suffix("mo") {
            let months = months.parse::<u32>().context("invalid history retention months")?;
            if months == 0 {
                return Err(anyhow!("history retention cannot be zero"));
            }
            return Ok(Self::CalendarMonths(months));
        }
        let seconds = parse_fixed_duration(value)?;
        if seconds == 0 {
            return Err(anyhow!("history retention cannot be zero"));
        }
        Ok(Self::Fixed(seconds))
    }

    fn cutoff(self, now: u64) -> u64 {
        match self {
            Self::Fixed(seconds) => now.saturating_sub(seconds),
            Self::CalendarMonths(months) => Utc
                .timestamp_opt(i64::try_from(now).unwrap_or(i64::MAX), 0)
                .single()
                .and_then(|value| value.checked_sub_months(Months::new(months)))
                .and_then(|value| u64::try_from(value.timestamp()).ok())
                .unwrap_or(0),
            Self::Forever => 0,
        }
    }

    fn approximate_seconds(self) -> u64 {
        match self {
            Self::Fixed(seconds) => seconds,
            // 365.25 days/year expressed without floating-point arithmetic.
            Self::CalendarMonths(months) => u64::from(months) * 36_525 * 86_400 / 1_200,
            Self::Forever => u64::MAX,
        }
    }
}

#[derive(Debug, Clone)]
struct TierSpec {
    name: String,
    retention_label: String,
    resolution: BucketResolution,
    retention: Retention,
}

impl TierSpec {
    fn from_config(value: &HistoryTierConfig) -> Result<Self> {
        Ok(Self {
            name: value.resolution.to_ascii_lowercase(),
            retention_label: value.retention.to_ascii_lowercase(),
            resolution: BucketResolution::parse(&value.resolution)?,
            retention: Retention::parse(&value.retention)?,
        })
    }
}

fn parse_fixed_duration(value: &str) -> Result<u64> {
    let (number, multiplier) = if let Some(number) = value.strip_suffix('s') {
        (number, 1)
    } else if let Some(number) = value.strip_suffix('m') {
        (number, 60)
    } else if let Some(number) = value.strip_suffix('h') {
        (number, 3_600)
    } else if let Some(number) = value.strip_suffix('d') {
        (number, 86_400)
    } else {
        return Err(anyhow!("unsupported history duration `{value}`"));
    };
    number
        .parse::<u64>()
        .with_context(|| format!("invalid history duration `{value}`"))?
        .checked_mul(multiplier)
        .ok_or_else(|| anyhow!("history duration `{value}` is too large"))
}

fn parse_tiers(cfg: &HistoryConfig) -> Result<Vec<TierSpec>> {
    let configured = if cfg.tiers.is_empty() {
        vec![HistoryTierConfig {
            resolution: format!("{}s", cfg.sample_interval.max(1)),
            retention: if cfg.retention_days == 0 {
                "forever".to_string()
            } else {
                format!("{}d", cfg.retention_days)
            },
        }]
    } else {
        cfg.tiers.clone()
    };
    let tiers = configured
        .iter()
        .map(TierSpec::from_config)
        .collect::<Result<Vec<_>>>()?;
    for pair in tiers.windows(2) {
        if pair[0].resolution.approximate_seconds() >= pair[1].resolution.approximate_seconds() {
            return Err(anyhow!("history tier resolutions must be strictly increasing"));
        }
        if pair[0].retention.approximate_seconds() >= pair[1].retention.approximate_seconds() {
            return Err(anyhow!("history tier retentions must be strictly increasing"));
        }
    }
    Ok(tiers)
}

pub type SharedHistoryStore = Arc<dyn HistoryStore>;

pub struct HistoryService {
    store: SharedHistoryStore,
    sender: std::sync::mpsc::SyncSender<HistoryPoint>,
    allowed_lateness: u64,
}

impl HistoryService {
    pub fn new(cfg: &HistoryConfig) -> Result<Option<Arc<Self>>> {
        let Some(store) = create_store(cfg)? else {
            return Ok(None);
        };

        let compression = cfg.compression.to_ascii_lowercase();
        if compression != "none" && compression != "latest" {
            return Err(anyhow!(
                "unsupported history compression `{}`; supported values: none, latest",
                cfg.compression
            ));
        }
        let raw_resolution = parse_tiers(cfg)?
            .first()
            .map(|tier| tier.resolution)
            .ok_or_else(|| anyhow!("at least one history tier is required"))?;
        let (sender, receiver) = std::sync::mpsc::sync_channel::<HistoryPoint>(2048);
        let maintenance_interval = if cfg.tiers.is_empty() {
            cfg.cleanup_interval.max(60)
        } else {
            cfg.maintenance_interval.max(5)
        };
        let vacuum_after_cleanup = cfg.vacuum_after_cleanup;
        let writer_store = Arc::clone(&store);
        let writer_compression = compression.clone();
        std::thread::Builder::new()
            .name("history-writer".to_string())
            .spawn(move || {
                while let Ok(mut point) = receiver.recv() {
                    point.ts = compression_bucket_timestamp(&writer_compression, raw_resolution, point.ts);
                    if let Err(err) = writer_store.write(&point) {
                        error!("history write failed: {err:?}");
                    }
                }
            })
            .context("start history writer thread")?;
        let maintenance_store = Arc::downgrade(&store);
        std::thread::Builder::new()
            .name("history-maintenance".to_string())
            .spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(maintenance_interval));
                let Some(store) = maintenance_store.upgrade() else {
                    break;
                };
                match store.maintain(now_secs(), false) {
                    Ok(report) => trace!(
                        "history maintenance compacted={}, deleted={}, dirty={}",
                        report.compacted,
                        report.deleted,
                        report.dirty_remaining
                    ),
                    Err(err) => error!("history maintenance failed: {err:?}"),
                }
                if vacuum_after_cleanup {
                    if let Err(err) = store.vacuum() {
                        error!("history vacuum failed: {err:?}");
                    }
                }
            })
            .context("start history maintenance thread")?;

        Ok(Some(Arc::new(Self {
            store,
            sender,
            allowed_lateness: cfg.allowed_lateness,
        })))
    }

    pub fn record(&self, stat: &HostStat) {
        let received_ts = now_secs();
        let mut point = HistoryPoint::from_stat(stat);
        point.ts = select_event_timestamp(stat.agent_ts, received_ts, self.allowed_lateness);
        point.received_ts = received_ts;
        if self.sender.try_send(point).is_err() {
            warn!("history queue is full; dropping a historical sample");
        }
    }

    pub fn query(&self, query: &HistoryQuery) -> Result<Value> {
        self.store.query(query)
    }
}

fn select_event_timestamp(agent_ts: Option<u64>, received_ts: u64, allowed_lateness: u64) -> u64 {
    agent_ts
        .filter(|event_ts| {
            *event_ts <= received_ts.saturating_add(30) && received_ts.saturating_sub(*event_ts) <= allowed_lateness
        })
        .unwrap_or(received_ts)
}

fn compression_bucket_timestamp(compression: &str, resolution: BucketResolution, timestamp: u64) -> u64 {
    if compression == "latest" {
        resolution.bucket_start(timestamp)
    } else {
        timestamp
    }
}

pub fn create_store(cfg: &HistoryConfig) -> Result<Option<SharedHistoryStore>> {
    if !cfg.enabled {
        return Ok(None);
    }
    if !cfg.backend.eq_ignore_ascii_case("sqlite") {
        return Err(anyhow!(
            "unsupported history backend `{}`; supported backends: sqlite",
            cfg.backend
        ));
    }
    Ok(Some(Arc::new(SqliteHistoryStore::open(cfg)?)))
}

struct SqliteHistoryStore {
    conn: Mutex<Connection>,
    path: String,
    tiers: Vec<TierSpec>,
    delete_batch_size: usize,
    max_query_points: usize,
}

impl SqliteHistoryStore {
    fn open(cfg: &HistoryConfig) -> Result<Self> {
        let path = &cfg.path;
        let tiers = parse_tiers(cfg)?;
        if let Some(parent) = Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).with_context(|| format!("create history directory {parent:?}"))?;
            }
        }
        let conn = Connection::open(path).with_context(|| format!("open SQLite history database `{path}`"))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS schema_meta (
                 key TEXT PRIMARY KEY NOT NULL,
                 value TEXT NOT NULL
             );
             INSERT OR IGNORE INTO schema_meta(key, value) VALUES ('schema_version', '2');
             CREATE TABLE IF NOT EXISTS metric_samples (
                 host_id TEXT NOT NULL,
                 ts INTEGER NOT NULL,
                 received_ts INTEGER NOT NULL DEFAULT 0,
                 gid TEXT NOT NULL DEFAULT '',
                 online4 INTEGER NOT NULL,
                 online6 INTEGER NOT NULL,
                 cpu REAL NOT NULL,
                 load_1 REAL NOT NULL,
                 load_5 REAL NOT NULL,
                 load_15 REAL NOT NULL,
                 memory_used INTEGER NOT NULL,
                 memory_total INTEGER NOT NULL,
                 swap_used INTEGER NOT NULL,
                 swap_total INTEGER NOT NULL,
                 hdd_used INTEGER NOT NULL,
                 hdd_total INTEGER NOT NULL,
                 tcp INTEGER NOT NULL,
                 udp INTEGER NOT NULL,
                 process INTEGER NOT NULL,
                 thread INTEGER NOT NULL,
                 network_in INTEGER NOT NULL,
                 network_out INTEGER NOT NULL,
                 network_rx INTEGER NOT NULL,
                 network_tx INTEGER NOT NULL,
                 ping_10010 REAL NOT NULL,
                 ping_189 REAL NOT NULL,
                 ping_10086 REAL NOT NULL,
                 time_10010 REAL NOT NULL,
                 time_189 REAL NOT NULL,
                 time_10086 REAL NOT NULL,
                 PRIMARY KEY(host_id, gid, ts)
             );
             CREATE INDEX IF NOT EXISTS idx_metric_samples_host_ts
                 ON metric_samples(host_id, ts);
             CREATE INDEX IF NOT EXISTS idx_metric_samples_ts
                 ON metric_samples(ts);
             CREATE TABLE IF NOT EXISTS metric_rollups (
                 host_id TEXT NOT NULL,
                 gid TEXT NOT NULL DEFAULT '',
                 tier TEXT NOT NULL,
                 bucket_ts INTEGER NOT NULL,
                 bucket_end INTEGER NOT NULL,
                 sample_count INTEGER NOT NULL,
                 data TEXT NOT NULL,
                 PRIMARY KEY(host_id, gid, tier, bucket_ts)
             );
             CREATE INDEX IF NOT EXISTS idx_metric_rollups_tier_ts
                 ON metric_rollups(tier, bucket_ts);
             CREATE TABLE IF NOT EXISTS history_dirty (
                 host_id TEXT NOT NULL,
                 gid TEXT NOT NULL DEFAULT '',
                 tier TEXT NOT NULL,
                 bucket_ts INTEGER NOT NULL,
                 PRIMARY KEY(host_id, gid, tier, bucket_ts)
             );
             CREATE TABLE IF NOT EXISTS history_watermarks (
                 tier TEXT PRIMARY KEY NOT NULL,
                 complete_until INTEGER NOT NULL DEFAULT 0,
                 updated_at INTEGER NOT NULL DEFAULT 0
             );",
        )?;
        let has_received_ts = {
            let mut stmt = conn.prepare("PRAGMA table_info(metric_samples)")?;
            let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
            columns
                .collect::<std::result::Result<Vec<_>, _>>()?
                .iter()
                .any(|name| name == "received_ts")
        };
        if !has_received_ts {
            conn.execute(
                "ALTER TABLE metric_samples ADD COLUMN received_ts INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        conn.execute(
            "INSERT INTO schema_meta(key, value) VALUES ('schema_version', '2')
             ON CONFLICT(key) DO UPDATE SET value='2'",
            [],
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
            path: path.clone(),
            tiers,
            delete_batch_size: cfg.delete_batch_size.max(1),
            max_query_points: cfg.max_query_points.clamp(100, 100_000),
        })
    }

    fn tier_index(&self, name: &str) -> Option<usize> {
        self.tiers.iter().position(|tier| tier.name.eq_ignore_ascii_case(name))
    }

    fn auto_tier_index(&self, now: u64, from: u64) -> usize {
        self.tiers
            .iter()
            .position(|tier| from >= tier.retention.cutoff(now))
            .unwrap_or_else(|| self.tiers.len().saturating_sub(1))
    }

    fn load_raw_points(
        conn: &Connection,
        host: &str,
        gid: Option<&str>,
        from: u64,
        to: u64,
    ) -> Result<Vec<HistoryPoint>> {
        let mut stmt = conn.prepare(
            "SELECT ts, online4, online6, cpu, load_1, load_5, load_15,
                    memory_used, memory_total, swap_used, swap_total, hdd_used, hdd_total,
                    tcp, udp, process, thread, network_in, network_out, network_rx, network_tx,
                    ping_10010, ping_189, ping_10086, time_10010, time_189, time_10086, gid,
                    received_ts
             FROM metric_samples
             WHERE host_id = ?1 AND (?2 IS NULL OR gid = ?2) AND ts >= ?3 AND ts < ?4
             ORDER BY ts ASC",
        )?;
        let rows = stmt.query_map(params![host, gid, from, to], |row| {
            Ok(HistoryPoint {
                ts: row.get(0)?,
                received_ts: row.get(28)?,
                host_id: host.to_string(),
                gid: row.get(27)?,
                online4: row.get(1)?,
                online6: row.get(2)?,
                cpu: row.get(3)?,
                load_1: row.get(4)?,
                load_5: row.get(5)?,
                load_15: row.get(6)?,
                memory_used: row.get(7)?,
                memory_total: row.get(8)?,
                swap_used: row.get(9)?,
                swap_total: row.get(10)?,
                hdd_used: row.get(11)?,
                hdd_total: row.get(12)?,
                tcp: row.get(13)?,
                udp: row.get(14)?,
                process: row.get(15)?,
                thread: row.get(16)?,
                network_in: row.get(17)?,
                network_out: row.get(18)?,
                network_rx: row.get(19)?,
                network_tx: row.get(20)?,
                ping_10010: row.get(21)?,
                ping_189: row.get(22)?,
                ping_10086: row.get(23)?,
                time_10010: row.get(24)?,
                time_189: row.get(25)?,
                time_10086: row.get(26)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn load_rollups(
        conn: &Connection,
        host: &str,
        gid: &str,
        tier: &str,
        from: u64,
        to: u64,
    ) -> Result<Vec<(u64, RollupData)>> {
        let mut stmt = conn.prepare(
            "SELECT bucket_ts, data FROM metric_rollups
             WHERE host_id = ?1 AND gid = ?2 AND tier = ?3
               AND bucket_ts >= ?4 AND bucket_ts < ?5
             ORDER BY bucket_ts ASC",
        )?;
        let rows = stmt.query_map(params![host, gid, tier, from, to], |row| {
            Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.map(|row| {
            let (ts, data) = row?;
            Ok((ts, serde_json::from_str::<RollupData>(&data)?))
        })
        .collect()
    }

    fn previous_counters(conn: &Connection, host: &str, gid: &str, before: u64) -> Result<Option<(u64, u64, u64)>> {
        conn.query_row(
            "SELECT ts, network_in, network_out FROM metric_samples
             WHERE host_id = ?1 AND gid = ?2 AND ts < ?3 ORDER BY ts DESC LIMIT 1",
            params![host, gid, before],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(Into::into)
    }

    fn aggregate_raw_bucket(conn: &Connection, host: &str, gid: &str, start: u64, end: u64) -> Result<RollupData> {
        let points = Self::load_raw_points(conn, host, Some(gid), start, end)?;
        let mut result = RollupData::default();
        let mut previous = Self::previous_counters(conn, host, gid, start)?;
        for point in points {
            result.add_raw(&point);
            if let Some((previous_ts, incoming, outgoing)) = previous {
                if point.ts > previous_ts.saturating_add(1) {
                    result.gap_count = result
                        .gap_count
                        .saturating_add(point.ts.saturating_sub(previous_ts).saturating_sub(1));
                }
                if point.network_in >= incoming {
                    result.add_sum("traffic_in", (point.network_in - incoming) as f64, point.ts);
                } else {
                    result.reset_count = result.reset_count.saturating_add(1);
                    result.add_sum("traffic_in", 0.0, point.ts);
                }
                if point.network_out >= outgoing {
                    result.add_sum("traffic_out", (point.network_out - outgoing) as f64, point.ts);
                } else {
                    result.reset_count = result.reset_count.saturating_add(1);
                    result.add_sum("traffic_out", 0.0, point.ts);
                }
            } else {
                result.add_sum("traffic_in", 0.0, point.ts);
                result.add_sum("traffic_out", 0.0, point.ts);
            }
            previous = Some((point.ts, point.network_in, point.network_out));
        }
        Ok(result)
    }

    fn aggregate_rollup_bucket(
        conn: &Connection,
        host: &str,
        gid: &str,
        source_tier: &str,
        start: u64,
        end: u64,
    ) -> Result<RollupData> {
        let mut result = RollupData::default();
        for (_, data) in Self::load_rollups(conn, host, gid, source_tier, start, end)? {
            result.add_rollup(&data);
        }
        Ok(result)
    }

    fn compact_dirty(&self, conn: &mut Connection, exhaustive: bool) -> Result<u64> {
        let limit = if exhaustive { 50_000_i64 } else { 2_000_i64 };
        let mut compacted = 0_u64;
        for tier_index in 1..self.tiers.len() {
            let tier = &self.tiers[tier_index];
            let dirty = {
                let mut stmt = conn.prepare(
                    "SELECT host_id, gid, bucket_ts FROM history_dirty
                     WHERE tier = ?1 ORDER BY bucket_ts ASC LIMIT ?2",
                )?;
                let rows = stmt.query_map(params![tier.name, limit], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, u64>(2)?,
                    ))
                })?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            };
            for (host, gid, bucket_ts) in dirty {
                let bucket_end = tier.resolution.bucket_end(bucket_ts);
                let data = if tier_index == 1 {
                    Self::aggregate_raw_bucket(conn, &host, &gid, bucket_ts, bucket_end)?
                } else {
                    Self::aggregate_rollup_bucket(
                        conn,
                        &host,
                        &gid,
                        &self.tiers[tier_index - 1].name,
                        bucket_ts,
                        bucket_end,
                    )?
                };
                let transaction = conn.transaction()?;
                if data.sample_count > 0 {
                    transaction.execute(
                        "INSERT INTO metric_rollups(
                            host_id, gid, tier, bucket_ts, bucket_end, sample_count, data
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                         ON CONFLICT(host_id, gid, tier, bucket_ts) DO UPDATE SET
                            bucket_end=excluded.bucket_end,
                            sample_count=excluded.sample_count,
                            data=excluded.data",
                        params![
                            host,
                            gid,
                            tier.name,
                            bucket_ts,
                            bucket_end,
                            data.sample_count,
                            serde_json::to_string(&data)?
                        ],
                    )?;
                    for parent in self.tiers.iter().skip(tier_index + 1) {
                        transaction.execute(
                            "INSERT OR IGNORE INTO history_dirty(host_id, gid, tier, bucket_ts)
                             VALUES (?1, ?2, ?3, ?4)",
                            params![host, gid, parent.name, parent.resolution.bucket_start(bucket_ts)],
                        )?;
                    }
                }
                transaction.execute(
                    "DELETE FROM history_dirty
                     WHERE host_id = ?1 AND gid = ?2 AND tier = ?3 AND bucket_ts = ?4",
                    params![host, gid, tier.name, bucket_ts],
                )?;
                transaction.execute(
                    "INSERT INTO history_watermarks(tier, complete_until, updated_at)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(tier) DO UPDATE SET
                        complete_until=MAX(complete_until, excluded.complete_until),
                        updated_at=excluded.updated_at",
                    params![tier.name, bucket_end, now_secs()],
                )?;
                transaction.commit()?;
                compacted = compacted.saturating_add(1);
            }
        }
        Ok(compacted)
    }

    fn cleanup_tiers(&self, conn: &mut Connection, now: u64, exhaustive: bool) -> Result<u64> {
        let limit = i64::try_from(if exhaustive {
            self.delete_batch_size.saturating_mul(100)
        } else {
            self.delete_batch_size
        })
        .unwrap_or(i64::MAX);
        let mut deleted = 0_u64;
        if let Some(raw) = self.tiers.first() {
            let cutoff = raw.retention.cutoff(now);
            let candidates = {
                let mut stmt = conn.prepare(
                    "SELECT rowid, host_id, gid, ts FROM metric_samples
                     WHERE ts < ?1 ORDER BY ts ASC LIMIT ?2",
                )?;
                let rows = stmt.query_map(params![cutoff, limit], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, u64>(3)?,
                    ))
                })?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            };
            for (rowid, host, gid, timestamp) in candidates {
                let safe = if let Some(parent) = self.tiers.get(1) {
                    conn.query_row(
                        "SELECT EXISTS(
                            SELECT 1 FROM metric_rollups
                            WHERE host_id=?1 AND gid=?2 AND tier=?3 AND bucket_ts=?4
                         )",
                        params![host, gid, parent.name, parent.resolution.bucket_start(timestamp)],
                        |row| row.get::<_, bool>(0),
                    )?
                } else {
                    true
                };
                if safe {
                    deleted = deleted.saturating_add(u64::try_from(
                        conn.execute("DELETE FROM metric_samples WHERE rowid=?1", params![rowid])?,
                    )?);
                }
            }
        }
        for tier_index in 1..self.tiers.len() {
            let tier = &self.tiers[tier_index];
            let cutoff = tier.retention.cutoff(now);
            let candidates = {
                let mut stmt = conn.prepare(
                    "SELECT host_id, gid, bucket_ts FROM metric_rollups
                     WHERE tier = ?1 AND bucket_ts < ?2 ORDER BY bucket_ts ASC LIMIT ?3",
                )?;
                let rows = stmt.query_map(params![tier.name, cutoff, limit], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, u64>(2)?,
                    ))
                })?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            };
            for (host, gid, bucket_ts) in candidates {
                let safe = if let Some(parent) = self.tiers.get(tier_index + 1) {
                    let parent_bucket = parent.resolution.bucket_start(bucket_ts);
                    conn.query_row(
                        "SELECT EXISTS(
                            SELECT 1 FROM metric_rollups
                            WHERE host_id=?1 AND gid=?2 AND tier=?3 AND bucket_ts=?4
                         )",
                        params![host, gid, parent.name, parent_bucket],
                        |row| row.get::<_, bool>(0),
                    )?
                } else {
                    true
                };
                if safe {
                    deleted = deleted.saturating_add(u64::try_from(conn.execute(
                        "DELETE FROM metric_rollups
                         WHERE host_id=?1 AND gid=?2 AND tier=?3 AND bucket_ts=?4",
                        params![host, gid, tier.name, bucket_ts],
                    )?)?);
                }
            }
        }
        Ok(deleted)
    }

    fn seed_missing_dirty(&self, conn: &mut Connection) -> Result<u64> {
        if self.tiers.len() < 2 {
            return Ok(0);
        }
        let tier_signature = self
            .tiers
            .iter()
            .skip(1)
            .map(|tier| tier.name.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let seeded = conn
            .query_row("SELECT value FROM schema_meta WHERE key = 'rollup_seeded'", [], |row| {
                row.get::<_, String>(0)
            })
            .optional()?;
        if seeded.as_deref() == Some(tier_signature.as_str()) {
            return Ok(0);
        }
        let mut inserted = 0_u64;
        for tier_index in 1..self.tiers.len() {
            let tier = &self.tiers[tier_index];
            let rows = if tier_index == 1 {
                let mut stmt = conn.prepare("SELECT DISTINCT host_id, gid, ts FROM metric_samples")?;
                let rows = stmt.query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, u64>(2)?,
                    ))
                })?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            } else {
                let mut stmt =
                    conn.prepare("SELECT DISTINCT host_id, gid, bucket_ts FROM metric_rollups WHERE tier = ?1")?;
                let rows = stmt.query_map(params![self.tiers[tier_index - 1].name], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, u64>(2)?,
                    ))
                })?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            };
            let transaction = conn.transaction()?;
            for (host, gid, ts) in rows {
                inserted = inserted.saturating_add(u64::try_from(transaction.execute(
                    "INSERT OR IGNORE INTO history_dirty(host_id, gid, tier, bucket_ts)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![host, gid, tier.name, tier.resolution.bucket_start(ts)],
                )?)?);
            }
            transaction.commit()?;
        }
        let transaction = conn.transaction()?;
        transaction.execute(
            "INSERT INTO schema_meta(key, value) VALUES ('rollup_seeded', ?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![tier_signature],
        )?;
        transaction.commit()?;
        Ok(inserted)
    }
}

impl HistoryStore for SqliteHistoryStore {
    fn write(&self, point: &HistoryPoint) -> Result<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("history database mutex poisoned"))?;
        let transaction = conn.transaction()?;
        transaction.execute(
            "INSERT INTO metric_samples (
                host_id, ts, gid, online4, online6, cpu, load_1, load_5, load_15,
                memory_used, memory_total, swap_used, swap_total, hdd_used, hdd_total,
                tcp, udp, process, thread, network_in, network_out, network_rx, network_tx,
                ping_10010, ping_189, ping_10086, time_10010, time_189, time_10086,
                received_ts
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30
             )
             ON CONFLICT(host_id, gid, ts) DO UPDATE SET
                gid=excluded.gid, online4=excluded.online4, online6=excluded.online6,
                cpu=excluded.cpu, load_1=excluded.load_1, load_5=excluded.load_5,
                load_15=excluded.load_15, memory_used=excluded.memory_used,
                memory_total=excluded.memory_total, swap_used=excluded.swap_used,
                swap_total=excluded.swap_total, hdd_used=excluded.hdd_used,
                hdd_total=excluded.hdd_total, tcp=excluded.tcp, udp=excluded.udp,
                process=excluded.process, thread=excluded.thread,
                network_in=excluded.network_in, network_out=excluded.network_out,
                network_rx=excluded.network_rx, network_tx=excluded.network_tx,
                ping_10010=excluded.ping_10010, ping_189=excluded.ping_189,
                ping_10086=excluded.ping_10086, time_10010=excluded.time_10010,
                time_189=excluded.time_189, time_10086=excluded.time_10086,
                received_ts=excluded.received_ts",
            params![
                point.host_id,
                point.ts,
                point.gid,
                point.online4,
                point.online6,
                point.cpu,
                point.load_1,
                point.load_5,
                point.load_15,
                point.memory_used,
                point.memory_total,
                point.swap_used,
                point.swap_total,
                point.hdd_used,
                point.hdd_total,
                point.tcp,
                point.udp,
                point.process,
                point.thread,
                point.network_in,
                point.network_out,
                point.network_rx,
                point.network_tx,
                point.ping_10010,
                point.ping_189,
                point.ping_10086,
                point.time_10010,
                point.time_189,
                point.time_10086,
                point.received_ts,
            ],
        )?;
        for tier in self.tiers.iter().skip(1) {
            transaction.execute(
                "INSERT OR IGNORE INTO history_dirty(host_id, gid, tier, bucket_ts)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    point.host_id,
                    point.gid,
                    tier.name,
                    tier.resolution.bucket_start(point.ts)
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    fn query(&self, query: &HistoryQuery) -> Result<Value> {
        let now = now_secs();
        let from = query.from.unwrap_or(now.saturating_sub(86_400));
        let to = query.to.unwrap_or(now);
        let range = to.saturating_sub(from);
        if from >= to {
            return Err(anyhow!("history `from` must be before `to`"));
        }
        if range > 10 * 366 * 86_400 {
            return Err(anyhow!("history query range cannot exceed 10 years"));
        }
        let metrics = parse_metrics(query.metrics.as_deref())?;
        let requested_resolution = query
            .resolution
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.eq_ignore_ascii_case("auto"));
        let tier_index = if let Some(resolution) = requested_resolution {
            self.tier_index(resolution)
                .ok_or_else(|| anyhow!("unsupported history resolution `{resolution}`"))?
        } else {
            self.auto_tier_index(now, from)
        };
        let source_resolution = self
            .tiers
            .get(tier_index)
            .map(|tier| tier.resolution.approximate_seconds())
            .unwrap_or(1);
        let requested_step = query.step.unwrap_or(source_resolution).max(source_resolution);
        let max_points = query
            .max_points
            .unwrap_or(self.max_query_points)
            .clamp(1, self.max_query_points);
        let point_step = requested_step.max((range.saturating_add(max_points as u64 - 1)) / max_points as u64);
        let step =
            point_step.saturating_add(source_resolution.saturating_sub(1)) / source_resolution * source_resolution;
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("history database mutex poisoned"))?;
        let mut series: HashMap<String, Vec<Value>> =
            metrics.iter().map(|metric| (metric.to_string(), Vec::new())).collect();
        if tier_index == 0 {
            let rows = Self::load_raw_points(&conn, &query.host, query.gid.as_deref(), from, to)?;
            let mut previous = HashMap::<String, (u64, u64)>::new();
            let mut buckets = BTreeMap::<u64, RollupData>::new();
            for point in rows {
                let bucket = point.ts / step * step;
                if !previous.contains_key(&point.gid) {
                    if let Some((_, incoming, outgoing)) =
                        Self::previous_counters(&conn, &query.host, &point.gid, from)?
                    {
                        previous.insert(point.gid.clone(), (incoming, outgoing));
                    }
                }
                let counters = previous.insert(point.gid.clone(), (point.network_in, point.network_out));
                let traffic_in = counters
                    .map(|(value, _)| point.network_in.saturating_sub(value))
                    .unwrap_or(0);
                let traffic_out = counters
                    .map(|(_, value)| point.network_out.saturating_sub(value))
                    .unwrap_or(0);
                let data = buckets.entry(bucket).or_default();
                data.add_raw(&point);
                data.add_sum("traffic_in", traffic_in as f64, point.ts);
                data.add_sum("traffic_out", traffic_out as f64, point.ts);
            }
            for (bucket, data) in buckets {
                for metric in &metrics {
                    if let Some(value) = data.value(metric) {
                        series
                            .get_mut(*metric)
                            .expect("history metric was initialized")
                            .push(json!([bucket, value]));
                    }
                }
            }
        } else {
            let tier = &self.tiers[tier_index];
            let mut stmt = conn.prepare(
                "SELECT bucket_ts, data FROM metric_rollups
                 WHERE host_id = ?1 AND (?2 IS NULL OR gid = ?2) AND tier = ?3
                   AND bucket_ts >= ?4 AND bucket_ts < ?5
                 ORDER BY bucket_ts ASC",
            )?;
            let rows = stmt.query_map(
                params![query.host.as_str(), query.gid.as_deref(), tier.name, from, to],
                |row| Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?)),
            )?;
            let mut buckets = BTreeMap::<u64, RollupData>::new();
            let calendar_group_size = step.saturating_add(source_resolution.saturating_sub(1)) / source_resolution;
            let mut calendar_bucket = 0_u64;
            for (index, row) in rows.enumerate() {
                let (timestamp, encoded) = row?;
                let data: RollupData = serde_json::from_str(&encoded)?;
                let bucket = if tier.resolution == BucketResolution::CalendarMonth {
                    if index % usize::try_from(calendar_group_size.max(1)).unwrap_or(usize::MAX) == 0 {
                        calendar_bucket = timestamp;
                    }
                    calendar_bucket
                } else {
                    timestamp / step * step
                };
                buckets.entry(bucket).or_default().add_rollup(&data);
            }
            for (bucket, data) in buckets {
                for metric in &metrics {
                    if let Some(value) = data.value(metric) {
                        series
                            .get_mut(*metric)
                            .expect("history metric was initialized")
                            .push(json!([bucket, value]));
                    }
                }
            }
        }
        Ok(json!({
            "host": query.host,
            "from": from,
            "to": to,
            "step": step,
            "resolution": self.tiers.get(tier_index).map(|tier| tier.name.as_str()).unwrap_or("1s"),
            "series": series,
        }))
    }

    fn delete_before(&self, before: u64, batch_size: usize) -> Result<u64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("history database mutex poisoned"))?;
        let batch_size = i64::try_from(batch_size.max(1)).unwrap_or(i64::MAX);
        let deleted = conn.execute(
            "DELETE FROM metric_samples WHERE rowid IN (
                SELECT rowid FROM metric_samples WHERE ts < ?1 LIMIT ?2
             )",
            params![before, batch_size],
        )?;
        Ok(u64::try_from(deleted).unwrap_or(0))
    }

    fn maintain(&self, now: u64, exhaustive: bool) -> Result<MaintenanceReport> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("history database mutex poisoned"))?;
        let seeded = self.seed_missing_dirty(&mut conn)?;
        let compacted = self.compact_dirty(&mut conn, exhaustive)?;
        let deleted = self.cleanup_tiers(&mut conn, now, exhaustive)?;
        let dirty_remaining = conn.query_row("SELECT COUNT(*) FROM history_dirty", [], |row| row.get::<_, u64>(0))?;
        Ok(MaintenanceReport {
            compacted: compacted.saturating_add(seeded),
            deleted,
            dirty_remaining,
        })
    }

    fn status(&self) -> Result<Value> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("history database mutex poisoned"))?;
        let raw = conn.query_row(
            "SELECT COUNT(*), COALESCE(MIN(ts), 0), COALESCE(MAX(ts), 0) FROM metric_samples",
            [],
            |row| Ok(json!({"tier": "raw", "count": row.get::<_, u64>(0)?, "from": row.get::<_, u64>(1)?, "to": row.get::<_, u64>(2)?})),
        )?;
        let mut tiers = vec![raw];
        let mut stmt = conn.prepare(
            "SELECT tier, COUNT(*), COALESCE(MIN(bucket_ts), 0), COALESCE(MAX(bucket_ts), 0)
             FROM metric_rollups GROUP BY tier ORDER BY tier",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(json!({
                "tier": row.get::<_, String>(0)?,
                "count": row.get::<_, u64>(1)?,
                "from": row.get::<_, u64>(2)?,
                "to": row.get::<_, u64>(3)?,
            }))
        })?;
        for row in rows {
            tiers.push(row?);
        }
        let dirty = conn.query_row("SELECT COUNT(*) FROM history_dirty", [], |row| row.get::<_, u64>(0))?;
        let schema_version = conn
            .query_row(
                "SELECT value FROM schema_meta WHERE key = 'schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let configured_tiers = self
            .tiers
            .iter()
            .map(|tier| {
                json!({
                    "resolution": tier.name,
                    "resolution_seconds_approx": tier.resolution.approximate_seconds(),
                    "retention": tier.retention_label,
                    "retention_seconds_approx": tier.retention.approximate_seconds(),
                })
            })
            .collect::<Vec<_>>();
        let database_bytes = fs::metadata(&self.path).map_or(0, |metadata| metadata.len());
        let wal_bytes = fs::metadata(format!("{}-wal", self.path)).map_or(0, |metadata| metadata.len());
        let shm_bytes = fs::metadata(format!("{}-shm", self.path)).map_or(0, |metadata| metadata.len());
        Ok(json!({
            "backend": "sqlite",
            "path": self.path.as_str(),
            "bytes": {
                "database": database_bytes,
                "wal": wal_bytes,
                "shm": shm_bytes,
                "total": database_bytes.saturating_add(wal_bytes).saturating_add(shm_bytes),
            },
            "schema_version": schema_version,
            "raw": tiers.remove(0),
            "tiers": tiers,
            "configured_tiers": configured_tiers,
            "dirty": dirty,
            "max_query_points": self.max_query_points,
        }))
    }

    fn verify(&self) -> Result<Value> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("history database mutex poisoned"))?;
        let quick_check = {
            let mut stmt = conn.prepare("PRAGMA quick_check")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        let sqlite_ok = quick_check.len() == 1 && quick_check.first().is_some_and(|value| value == "ok");
        let dirty = conn.query_row("SELECT COUNT(*) FROM history_dirty", [], |row| row.get::<_, u64>(0))?;

        let configured = self.tiers.iter().map(|tier| tier.name.as_str()).collect::<HashSet<_>>();
        let unknown_tiers = {
            let mut stmt = conn.prepare("SELECT DISTINCT tier FROM metric_rollups ORDER BY tier")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
                .into_iter()
                .filter(|tier| !configured.contains(tier.as_str()))
                .collect::<Vec<_>>()
        };

        let now = now_secs();
        let mut expired = Vec::new();
        if let Some(raw) = self.tiers.first() {
            let count = conn.query_row(
                "SELECT COUNT(*) FROM metric_samples WHERE ts < ?1",
                params![raw.retention.cutoff(now)],
                |row| row.get::<_, u64>(0),
            )?;
            expired.push(json!({"tier": raw.name, "count": count}));
        }
        for tier in self.tiers.iter().skip(1) {
            let count = conn.query_row(
                "SELECT COUNT(*) FROM metric_rollups WHERE tier = ?1 AND bucket_ts < ?2",
                params![tier.name, tier.retention.cutoff(now)],
                |row| row.get::<_, u64>(0),
            )?;
            expired.push(json!({"tier": tier.name, "count": count}));
        }
        let expired_total = expired
            .iter()
            .filter_map(|entry| entry.get("count").and_then(Value::as_u64))
            .sum::<u64>();
        Ok(json!({
            "ok": sqlite_ok && dirty == 0 && expired_total == 0 && unknown_tiers.is_empty(),
            "sqlite_quick_check": quick_check,
            "dirty": dirty,
            "expired": expired,
            "expired_total": expired_total,
            "unknown_tiers": unknown_tiers,
        }))
    }

    fn prune(&self, before: u64, host: Option<&str>, tier: Option<&str>, apply: bool) -> Result<Value> {
        let requested_tier = tier.map(str::trim);
        if requested_tier.is_some_and(str::is_empty) {
            return Err(anyhow!("history tier cannot be empty"));
        }
        let raw_selected = requested_tier.is_none()
            || requested_tier.is_some_and(|value| {
                value.eq_ignore_ascii_case("raw")
                    || value.eq_ignore_ascii_case("1s")
                    || self
                        .tiers
                        .first()
                        .is_some_and(|raw| raw.name.eq_ignore_ascii_case(value))
            });
        let all_rollups_selected = requested_tier.is_none();
        let rollup_tier = if raw_selected {
            None
        } else {
            let value = requested_tier.unwrap_or_default();
            let configured = self
                .tiers
                .iter()
                .skip(1)
                .find(|candidate| candidate.name.eq_ignore_ascii_case(value))
                .map(|candidate| candidate.name.clone());
            let stored = if configured.is_some() {
                configured
            } else {
                let conn = self
                    .conn
                    .lock()
                    .map_err(|_| anyhow!("history database mutex poisoned"))?;
                conn.query_row(
                    "SELECT tier FROM metric_rollups WHERE tier = ?1 COLLATE NOCASE LIMIT 1",
                    params![value],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
            };
            Some(stored.ok_or_else(|| anyhow!("unsupported history tier `{value}`"))?)
        };
        let rollup_tier = rollup_tier.as_deref();
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("history database mutex poisoned"))?;
        let raw_count = if !raw_selected {
            0
        } else if let Some(host) = host {
            conn.query_row(
                "SELECT COUNT(*) FROM metric_samples WHERE ts < ?1 AND host_id = ?2",
                params![before, host],
                |row| row.get::<_, u64>(0),
            )?
        } else {
            conn.query_row(
                "SELECT COUNT(*) FROM metric_samples WHERE ts < ?1",
                params![before],
                |row| row.get::<_, u64>(0),
            )?
        };
        let rollup_count = if !all_rollups_selected && rollup_tier.is_none() {
            0
        } else {
            match (host, rollup_tier) {
                (Some(host), Some(tier)) => conn.query_row(
                    "SELECT COUNT(*) FROM metric_rollups WHERE bucket_ts < ?1 AND host_id = ?2 AND tier = ?3",
                    params![before, host, tier],
                    |row| row.get::<_, u64>(0),
                )?,
                (Some(host), None) => conn.query_row(
                    "SELECT COUNT(*) FROM metric_rollups WHERE bucket_ts < ?1 AND host_id = ?2",
                    params![before, host],
                    |row| row.get::<_, u64>(0),
                )?,
                (None, Some(tier)) => conn.query_row(
                    "SELECT COUNT(*) FROM metric_rollups WHERE bucket_ts < ?1 AND tier = ?2",
                    params![before, tier],
                    |row| row.get::<_, u64>(0),
                )?,
                (None, None) => conn.query_row(
                    "SELECT COUNT(*) FROM metric_rollups WHERE bucket_ts < ?1",
                    params![before],
                    |row| row.get::<_, u64>(0),
                )?,
            }
        };
        drop(conn);
        let mut deleted = 0_u64;
        if apply {
            let conn = self
                .conn
                .lock()
                .map_err(|_| anyhow!("history database mutex poisoned"))?;
            if !raw_selected {
                // Raw seconds were not selected.
            } else if let Some(host) = host {
                deleted = deleted.saturating_add(u64::try_from(conn.execute(
                    "DELETE FROM metric_samples WHERE ts < ?1 AND host_id = ?2",
                    params![before, host],
                )?)?);
            } else {
                deleted = deleted.saturating_add(u64::try_from(
                    conn.execute("DELETE FROM metric_samples WHERE ts < ?1", params![before])?,
                )?);
            }
            if all_rollups_selected || rollup_tier.is_some() {
                match (host, rollup_tier) {
                    (Some(host), Some(tier)) => {
                        deleted = deleted.saturating_add(u64::try_from(conn.execute(
                            "DELETE FROM metric_rollups WHERE bucket_ts < ?1 AND host_id = ?2 AND tier = ?3",
                            params![before, host, tier],
                        )?)?)
                    }
                    (Some(host), None) => {
                        deleted = deleted.saturating_add(u64::try_from(conn.execute(
                            "DELETE FROM metric_rollups WHERE bucket_ts < ?1 AND host_id = ?2",
                            params![before, host],
                        )?)?)
                    }
                    (None, Some(tier)) => {
                        deleted = deleted.saturating_add(u64::try_from(conn.execute(
                            "DELETE FROM metric_rollups WHERE bucket_ts < ?1 AND tier = ?2",
                            params![before, tier],
                        )?)?)
                    }
                    (None, None) => {
                        deleted = deleted.saturating_add(u64::try_from(
                            conn.execute("DELETE FROM metric_rollups WHERE bucket_ts < ?1", params![before])?,
                        )?)
                    }
                }
                match (host, rollup_tier) {
                    (Some(host), Some(tier)) => {
                        conn.execute(
                            "DELETE FROM history_dirty WHERE bucket_ts < ?1 AND host_id = ?2 AND tier = ?3",
                            params![before, host, tier],
                        )?;
                    }
                    (Some(host), None) => {
                        conn.execute(
                            "DELETE FROM history_dirty WHERE bucket_ts < ?1 AND host_id = ?2",
                            params![before, host],
                        )?;
                    }
                    (None, Some(tier)) => {
                        conn.execute(
                            "DELETE FROM history_dirty WHERE bucket_ts < ?1 AND tier = ?2",
                            params![before, tier],
                        )?;
                    }
                    (None, None) => {
                        conn.execute("DELETE FROM history_dirty WHERE bucket_ts < ?1", params![before])?;
                    }
                }
            }
        }
        Ok(json!({
            "before": before,
            "raw_candidates": raw_count,
            "rollup_candidates": rollup_count,
            "deleted": deleted,
            "applied": apply,
        }))
    }

    fn vacuum(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("history database mutex poisoned"))?;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")?;
        Ok(())
    }
}

fn parse_metrics(input: Option<&str>) -> Result<Vec<&'static str>> {
    let requested = input
        .unwrap_or("cpu,load_1,memory_used,network_in,network_out")
        .split(',')
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .collect::<Vec<_>>();
    if requested.is_empty() {
        return Err(anyhow!("at least one history metric is required"));
    }
    requested
        .into_iter()
        .map(|metric| {
            METRICS
                .iter()
                .copied()
                .find(|allowed| *allowed == metric)
                .ok_or_else(|| anyhow!("unsupported history metric `{metric}`"))
        })
        .collect()
}

#[must_use]
pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(ts: u64, cpu: f64, network_in: u64) -> HistoryPoint {
        HistoryPoint {
            ts,
            received_ts: ts,
            host_id: "host-1".to_string(),
            gid: String::new(),
            online4: true,
            online6: false,
            cpu,
            load_1: 0.0,
            load_5: 0.0,
            load_15: 0.0,
            memory_used: 0,
            memory_total: 0,
            swap_used: 0,
            swap_total: 0,
            hdd_used: 0,
            hdd_total: 0,
            tcp: 0,
            udp: 0,
            process: 0,
            thread: 0,
            network_in,
            network_out: 0,
            network_rx: 0,
            network_tx: 0,
            ping_10010: 0.0,
            ping_189: 0.0,
            ping_10086: 0.0,
            time_10010: 0.0,
            time_189: 0.0,
            time_10086: 0.0,
        }
    }

    fn store() -> SqliteHistoryStore {
        let config = HistoryConfig {
            path: ":memory:".to_string(),
            ..HistoryConfig::default()
        };
        SqliteHistoryStore::open(&config).unwrap()
    }

    #[test]
    fn sqlite_query_handles_counter_reset_and_response_buckets() {
        let store = store();
        store.write(&point(100, 10.0, 1000)).unwrap();
        store.write(&point(110, 20.0, 1500)).unwrap();
        store.write(&point(120, 30.0, 10)).unwrap();

        let result = store
            .query(&HistoryQuery {
                host: "host-1".to_string(),
                gid: None,
                from: Some(90),
                to: Some(130),
                step: Some(20),
                metrics: Some("cpu,traffic_in".to_string()),
                resolution: Some("1s".to_string()),
                max_points: Some(1200),
            })
            .unwrap();

        assert_eq!(result["series"]["cpu"], json!([[100, 15.0], [120, 30.0]]));
        assert_eq!(result["series"]["traffic_in"], json!([[100, 500.0], [120, 0.0]]));

        let result = store
            .query(&HistoryQuery {
                host: "host-1".to_string(),
                gid: None,
                from: Some(105),
                to: Some(115),
                step: Some(0),
                metrics: Some("traffic_in".to_string()),
                resolution: Some("1s".to_string()),
                max_points: Some(1200),
            })
            .unwrap();
        assert_eq!(result["series"]["traffic_in"], json!([[110, 500.0]]));
    }

    #[test]
    fn cleanup_deletes_in_batches() {
        let store = store();
        store.write(&point(100, 10.0, 1000)).unwrap();
        store.write(&point(200, 20.0, 2000)).unwrap();
        assert_eq!(store.delete_before(300, 1).unwrap(), 1);
        assert_eq!(store.delete_before(300, 10).unwrap(), 1);
        assert_eq!(store.delete_before(300, 10).unwrap(), 0);
    }

    #[test]
    fn tiered_compaction_averages_gauges_and_sums_counter_deltas() {
        let store = store();
        store.write(&point(100, 10.0, 1000)).unwrap();
        store.write(&point(110, 20.0, 1500)).unwrap();
        store.write(&point(120, 30.0, 1700)).unwrap();
        let report = store.maintain(130, true).unwrap();
        assert!(report.compacted > 0);

        let result = store
            .query(&HistoryQuery {
                host: "host-1".to_string(),
                gid: None,
                from: Some(60),
                to: Some(180),
                step: Some(60),
                metrics: Some("cpu,traffic_in".to_string()),
                resolution: Some("1m".to_string()),
                max_points: Some(1200),
            })
            .unwrap();
        assert_eq!(result["series"]["cpu"], json!([[60, 15.0], [120, 30.0]]));
        assert_eq!(result["series"]["traffic_in"], json!([[60, 500.0], [120, 200.0]]));
    }

    #[test]
    fn late_duplicate_recomputes_affected_rollup_idempotently() {
        let store = store();
        store.write(&point(100, 10.0, 1000)).unwrap();
        store.maintain(110, true).unwrap();
        store.write(&point(100, 40.0, 1000)).unwrap();
        store.maintain(120, true).unwrap();
        let result = store
            .query(&HistoryQuery {
                host: "host-1".to_string(),
                gid: None,
                from: Some(60),
                to: Some(120),
                step: Some(60),
                metrics: Some("cpu".to_string()),
                resolution: Some("1m".to_string()),
                max_points: Some(1200),
            })
            .unwrap();
        assert_eq!(result["series"]["cpu"], json!([[60, 40.0]]));
    }

    #[test]
    fn raw_expiry_keeps_compacted_minute_history() {
        let store = store();
        let timestamp = 1_000_000;
        store.write(&point(timestamp, 25.0, 1000)).unwrap();
        store.maintain(timestamp + 901, true).unwrap();
        let status = store.status().unwrap();
        assert_eq!(status["raw"]["count"], 0);
        let result = store
            .query(&HistoryQuery {
                host: "host-1".to_string(),
                gid: None,
                from: Some(timestamp - 60),
                to: Some(timestamp + 60),
                step: Some(60),
                metrics: Some("cpu".to_string()),
                resolution: Some("1m".to_string()),
                max_points: Some(1200),
            })
            .unwrap();
        assert_eq!(result["series"]["cpu"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn retention_keeps_only_tiers_that_cover_the_sample_age() {
        let store = store();
        let now = now_secs();
        let timestamp = now - 2 * 365 * 86_400;
        store.write(&point(timestamp, 25.0, 1_000)).unwrap();
        store.maintain(now, true).unwrap();

        let status = store.status().unwrap();
        assert_eq!(status["raw"]["count"], 0);
        let counts = status["tiers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tier| {
                (
                    tier["tier"].as_str().unwrap().to_string(),
                    tier["count"].as_u64().unwrap(),
                )
            })
            .collect::<HashMap<_, _>>();
        assert_eq!(counts.get("1m"), None);
        assert_eq!(counts.get("5m"), None);
        assert_eq!(counts.get("1h"), None);
        assert_eq!(counts.get("1d"), Some(&1));
        assert_eq!(counts.get("1mo"), Some(&1));

        let result = store
            .query(&HistoryQuery {
                host: "host-1".to_string(),
                gid: None,
                from: Some(timestamp.saturating_sub(86_400)),
                to: Some(timestamp.saturating_add(86_400)),
                step: None,
                metrics: Some("cpu".to_string()),
                resolution: Some("auto".to_string()),
                max_points: Some(1_200),
            })
            .unwrap();
        assert_eq!(result["resolution"], "1d");
        assert_eq!(result["series"]["cpu"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn data_older_than_ten_years_is_removed_from_every_tier() {
        let store = store();
        let now = now_secs();
        store.write(&point(now - 11 * 366 * 86_400, 25.0, 1_000)).unwrap();
        store.maintain(now, true).unwrap();
        let status = store.status().unwrap();
        assert_eq!(status["raw"]["count"], 0);
        assert!(status["tiers"].as_array().is_some_and(Vec::is_empty));
    }

    #[test]
    fn version_one_database_is_migrated_without_losing_samples_table() {
        let path = std::env::temp_dir().join(format!("server-status-history-{}.db", uuid::Uuid::new_v4()));
        let config = HistoryConfig {
            path: path.to_string_lossy().into_owned(),
            ..HistoryConfig::default()
        };
        {
            let store = SqliteHistoryStore::open(&config).unwrap();
            drop(store);
            let conn = Connection::open(&path).unwrap();
            conn.execute("ALTER TABLE metric_samples DROP COLUMN received_ts", [])
                .unwrap();
            conn.execute("UPDATE schema_meta SET value='1' WHERE key='schema_version'", [])
                .unwrap();
        }
        {
            let store = SqliteHistoryStore::open(&config).unwrap();
            let conn = store.conn.lock().unwrap();
            let columns = {
                let mut stmt = conn.prepare("PRAGMA table_info(metric_samples)").unwrap();
                let rows = stmt.query_map([], |row| row.get::<_, String>(1)).unwrap();
                rows.collect::<std::result::Result<Vec<_>, _>>().unwrap()
            };
            assert!(columns.iter().any(|column| column == "received_ts"));
            let version = conn
                .query_row("SELECT value FROM schema_meta WHERE key='schema_version'", [], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap();
            assert_eq!(version, "2");
        }
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(format!("{}-wal", path.to_string_lossy()));
        let _ = fs::remove_file(format!("{}-shm", path.to_string_lossy()));
    }

    #[test]
    fn default_tiers_cover_seconds_through_ten_years() {
        let tiers = parse_tiers(&HistoryConfig::default()).unwrap();
        assert_eq!(
            tiers.iter().map(|tier| tier.name.as_str()).collect::<Vec<_>>(),
            vec!["1s", "1m", "5m", "1h", "1d", "1mo"]
        );
        assert_eq!(tiers[0].retention.approximate_seconds(), 15 * 60);
        assert_eq!(tiers[1].retention.approximate_seconds(), 30 * 86_400);
        assert_eq!(tiers[2].retention.approximate_seconds(), 90 * 86_400);
        assert!(tiers.last().unwrap().retention.approximate_seconds() >= 10 * 365 * 86_400);
    }

    #[test]
    fn legacy_single_tier_configuration_remains_supported() {
        let config = HistoryConfig {
            enabled: true,
            tiers: Vec::new(),
            sample_interval: 60,
            retention_days: 0,
            ..HistoryConfig::default()
        };
        let tiers = parse_tiers(&config).unwrap();
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].name, "60s");
        assert_eq!(tiers[0].retention.approximate_seconds(), u64::MAX);
        assert_eq!(tiers[0].retention.cutoff(now_secs()), 0);
    }

    #[test]
    fn configurable_latest_compression_buckets_by_first_tier() {
        let resolution = BucketResolution::Fixed(60);
        assert_eq!(compression_bucket_timestamp("latest", resolution, 119), 60);
        assert_eq!(compression_bucket_timestamp("none", resolution, 119), 119);
    }

    #[test]
    fn calendar_month_buckets_follow_utc_month_boundaries() {
        let timestamp = u64::try_from(
            Utc.with_ymd_and_hms(2024, 2, 20, 12, 34, 56)
                .single()
                .unwrap()
                .timestamp(),
        )
        .unwrap();
        let expected_start =
            u64::try_from(Utc.with_ymd_and_hms(2024, 2, 1, 0, 0, 0).single().unwrap().timestamp()).unwrap();
        let expected_end =
            u64::try_from(Utc.with_ymd_and_hms(2024, 3, 1, 0, 0, 0).single().unwrap().timestamp()).unwrap();
        assert_eq!(BucketResolution::CalendarMonth.bucket_start(timestamp), expected_start);
        assert_eq!(BucketResolution::CalendarMonth.bucket_end(expected_start), expected_end);
    }

    #[test]
    fn auto_resolution_is_selected_by_data_age() {
        let store = store();
        let now = 2_000_000_000;
        assert_eq!(store.auto_tier_index(now, now - 10 * 60), 0);
        assert_eq!(store.auto_tier_index(now, now - 24 * 60 * 60), 1);
        assert_eq!(store.auto_tier_index(now, now - 40 * 24 * 60 * 60), 2);
        assert_eq!(store.auto_tier_index(now, now - 100 * 24 * 60 * 60), 3);
        assert_eq!(store.auto_tier_index(now, now - 2 * 365 * 24 * 60 * 60), 4);
        assert_eq!(store.auto_tier_index(now, now - 4 * 365 * 24 * 60 * 60), 5);
    }

    #[test]
    fn event_time_accepts_bounded_lateness_and_rejects_bad_clock_values() {
        assert_eq!(select_event_timestamp(Some(700), 1_000, 300), 700);
        assert_eq!(select_event_timestamp(Some(699), 1_000, 300), 1_000);
        assert_eq!(select_event_timestamp(Some(1_030), 1_000, 300), 1_030);
        assert_eq!(select_event_timestamp(Some(1_031), 1_000, 300), 1_000);
        assert_eq!(select_event_timestamp(None, 1_000, 300), 1_000);
    }

    #[test]
    fn query_step_is_aligned_to_the_selected_source_resolution() {
        let store = store();
        let result = store
            .query(&HistoryQuery {
                host: "host-1".to_string(),
                gid: None,
                from: Some(1_000),
                to: Some(11_000),
                step: None,
                metrics: Some("cpu".to_string()),
                resolution: Some("5m".to_string()),
                max_points: Some(10),
            })
            .unwrap();
        assert_eq!(result["resolution"], "5m");
        assert_eq!(result["step"], 1_200);
    }

    #[test]
    fn calendar_month_query_honors_max_points() {
        let store = store();
        let from = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).single().unwrap();
        {
            let conn = store.conn.lock().unwrap();
            for month in 0..12_u32 {
                let timestamp = from.checked_add_months(Months::new(month)).unwrap();
                let timestamp = u64::try_from(timestamp.timestamp()).unwrap();
                let mut data = RollupData {
                    sample_count: 1,
                    ..RollupData::default()
                };
                data.add_value("cpu", f64::from(month), timestamp, 1);
                conn.execute(
                    "INSERT INTO metric_rollups(host_id, gid, tier, bucket_ts, bucket_end, sample_count, data)
                     VALUES (?1, '', '1mo', ?2, ?3, 1, ?4)",
                    params![
                        "host-1",
                        timestamp,
                        BucketResolution::CalendarMonth.bucket_end(timestamp),
                        serde_json::to_string(&data).unwrap()
                    ],
                )
                .unwrap();
            }
        }
        let to = u64::try_from(from.checked_add_months(Months::new(12)).unwrap().timestamp()).unwrap();
        let result = store
            .query(&HistoryQuery {
                host: "host-1".to_string(),
                gid: None,
                from: Some(u64::try_from(from.timestamp()).unwrap()),
                to: Some(to),
                step: None,
                metrics: Some("cpu".to_string()),
                resolution: Some("1mo".to_string()),
                max_points: Some(5),
            })
            .unwrap();
        assert!(result["series"]["cpu"].as_array().unwrap().len() <= 5);
    }

    #[test]
    fn empty_database_passes_integrity_and_retention_verification() {
        let result = store().verify().unwrap();
        assert_eq!(result["ok"], true);
        assert_eq!(result["sqlite_quick_check"], json!(["ok"]));
        assert_eq!(result["expired_total"], 0);
    }

    #[test]
    fn pruning_raw_tier_does_not_delete_rollups() {
        let store = store();
        store.write(&point(100, 10.0, 1_000)).unwrap();
        store.maintain(110, true).unwrap();

        let preview = store.prune(1_000, None, Some("raw"), false).unwrap();
        assert_eq!(preview["raw_candidates"], 1);
        assert_eq!(preview["rollup_candidates"], 0);

        store.prune(1_000, None, Some("raw"), true).unwrap();
        let status = store.status().unwrap();
        assert_eq!(status["raw"]["count"], 0);
        assert!(status["tiers"]
            .as_array()
            .is_some_and(|tiers| tiers.iter().any(|tier| tier["count"].as_u64().unwrap_or(0) > 0)));
        assert!(store.prune(1_000, None, Some("invalid"), false).is_err());
    }
}
