//! Historical metric storage.
//!
//! The rest of the server only depends on `HistoryStore`.  This keeps the
//! ingestion and HTTP layers independent from a concrete database and makes
//! remote backends (VictoriaMetrics, InfluxDB, ...) possible later.

use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::HistoryConfig;
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

#[derive(Debug, Clone)]
pub struct HistoryPoint {
    pub ts: u64,
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

impl HistoryPoint {
    #[must_use]
    pub fn from_stat(stat: &HostStat) -> Self {
        Self {
            ts: stat.latest_ts,
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

pub trait HistoryStore: Send + Sync {
    fn write(&self, point: &HistoryPoint) -> Result<()>;
    fn query(&self, query: &HistoryQuery) -> Result<Value>;
    fn delete_before(&self, before: u64, batch_size: usize) -> Result<u64>;
    fn vacuum(&self) -> Result<()>;
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

pub type SharedHistoryStore = Arc<dyn HistoryStore>;

pub struct HistoryService {
    store: SharedHistoryStore,
    sender: std::sync::mpsc::SyncSender<HistoryPoint>,
    compression: String,
    sample_interval: u64,
    retention_days: u64,
    cleanup_interval: u64,
    delete_batch_size: usize,
    vacuum_after_cleanup: bool,
}

impl HistoryService {
    pub fn new(cfg: &HistoryConfig) -> Result<Option<Arc<Self>>> {
        let Some(store) = create_store(cfg)? else {
            return Ok(None);
        };

        let sample_interval = cfg.sample_interval.max(1);
        let compression = cfg.compression.to_ascii_lowercase();
        if compression != "none" && compression != "latest" {
            return Err(anyhow!(
                "unsupported history compression `{}`; supported values: none, latest",
                cfg.compression
            ));
        }
        let (sender, receiver) = std::sync::mpsc::sync_channel::<HistoryPoint>(2048);
        let cleanup_interval = cfg.cleanup_interval.max(60);
        let retention_days = cfg.retention_days;
        let delete_batch_size = cfg.delete_batch_size.max(1);
        let vacuum_after_cleanup = cfg.vacuum_after_cleanup;
        let worker_store = Arc::clone(&store);
        let worker_compression = compression.clone();
        std::thread::Builder::new()
            .name("history-writer".to_string())
            .spawn(move || {
                let mut last_cleanup = 0_u64;
                loop {
                    match receiver.recv_timeout(std::time::Duration::from_secs(5)) {
                        Ok(mut point) => {
                            if worker_compression == "latest" {
                                point.ts = point.ts / sample_interval * sample_interval;
                            }
                            if let Err(err) = worker_store.write(&point) {
                                error!("history write failed: {err:?}");
                            }
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    }

                    let now = now_secs();
                    if retention_days > 0 && now >= last_cleanup.saturating_add(cleanup_interval) {
                        last_cleanup = now;
                        let cutoff = now.saturating_sub(retention_days.saturating_mul(86_400));
                        loop {
                            match worker_store.delete_before(cutoff, delete_batch_size) {
                                Ok(0) => break,
                                Ok(deleted) => trace!("history cleanup deleted {deleted} rows"),
                                Err(err) => {
                                    error!("history cleanup failed: {err:?}");
                                    break;
                                }
                            }
                        }
                        if vacuum_after_cleanup {
                            if let Err(err) = worker_store.vacuum() {
                                error!("history vacuum failed: {err:?}");
                            }
                        }
                    }
                }
            })
            .context("start history writer thread")?;

        Ok(Some(Arc::new(Self {
            store,
            sender,
            compression,
            sample_interval,
            retention_days,
            cleanup_interval,
            delete_batch_size,
            vacuum_after_cleanup,
        })))
    }

    pub fn record(&self, stat: &HostStat) {
        let point = HistoryPoint::from_stat(stat);
        if self.sender.try_send(point).is_err() {
            warn!("history queue is full; dropping a historical sample");
        }
    }

    pub fn query(&self, query: &HistoryQuery) -> Result<Value> {
        self.store.query(query)
    }

    #[allow(dead_code)]
    pub fn config_summary(&self) -> (&str, u64, u64, u64, usize, bool) {
        (
            self.compression.as_str(),
            self.sample_interval,
            self.retention_days,
            self.cleanup_interval,
            self.delete_batch_size,
            self.vacuum_after_cleanup,
        )
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
    Ok(Some(Arc::new(SqliteHistoryStore::open(&cfg.path)?)))
}

struct SqliteHistoryStore {
    conn: Mutex<Connection>,
}

impl SqliteHistoryStore {
    fn open(path: &str) -> Result<Self> {
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
             INSERT OR IGNORE INTO schema_meta(key, value) VALUES ('schema_version', '1');
             CREATE TABLE IF NOT EXISTS metric_samples (
                 host_id TEXT NOT NULL,
                 ts INTEGER NOT NULL,
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
                 ON metric_samples(ts);",
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }
}

impl HistoryStore for SqliteHistoryStore {
    fn write(&self, point: &HistoryPoint) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("history database mutex poisoned"))?;
        conn.execute(
            "INSERT INTO metric_samples (
                host_id, ts, gid, online4, online6, cpu, load_1, load_5, load_15,
                memory_used, memory_total, swap_used, swap_total, hdd_used, hdd_total,
                tcp, udp, process, thread, network_in, network_out, network_rx, network_tx,
                ping_10010, ping_189, ping_10086, time_10010, time_189, time_10086
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29
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
                time_189=excluded.time_189, time_10086=excluded.time_10086",
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
            ],
        )?;
        Ok(())
    }

    fn query(&self, query: &HistoryQuery) -> Result<Value> {
        let now = now_secs();
        let from = query.from.unwrap_or(now.saturating_sub(86_400));
        let to = query.to.unwrap_or(now);
        if from >= to {
            return Err(anyhow!("history `from` must be before `to`"));
        }
        if to.saturating_sub(from) > 31 * 86_400 {
            return Err(anyhow!("history query range cannot exceed 31 days"));
        }
        let metrics = parse_metrics(query.metrics.as_deref())?;
        let step = query.step.unwrap_or(0);
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("history database mutex poisoned"))?;
        let mut previous_counters: HashMap<&str, f64> = HashMap::new();
        if metrics.contains(&"traffic_in") || metrics.contains(&"traffic_out") {
            let previous = conn
                .query_row(
                    "SELECT network_in, network_out
                     FROM metric_samples
                     WHERE host_id = ?1 AND (?3 IS NULL OR gid = ?3) AND ts < ?2
                     ORDER BY ts DESC
                     LIMIT 1",
                    params![query.host.as_str(), from, query.gid.as_deref()],
                    |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?)),
                )
                .optional()?;
            if let Some((network_in, network_out)) = previous {
                previous_counters.insert("network_in", network_in as f64);
                previous_counters.insert("network_out", network_out as f64);
            }
        }
        let mut stmt = conn.prepare(
            "SELECT ts, online4, online6, cpu, load_1, load_5, load_15,
                    memory_used, memory_total, swap_used, swap_total, hdd_used, hdd_total,
                    tcp, udp, process, thread, network_in, network_out, network_rx, network_tx,
                    ping_10010, ping_189, ping_10086, time_10010, time_189, time_10086
             FROM metric_samples
             WHERE host_id = ?1 AND (?4 IS NULL OR gid = ?4) AND ts >= ?2 AND ts <= ?3
             ORDER BY ts ASC
             LIMIT 100000",
        )?;
        let rows = stmt.query_map(params![query.host.as_str(), from, to, query.gid.as_deref()], |row| {
            let mut values = HashMap::new();
            values.insert("online4", if row.get::<_, bool>(1)? { 1.0 } else { 0.0 });
            values.insert("online6", if row.get::<_, bool>(2)? { 1.0 } else { 0.0 });
            values.insert("cpu", row.get(3)?);
            values.insert("load_1", row.get(4)?);
            values.insert("load_5", row.get(5)?);
            values.insert("load_15", row.get(6)?);
            values.insert("memory_used", row.get::<_, u64>(7)? as f64);
            values.insert("memory_total", row.get::<_, u64>(8)? as f64);
            values.insert("swap_used", row.get::<_, u64>(9)? as f64);
            values.insert("swap_total", row.get::<_, u64>(10)? as f64);
            values.insert("hdd_used", row.get::<_, u64>(11)? as f64);
            values.insert("hdd_total", row.get::<_, u64>(12)? as f64);
            values.insert("tcp", row.get::<_, u32>(13)? as f64);
            values.insert("udp", row.get::<_, u32>(14)? as f64);
            values.insert("process", row.get::<_, u32>(15)? as f64);
            values.insert("thread", row.get::<_, u32>(16)? as f64);
            values.insert("network_in", row.get::<_, u64>(17)? as f64);
            values.insert("network_out", row.get::<_, u64>(18)? as f64);
            values.insert("network_rx", row.get::<_, u64>(19)? as f64);
            values.insert("network_tx", row.get::<_, u64>(20)? as f64);
            values.insert("ping_10010", row.get(21)?);
            values.insert("ping_189", row.get(22)?);
            values.insert("ping_10086", row.get(23)?);
            values.insert("time_10010", row.get(24)?);
            values.insert("time_189", row.get(25)?);
            values.insert("time_10086", row.get(26)?);
            Ok((row.get::<_, u64>(0)?, values))
        })?;

        let mut series: HashMap<String, Vec<Value>> = metrics.iter().map(|m| (m.to_string(), Vec::new())).collect();
        for row in rows {
            let (ts, values) = row?;
            let bucket = ts.checked_div(step).map_or(ts, |quotient| quotient * step);
            for metric in &metrics {
                let source_metric = match *metric {
                    "traffic_in" => "network_in",
                    "traffic_out" => "network_out",
                    value => value,
                };
                if let Some(value) = values.get(source_metric) {
                    let value = if *metric == "traffic_in" || *metric == "traffic_out" {
                        let previous = previous_counters.insert(source_metric, *value);
                        previous.map_or(0.0, |previous| (*value - previous).max(0.0))
                    } else {
                        *value
                    };
                    let points = series.get_mut(*metric).expect("metric was initialized");
                    let merge_with_last = step > 0
                        && points.last().and_then(|point| point.get(0)).and_then(Value::as_u64) == Some(bucket);
                    if merge_with_last {
                        let last = points.last_mut().expect("last point exists");
                        if *metric == "traffic_in" || *metric == "traffic_out" {
                            let accumulated = last.get(1).and_then(Value::as_f64).unwrap_or(0.0) + value;
                            *last = json!([bucket, accumulated]);
                        } else {
                            // Gauges and raw counters use the latest value in
                            // the requested response bucket.
                            *last = json!([bucket, value]);
                        }
                    } else {
                        points.push(json!([bucket, value]));
                    }
                }
            }
        }
        Ok(json!({
            "host": query.host,
            "from": from,
            "to": to,
            "step": step,
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

    #[test]
    fn sqlite_query_handles_counter_reset_and_response_buckets() {
        let store = SqliteHistoryStore::open(":memory:").unwrap();
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
            })
            .unwrap();

        assert_eq!(result["series"]["cpu"], json!([[100, 20.0], [120, 30.0]]));
        assert_eq!(result["series"]["traffic_in"], json!([[100, 500.0], [120, 0.0]]));

        let result = store
            .query(&HistoryQuery {
                host: "host-1".to_string(),
                gid: None,
                from: Some(105),
                to: Some(115),
                step: Some(0),
                metrics: Some("traffic_in".to_string()),
            })
            .unwrap();
        assert_eq!(result["series"]["traffic_in"], json!([[110, 500.0]]));
    }

    #[test]
    fn cleanup_deletes_in_batches() {
        let store = SqliteHistoryStore::open(":memory:").unwrap();
        store.write(&point(100, 10.0, 1000)).unwrap();
        store.write(&point(200, 20.0, 2000)).unwrap();
        assert_eq!(store.delete_before(300, 1).unwrap(), 1);
        assert_eq!(store.delete_before(300, 10).unwrap(), 1);
        assert_eq!(store.delete_before(300, 10).unwrap(), 0);
    }
}
