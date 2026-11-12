#![allow(unused)]
use anyhow::Result;
use chrono::{Datelike, Local, Timelike};
use once_cell::sync::OnceCell;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, sync_channel, Sender, SyncSender, TrySendError};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread;
use std::time::Duration;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::alerts::AlertManager;
use crate::config::{Host, HostGroup};
use crate::history::HistoryService;
use crate::notifier::{AlertDispatch, Event, Notifier, NotifyOutcome};
use crate::payload::{HostStat, StatsResp};

const SAVE_INTERVAL: u64 = 60;
const REPORT_QUEUE_CAPACITY: usize = 512;
const NOTIFIER_QUEUE_WARN_DEPTH: usize = 256;
const BOTTLENECK_WARN_MS: u128 = 1_000;
const BOTTLENECK_WARN_INTERVAL_MS: u64 = 10_000;
const OS_LIST: [&str; 10] = [
    "centos", "debian", "ubuntu", "arch", "windows", "macos", "pi", "android", "linux", "freebsd",
];

static STAT_SENDER: OnceCell<SyncSender<Cow<HostStat>>> = OnceCell::new();
static REPORT_QUEUE_DELAY_WARNED_AT: AtomicU64 = AtomicU64::new(0);
static REPORT_QUEUE_FULL_WARNED_AT: AtomicU64 = AtomicU64::new(0);
static STATE_LOCK_WARNED_AT: AtomicU64 = AtomicU64::new(0);
static REPORT_PROCESSING_WARNED_AT: AtomicU64 = AtomicU64::new(0);
static NOTIFIER_QUEUE_WARNED_AT: AtomicU64 = AtomicU64::new(0);
static TIMER_CYCLE_WARNED_AT: AtomicU64 = AtomicU64::new(0);

type NotifyMessage = (Event, Arc<HostStat>, AlertDispatch);

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(u64::MAX)
}

fn bottleneck_warning_allowed(last_warned_at: &AtomicU64) -> bool {
    let now = unix_now_ms();
    let previous = last_warned_at.load(Ordering::Relaxed);
    if now.saturating_sub(previous) < BOTTLENECK_WARN_INTERVAL_MS {
        return false;
    }
    last_warned_at
        .compare_exchange(previous, now, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
}

fn enqueue_notification(
    sender: &Sender<NotifyMessage>,
    queue_depth: &AtomicUsize,
    source: &str,
    event: Event,
    stat: Arc<HostStat>,
    dispatch: AlertDispatch,
) -> std::result::Result<usize, &'static str> {
    let depth = queue_depth.fetch_add(1, Ordering::Relaxed).saturating_add(1);
    if depth >= NOTIFIER_QUEUE_WARN_DEPTH && bottleneck_warning_allowed(&NOTIFIER_QUEUE_WARNED_AT) {
        warn!(
            "notifier queue backlog source={} event={event:?} host={} depth={} threshold={}",
            source, stat.name, depth, NOTIFIER_QUEUE_WARN_DEPTH
        );
    }
    match sender.send((event, stat, dispatch)) {
        Ok(()) => Ok(depth),
        Err(_) => {
            queue_depth.fetch_sub(1, Ordering::Relaxed);
            Err("notification receiver disconnected")
        }
    }
}

fn effective_last_received(processed_at: u64, ingress_at: Option<u64>) -> u64 {
    ingress_at.unwrap_or(processed_at).max(processed_at)
}

fn is_report_offline(last_received_at: u64, now: u64, offline_threshold: u64) -> bool {
    last_received_at.saturating_add(offline_threshold) < now
}

fn configured_host_stat(host: &Host) -> HostStat {
    HostStat {
        name: host.name.clone(),
        alias: host.alias.clone(),
        host_type: host.r#type.clone(),
        location: host.location.clone(),
        // A configured host that has not reported since this server process
        // started must be visible as offline, but must not trigger a false
        // NodeDown notification during startup.
        notify: false,
        online4: false,
        online6: false,
        labels: host.labels.clone(),
        tags: host.tags.clone(),
        gid: host.gid.clone(),
        weight: host.weight,
        pos: host.pos,
        disabled: host.disabled,
        ..HostStat::default()
    }
}

fn should_keep_runtime_host(
    configured_hosts: &HashMap<String, Host>,
    name: &str,
    latest_ts: u64,
    now: u64,
    group_gc: u64,
) -> bool {
    configured_hosts.contains_key(name) || latest_ts.saturating_add(group_gc) >= now
}

fn should_notify_up(
    previous_latest_ts: u64,
    latest_ts: u64,
    notify: bool,
    offline_threshold: u64,
    was_confirmed_offline: bool,
) -> bool {
    // A delayed report alone is not a recovery. The timer thread must have
    // confirmed this host as offline and emitted NodeDown first.
    notify
        && was_confirmed_offline
        && previous_latest_ts != 0
        && previous_latest_ts.saturating_add(offline_threshold) < latest_ts
}

fn prepare_report_host(
    configured_hosts: &HashMap<String, Host>,
    configured_groups: &HashMap<String, HostGroup>,
    runtime_hosts: &mut HashMap<String, Host>,
    stat: &mut HostStat,
) -> bool {
    // A fixed host's identity and directory are owned by the server config.
    // In particular, ignore gid sent by legacy/custom Agents for that host.
    if configured_hosts.contains_key(&stat.name) {
        return true;
    }

    if stat.gid.is_empty() {
        // Preserve the legacy-agent behavior: once a dynamic host has been
        // registered, subsequent reports that omit gid still update it.
        return runtime_hosts.contains_key(&stat.name);
    }
    if stat.alias.is_empty() {
        stat.alias.clone_from(&stat.name);
    }

    let previous = runtime_hosts.get(&stat.name);
    if previous.is_some_and(|host| host.gid == stat.gid) {
        return true;
    }
    let Some(group) = configured_groups.get(&stat.gid) else {
        return false;
    };
    let mut instance = group.inst_host(&stat.name);
    if let Some(previous) = previous {
        instance.last_network_in = previous.last_network_in;
        instance.last_network_out = previous.last_network_out;
    }
    runtime_hosts.insert(stat.name.clone(), instance);
    true
}

pub struct StatsMgr {
    resp_json: Arc<Mutex<String>>,
    stats_data: Arc<Mutex<StatsResp>>,
    /// Liveness is based on reports accepted at the HTTP/gRPC boundary, not
    /// on when the single statistics worker eventually processes them.
    last_report_received: Arc<Mutex<HashMap<String, u64>>>,
    history: Option<Arc<HistoryService>>,
}

impl StatsMgr {
    pub fn new() -> Self {
        Self {
            resp_json: Arc::new(Mutex::new("{}".to_string())),
            stats_data: Arc::new(Mutex::new(StatsResp::new())),
            last_report_received: Arc::new(Mutex::new(HashMap::new())),
            history: None,
        }
    }

    fn load_last_network(hosts_map: &mut HashMap<String, Host>) {
        let contents = fs::read_to_string("stats.json").unwrap_or_default();
        if contents.is_empty() {
            return;
        }

        if let Ok(stats_json) = serde_json::from_str::<serde_json::Value>(contents.as_str()) {
            if let Some(servers) = stats_json["servers"].as_array() {
                for v in servers {
                    if let (Some(name), Some(last_network_in), Some(last_network_out)) = (
                        v["name"].as_str(),
                        v["last_network_in"].as_u64(),
                        v["last_network_out"].as_u64(),
                    ) {
                        if let Some(srv) = hosts_map.get_mut(name) {
                            srv.last_network_in = last_network_in;
                            srv.last_network_out = last_network_out;

                            trace!("{} => last in/out ({}/{}))", &name, last_network_in, last_network_out);
                        }
                    } else {
                        error!("invalid json => {v:?}");
                    }
                }
                trace!("load stats.json succ!");
            }
        } else {
            warn!("ignore invalid stats.json");
        }
    }

    #[allow(clippy::too_many_lines)]
    #[allow(clippy::unnecessary_wraps)]
    pub fn init(
        &mut self,
        cfg: &'static crate::config::Config,
        notifies: Arc<Mutex<Vec<Box<dyn Notifier + Send>>>>,
    ) -> Result<()> {
        self.history = HistoryService::new(&cfg.history)?;
        if self.history.is_some() {
            let tiers = cfg
                .history
                .tiers
                .iter()
                .map(|tier| format!("{}:{}", tier.resolution, tier.retention))
                .collect::<Vec<_>>()
                .join(",");
            info!(
                "history storage enabled: backend={}, path={}, tiers=[{}], allowed_lateness={}s, maintenance_interval={}s",
                cfg.history.backend,
                cfg.history.path,
                tiers,
                cfg.history.allowed_lateness,
                cfg.history.maintenance_interval
            );
        }
        let hosts_map_base = Arc::new(Mutex::new(cfg.hosts_map.clone()));

        // load last_network_in/out
        if let Ok(mut hosts_map_guard) = hosts_map_base.lock() {
            Self::load_last_network(&mut hosts_map_guard);
        }

        let (stat_tx, stat_rx) = sync_channel(REPORT_QUEUE_CAPACITY);
        STAT_SENDER.set(stat_tx).unwrap();
        let (notifier_tx, notifier_rx) = channel();
        let notifier_queue_depth = Arc::new(AtomicUsize::new(0));
        let alert_manager = cfg.alerts.enabled.then(|| AlertManager::new(&cfg.alerts));

        // Fixed hosts are part of the server configuration, not ephemeral
        // Agent sessions. Seed them as offline so they remain visible even if
        // the server restarts while an Agent is down or has never reported.
        let initial_stats = cfg
            .hosts_map
            .iter()
            .filter(|(_, host)| !host.disabled)
            .map(|(name, host)| (name.clone(), Arc::new(configured_host_stat(host))))
            .collect();
        let stat_map: Arc<Mutex<HashMap<String, Arc<HostStat>>>> = Arc::new(Mutex::new(initial_stats));

        // stat_rx thread
        thread::spawn({
            let hosts_map = hosts_map_base.clone();
            let stat_map = stat_map.clone();
            let notifier_tx = notifier_tx.clone();
            let notifier_queue_depth = notifier_queue_depth.clone();
            let history = self.history.clone();

            move || loop {
                while let Ok(mut stat) = stat_rx.recv() {
                    let report_processing_started = Instant::now();
                    trace!("recv stat `{stat:?}");

                    let stat_t = stat.to_mut();
                    let report_host = stat_t.name.clone();
                    let queue_delay_ms = stat_t
                        .received_ts_ms
                        .map(|received_at| unix_now_ms().saturating_sub(received_at));
                    let queue_delay_is_bottleneck =
                        queue_delay_ms.is_some_and(|delay| u128::from(delay) >= BOTTLENECK_WARN_MS);

                    // Resolve fixed-vs-dynamic identity before applying the
                    // report. This does not change the Agent wire protocol.
                    if let Ok(mut runtime_hosts) = hosts_map.lock() {
                        if !prepare_report_host(&cfg.hosts_map, &cfg.hosts_group_map, &mut runtime_hosts, stat_t) {
                            continue;
                        }
                    } else {
                        continue;
                    }

                    //
                    if let Ok(mut hosts_map) = hosts_map.lock() {
                        let host_info = hosts_map.get_mut(&stat_t.name);
                        if host_info.is_none() {
                            error!("invalid stat `{stat_t:?}");
                            continue;
                        }
                        let info = host_info.unwrap();

                        if info.disabled {
                            continue;
                        }

                        // 补齐
                        if stat_t.location.is_empty() {
                            stat_t.location = info.location.clone();
                        }
                        if stat_t.host_type.is_empty() {
                            stat_t.host_type = info.r#type.clone();
                        }
                        stat_t.notify = info.notify && stat_t.notify;
                        stat_t.pos = info.pos;
                        stat_t.disabled = info.disabled;
                        stat_t.weight += info.weight;
                        // Fixed hosts may also belong to a directory. The
                        // directory is server-managed and must be exposed in
                        // the JSON response even when the agent does not send
                        // a group id.
                        stat_t.gid.clone_from(&info.gid);
                        stat_t.labels = info.labels.clone();
                        stat_t.tags.clone_from(&info.tags);

                        // !group
                        if !info.alias.is_empty() {
                            stat_t.alias = info.alias.clone();
                        }

                        let received_at = stat_t
                            .received_ts
                            .unwrap_or_else(|| SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs());
                        let agent_ts = stat_t.agent_ts;
                        info.latest_ts = received_at;
                        stat_t.latest_ts = received_at;

                        // last_network_in/out
                        if !stat_t.vnstat {
                            let local_now = Local::now();
                            if info.last_network_in == 0
                                || (stat_t.network_in != 0 && info.last_network_in > stat_t.network_in)
                                || (local_now.day() == info.monthstart
                                    && local_now.hour() == 0
                                    && local_now.minute() < 5)
                            {
                                info.last_network_in = stat_t.network_in;
                                info.last_network_out = stat_t.network_out;
                            } else {
                                stat_t.last_network_in = info.last_network_in;
                                stat_t.last_network_out = info.last_network_out;
                            }
                        }

                        // uptime str
                        let day = stat_t.uptime / (3600 * 24);
                        if day > 0 {
                            stat_t.uptime_str = format!("{day} 天");
                        } else {
                            stat_t.uptime_str = format!(
                                "{:02}:{:02}:{:02}",
                                stat_t.uptime / 3600,
                                (stat_t.uptime / 60) % 60,
                                stat_t.uptime % 60
                            );
                        }

                        debug!("update stat `{stat_t:?}");
                        let state_lock_started = Instant::now();
                        if let Ok(mut host_stat_map) = stat_map.lock() {
                            let state_lock_wait_ms = state_lock_started.elapsed().as_millis();
                            if state_lock_wait_ms >= BOTTLENECK_WARN_MS
                                && bottleneck_warning_allowed(&STATE_LOCK_WARNED_AT)
                            {
                                warn!(
                                    "stats state lock contention source=report_worker host={} wait_ms={} threshold_ms={}",
                                    report_host,
                                    state_lock_wait_ms,
                                    BOTTLENECK_WARN_MS
                                );
                            }
                            let previous_stat = host_stat_map.get(&stat_t.name);
                            let previous_received_at = previous_stat.map(|value| value.latest_ts).unwrap_or_default();
                            let previous_agent_ts = previous_stat.and_then(|value| value.agent_ts);
                            let receive_gap_seconds = (previous_received_at != 0)
                                .then(|| received_at.saturating_sub(previous_received_at));
                            let agent_gap_seconds = agent_ts
                                .zip(previous_agent_ts)
                                .map(|(current, previous)| current as i128 - previous as i128);
                            let agent_delay_seconds =
                                agent_ts.map(|timestamp| received_at as i128 - timestamp as i128);
                            let timing_level = if queue_delay_is_bottleneck
                                && bottleneck_warning_allowed(&REPORT_QUEUE_DELAY_WARNED_AT)
                            {
                                log::Level::Warn
                            } else if receive_gap_seconds.is_some_and(|gap| {
                                gap >= cfg.offline_threshold.saturating_div(2).max(1)
                            }) {
                                log::Level::Info
                            } else {
                                log::Level::Debug
                            };
                            log::log!(
                                timing_level,
                                "agent report timing host={} gid={} received_at={} agent_ts={} receive_gap_seconds={} agent_gap_seconds={} agent_delay_seconds={} queue_delay_ms={} timestamp_source={}",
                                stat_t.name,
                                stat_t.gid,
                                received_at,
                                agent_ts.map_or_else(|| "-".to_string(), |value| value.to_string()),
                                receive_gap_seconds.map_or_else(|| "-".to_string(), |value| value.to_string()),
                                agent_gap_seconds.map_or_else(|| "-".to_string(), |value| value.to_string()),
                                agent_delay_seconds.map_or_else(|| "-".to_string(), |value| value.to_string()),
                                queue_delay_ms.map_or_else(|| "-".to_string(), |value| value.to_string()),
                                if agent_ts.is_some() { "agent" } else { "server" },
                            );
                            let mut notify_up = false;
                            if let Some(pre_stat) = host_stat_map.get(&stat_t.name) {
                                if stat_t.ip_info.is_none() {
                                    stat_t.ip_info = pre_stat.ip_info.clone();
                                }

                                let previous_latest_ts = pre_stat.latest_ts;
                                let report_gap = stat_t.latest_ts.saturating_sub(previous_latest_ts);
                                let crossed_offline_threshold = previous_latest_ts != 0
                                    && previous_latest_ts.saturating_add(cfg.offline_threshold)
                                        < stat_t.latest_ts;
                                if crossed_offline_threshold {
                                    let decision = if !stat_t.notify {
                                        "suppressed_notify_disabled"
                                    } else if pre_stat.offline {
                                        "send"
                                    } else {
                                        "suppressed_no_prior_node_down"
                                    };
                                    info!(
                                        "node up candidate host={} previous_latest_ts={} received_at={} gap_seconds={} offline_threshold={} notify={} previous_offline={} previous_online4={} previous_online6={} decision={}",
                                        stat_t.name,
                                        previous_latest_ts,
                                        stat_t.latest_ts,
                                        report_gap,
                                        cfg.offline_threshold,
                                        stat_t.notify,
                                        pre_stat.offline,
                                        pre_stat.online4,
                                        pre_stat.online6,
                                        decision
                                    );
                                }
                                if should_notify_up(
                                    pre_stat.latest_ts,
                                    stat_t.latest_ts,
                                    stat_t.notify,
                                    cfg.offline_threshold,
                                    pre_stat.offline,
                                ) {
                                    notify_up = true;
                                }
                            }
                            if notify_up {
                                stat.to_mut().offline = false;
                            }
                            let arc_stat = Arc::new(stat.into_owned());
                            if let Some(history) = history.as_ref() {
                                history.record(&arc_stat);
                            }
                            if notify_up {
                                // node up notify
                                info!(
                                    "alert event detected event=NodeUp host={} gid={} route_keys=[node_up]",
                                    arc_stat.name,
                                    arc_stat.gid
                                );
                                let send_result = enqueue_notification(
                                    &notifier_tx,
                                    &notifier_queue_depth,
                                    "report_worker",
                                    Event::NodeUp,
                                    Arc::clone(&arc_stat),
                                    AlertDispatch {
                                        route_keys: vec!["node_up".to_string()],
                                        ..AlertDispatch::default()
                                    },
                                );
                                if let Err(error) = send_result {
                                    error!(
                                        "alert event queue failed event=NodeUp host={} error={error}",
                                        arc_stat.name
                                    );
                                } else {
                                    info!(
                                        "alert delivery queue=accepted event=NodeUp host={}",
                                        arc_stat.name
                                    );
                                }
                            }
                            host_stat_map.insert(arc_stat.name.clone(), arc_stat);
                            //trace!("{:?}", host_stat_map);
                        }
                    }

                    let processing_ms = report_processing_started.elapsed().as_millis();
                    if processing_ms >= BOTTLENECK_WARN_MS
                        && bottleneck_warning_allowed(&REPORT_PROCESSING_WARNED_AT)
                    {
                        warn!(
                            "report processing slow host={} duration_ms={} threshold_ms={}",
                            report_host,
                            processing_ms,
                            BOTTLENECK_WARN_MS
                        );
                    }
                }
            }
        });

        // timer thread
        thread::spawn({
            let resp_json = self.resp_json.clone();
            let stats_data = self.stats_data.clone();
            let hosts_map = hosts_map_base.clone();
            let stat_map = stat_map.clone();
            let last_report_received = self.last_report_received.clone();
            let notifier_tx = notifier_tx.clone();
            let notifier_queue_depth = notifier_queue_depth.clone();
            let history = self.history.clone();
            let mut alert_manager = alert_manager;
            let mut latest_save_ts = 0_u64;
            let mut latest_group_gc = 0_u64;
            let mut latest_alert_check_ts = 0_u64;
            move || loop {
                thread::sleep(Duration::from_millis(500));
                let timer_cycle_started = Instant::now();

                let mut resp = StatsResp::new();
                let now = resp.updated;
                let ingress_received = last_report_received
                    .lock()
                    .map(|received| received.clone())
                    .unwrap_or_default();

                // group gc
                if latest_group_gc + cfg.group_gc < now {
                    latest_group_gc = now;
                    //
                    if let Ok(mut hm) = hosts_map.lock() {
                        hm.retain(|name, o| {
                            should_keep_runtime_host(&cfg.hosts_map, name, o.latest_ts, now, cfg.group_gc)
                        });
                    }
                    //
                    if let Ok(mut sm) = stat_map.lock() {
                        sm.retain(|name, o| {
                            should_keep_runtime_host(&cfg.hosts_map, name, o.latest_ts, now, cfg.group_gc)
                        });
                    }
                    if let Ok(mut received) = last_report_received.lock() {
                        received.retain(|name, latest_ts| {
                            should_keep_runtime_host(&cfg.hosts_map, name, *latest_ts, now, cfg.group_gc)
                        });
                    }
                }

                // Resource evaluation may read/write SQLite alert state. Keep
                // it outside the global host-state lock so incoming reports
                // can update liveness while alerts are being evaluated.
                let mut pending_custom_checks = Vec::new();
                let state_lock_started = Instant::now();
                if let Ok(mut host_stat_map) = stat_map.lock() {
                    let state_lock_wait_ms = state_lock_started.elapsed().as_millis();
                    if state_lock_wait_ms >= BOTTLENECK_WARN_MS
                        && bottleneck_warning_allowed(&STATE_LOCK_WARNED_AT)
                    {
                        warn!(
                            "stats state lock contention source=timer wait_ms={} threshold_ms={}",
                            state_lock_wait_ms,
                            BOTTLENECK_WARN_MS
                        );
                    }
                    for (_, stat) in host_stat_map.iter_mut() {
                        if stat.disabled {
                            resp.servers.push(Arc::clone(stat));
                            continue;
                        }
                        let mut became_offline = false;
                        let notify_event = {
                            let o = Arc::make_mut(stat);
                            // 30s 下线
                            let previous_latest_ts =
                                effective_last_received(o.latest_ts, ingress_received.get(&o.name).copied());
                            if is_report_offline(previous_latest_ts, now, cfg.offline_threshold) {
                                became_offline = o.online4 || o.online6;
                                if became_offline && cfg.alerts.offline_log {
                                    info!(
                                        "offline candidate host={} gid={} last_received_at={} gap_seconds={} offline_threshold={} online4={} online6={}",
                                        o.name,
                                        o.gid,
                                        previous_latest_ts,
                                        now.saturating_sub(previous_latest_ts),
                                        cfg.offline_threshold,
                                        o.online4,
                                        o.online6
                                    );
                                }
                                o.online4 = false;
                                o.online6 = false;
                            }

                            // labels
                            if !o.labels.contains("os=") {
                                if let Some(sys_info) = &o.sys_info {
                                    let os_r = sys_info.os_release.to_lowercase();
                                    for s in &OS_LIST {
                                        if os_r.contains(s) {
                                            if o.labels.is_empty() {
                                                write!(o.labels, "os={s}");
                                            } else {
                                                write!(o.labels, ";os={s}");
                                            }
                                            break;
                                        }
                                    }
                                }
                            }

                            // A confirmed online -> offline transition must
                            // produce NodeDown immediately. It must not wait
                            // for the periodic Custom check interval.
                            if o.notify && became_offline {
                                // Mark the transition before queueing the
                                // event. A later report can only generate
                                // NodeUp after this confirmed transition.
                                o.offline = true;
                                o.disabled = true;
                                if cfg.alerts.offline_log {
                                    info!(
                                        "node down host={} gid={} last_received_at={} gap_seconds={} offline_threshold={}",
                                        o.name,
                                        o.gid,
                                        previous_latest_ts,
                                        now.saturating_sub(previous_latest_ts),
                                        cfg.offline_threshold
                                    );
                                }
                                Some(Event::NodeDown)
                            } else if o.notify
                                && (o.online4 || o.online6)
                                && o.last_notify_ts + cfg.notify_interval < now
                            {
                                // Custom checks are throttled per host. A
                                // global timestamp would let another host's
                                // frequent Custom event delay this host.
                                o.last_notify_ts = now;
                                Some(Event::Custom)
                            } else {
                                None
                            }
                        };

                        if became_offline {
                            if let Some(history) = history.as_ref() {
                                let mut offline_stat = (**stat).clone();
                                offline_stat.latest_ts = now;
                                // This state transition is generated by the
                                // server, not delayed Agent event time.
                                offline_stat.agent_ts = None;
                                history.record(&offline_stat);
                            }
                        }

                        if let Some(event) = notify_event {
                            if matches!(&event, Event::Custom) {
                                pending_custom_checks.push(Arc::clone(stat));
                            } else {
                                let route_key = match event {
                                    Event::NodeUp => "node_up",
                                    Event::NodeDown => "node_down",
                                    Event::Custom => "custom",
                                    Event::AlertRecovered => "alert_recovered",
                                };
                                info!(
                                    "alert event detected event={event:?} host={} gid={}",
                                    stat.name,
                                    stat.gid
                                );
                                let send_result = enqueue_notification(
                                    &notifier_tx,
                                    &notifier_queue_depth,
                                    "timer",
                                    event.clone(),
                                    Arc::clone(stat),
                                    AlertDispatch {
                                        route_keys: vec![route_key.to_string()],
                                        ..AlertDispatch::default()
                                    },
                                );
                                if let Err(error) = send_result {
                                    error!(
                                        "alert delivery queue_failed event={event:?} host={} error={error}",
                                        stat.name
                                    );
                                } else {
                                    info!(
                                        "alert delivery queue=accepted event={event:?} host={}",
                                        stat.name
                                    );
                                }
                            }
                        }

                        resp.servers.push(Arc::clone(stat));
                    }
                }

                for stat in pending_custom_checks {
                    let mut deliveries = Vec::new();
                    match alert_manager.as_mut() {
                        Some(manager) => {
                            let evaluation = manager.evaluate_custom(&stat, now);
                            if !evaluation.recovered.is_empty() {
                                let mut route_keys = evaluation
                                    .recovered
                                    .iter()
                                    .map(|item| recovery_route_key(&item.alert_type))
                                    .collect::<Vec<_>>();
                                route_keys.push("alert_recovered".to_string());
                                deliveries.push((
                                    Event::AlertRecovered,
                                    AlertDispatch {
                                        route_keys,
                                        recovered: evaluation.recovered,
                                        ..AlertDispatch::default()
                                    },
                                ));
                            }
                            if evaluation.should_send {
                                let route_keys = if evaluation.firing.is_empty() {
                                    vec!["custom".to_string()]
                                } else {
                                    evaluation
                                        .firing
                                        .iter()
                                        .map(|item| item.alert_type.clone())
                                        .collect()
                                };
                                deliveries.push((
                                    Event::Custom,
                                    AlertDispatch {
                                        route_keys,
                                        firing: evaluation.firing,
                                        suppressed_rule_ids: evaluation.suppressed_rule_ids,
                                        ..AlertDispatch::default()
                                    },
                                ));
                            }
                        }
                        None => deliveries.push((
                            Event::Custom,
                            AlertDispatch {
                                route_keys: vec!["custom".to_string()],
                                ..AlertDispatch::default()
                            },
                        )),
                    }
                    for (delivery_event, dispatch) in deliveries {
                        info!(
                            "alert event detected event={delivery_event:?} host={} gid={}",
                            stat.name, stat.gid
                        );
                        let send_result = enqueue_notification(
                            &notifier_tx,
                            &notifier_queue_depth,
                            "timer",
                            delivery_event.clone(),
                            Arc::clone(&stat),
                            dispatch,
                        );
                        if let Err(error) = send_result {
                            error!(
                                "alert delivery queue_failed event={delivery_event:?} host={} error={error}",
                                stat.name
                            );
                        } else {
                            info!(
                                "alert delivery queue=accepted event={delivery_event:?} host={}",
                                stat.name
                            );
                        }
                    }
                }

                resp.servers.sort_by(|a, b| {
                    if a.weight != b.weight {
                        return a.weight.cmp(&b.weight).reverse();
                    }
                    if a.pos != b.pos {
                        return a.pos.cmp(&b.pos);
                    }
                    // same group
                    a.alias.cmp(&b.alias)
                });

                // last_network_in/out save /60s
                if latest_save_ts + SAVE_INTERVAL < now {
                    latest_save_ts = now;
                    if !resp.servers.is_empty() {
                        if let Ok(mut file) = File::create("stats.json") {
                            file.write_all(serde_json::to_string(&resp).unwrap().as_bytes());
                            file.flush();
                            trace!("save stats.json succ!");
                        } else {
                            error!("save stats.json fail!");
                        }
                    }
                }
                //
                if let Ok(mut o) = resp_json.lock() {
                    *o = serde_json::to_string(&resp).unwrap();
                }
                if let Ok(mut o) = stats_data.lock() {
                    *o = resp;
                }

                let timer_cycle_ms = timer_cycle_started.elapsed().as_millis();
                if timer_cycle_ms >= BOTTLENECK_WARN_MS
                    && bottleneck_warning_allowed(&TIMER_CYCLE_WARNED_AT)
                {
                    warn!(
                        "stats timer cycle slow duration_ms={} threshold_ms={} host_count={}",
                        timer_cycle_ms,
                        BOTTLENECK_WARN_MS,
                        stats_data.lock().map(|data| data.servers.len()).unwrap_or_default()
                    );
                }
            }
        });

        // notify thread
        thread::spawn(move || loop {
            while let Ok(msg) = notifier_rx.recv() {
                notifier_queue_depth.fetch_sub(1, Ordering::Relaxed);
                let (e, stat, dispatch) = msg;
                let notify_list = &*notifies.lock().unwrap();
                trace!("recv notify => {e:?}, {stat:?}");
                if notify_list.is_empty() {
                    if cfg.alerts.log_delivery {
                        info!(
                            "alert delivery skipped event={e:?} host={} reason=no_enabled_notifier route_keys={:?}",
                            stat.name,
                            dispatch.route_keys
                        );
                    }
                    continue;
                }
                for n in notify_list {
                    let selected_route_keys = cfg.alerts.allowed_event_types(&dispatch.route_keys, n.kind());
                    if selected_route_keys.is_empty() {
                        if cfg.alerts.log_delivery {
                            info!(
                                "alert delivery skipped notifier={} event={e:?} host={} reason=route_not_selected route_keys={:?}",
                                n.kind(),
                                stat.name,
                                dispatch.route_keys
                            );
                        }
                        continue;
                    }
                    let mut channel_dispatch = dispatch.clone();
                    channel_dispatch.route_keys = selected_route_keys.clone();
                    if cfg.alerts.log_delivery {
                        info!(
                            "alert channel selected notifier={} event={e:?} host={} route_keys={:?}",
                            n.kind(),
                            stat.name,
                            selected_route_keys
                        );
                    }
                    if matches!(e, Event::Custom) && !dispatch.firing.is_empty() {
                        let selected_firing = dispatch
                            .firing
                            .iter()
                            .filter(|item| selected_route_keys.iter().any(|key| key == &item.alert_type))
                            .cloned()
                            .collect::<Vec<_>>();
                        channel_dispatch.firing = selected_firing.clone();
                        channel_dispatch.allowed_rule_ids = Some(
                            selected_firing
                                .iter()
                                .map(|item| item.alert_type.clone())
                                .collect(),
                        );
                    }
                    if matches!(e, Event::AlertRecovered) && !dispatch.recovered.is_empty() {
                        channel_dispatch.recovered.retain(|item| {
                            selected_route_keys.iter().any(|key| key == "alert_recovered")
                                || selected_route_keys.iter().any(|key| key == &recovery_route_key(&item.alert_type))
                        });
                        if channel_dispatch.recovered.is_empty() {
                            continue;
                        }
                    }
                    if cfg.alerts.log_delivery {
                        info!(
                            "alert processing notifier={} event={e:?} host={} firing={} recoveries={} suppressed_rules={}",
                            n.kind(),
                            stat.name,
                            channel_dispatch.firing.len(),
                            channel_dispatch.recovered.len(),
                            channel_dispatch.suppressed_rule_ids.len()
                        );
                    }
                    trace!("{} notify {:?} => {:?}", n.kind(), e, stat);
                    match n.notify(&e, &stat, &channel_dispatch) {
                        Ok(NotifyOutcome::Queued) => {
                            if cfg.alerts.log_delivery {
                                info!(
                                    "alert delivery accepted notifier={} event={e:?} host={} status=queued",
                                    n.kind(),
                                    stat.name
                                );
                            }
                        }
                        Ok(NotifyOutcome::Skipped { reason }) => {
                            if cfg.alerts.log_delivery {
                                info!(
                                    "alert delivery skipped notifier={} event={e:?} host={} reason={reason}",
                                    n.kind(),
                                    stat.name
                                );
                            }
                        }
                        Err(error) => {
                            error!(
                                "alert delivery failed notifier={} event={e:?} host={} error={error:#}",
                                n.kind(),
                                stat.name
                            );
                        }
                    }
                }
            }
        });

        Ok(())
    }

    pub fn get_stats(&self) -> Arc<Mutex<StatsResp>> {
        self.stats_data.clone()
    }

    pub fn get_stats_json(&self) -> String {
        self.resp_json.lock().unwrap().to_string()
    }

    pub fn get_hosts_catalog(&self) -> serde_json::Value {
        let data = self.stats_data.lock().unwrap();
        build_hosts_catalog(&data)
    }

    pub fn get_hosts(&self, directory: Option<&str>, tags: &[String]) -> serde_json::Value {
        let data = self.stats_data.lock().unwrap();
        serde_json::to_value(filter_hosts(&data, directory, tags))
            .unwrap_or_else(|_| serde_json::json!({"updated": data.updated, "servers": []}))
    }

    pub fn get_host(&self, name: &str, gid: Option<&str>) -> serde_json::Value {
        let data = self.stats_data.lock().unwrap();
        serde_json::to_value(filter_host(&data, name, gid))
            .unwrap_or_else(|_| serde_json::json!({"updated": data.updated, "servers": []}))
    }

    pub fn get_history(&self) -> Option<Arc<HistoryService>> {
        self.history.clone()
    }

    #[allow(clippy::unused_self)]
    #[allow(clippy::unnecessary_wraps)]
    pub fn report(&self, mut data: serde_json::Value) -> Result<()> {
        static SENDER: LazyLock<SyncSender<Cow<'static, HostStat>>> =
            LazyLock::new(|| STAT_SENDER.get().unwrap().clone());

        // Capture the server-side receive time before the bounded queue. This
        // distinguishes network/Agent gaps from time spent waiting for the
        // statistics worker and does not alter the Agent wire protocol.
        if let Some(object) = data.as_object_mut() {
            let received_at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
            let received_at_ms = unix_now_ms();
            object.insert("_received_ts".to_string(), serde_json::json!(received_at));
            object.insert("_received_ts_ms".to_string(), serde_json::json!(received_at_ms));
        }
        match serde_json::from_value::<HostStat>(data) {
            Ok(stat) => {
                let report_host = stat.name.clone();
                let received_at = stat.received_ts.unwrap_or_else(|| unix_now_ms() / 1_000);
                if let Ok(mut last_report_received) = self.last_report_received.lock() {
                    last_report_received.insert(report_host.clone(), received_at);
                }
                trace!("send stat => {stat:?} ");
                match SENDER.try_send(Cow::Owned(stat)) {
                    Ok(()) => {}
                    Err(TrySendError::Full(_)) => {
                        if bottleneck_warning_allowed(&REPORT_QUEUE_FULL_WARNED_AT) {
                            warn!(
                                "report queue full host={} capacity={} action=drop_metrics_keep_liveness warning_interval_ms={}",
                                report_host, REPORT_QUEUE_CAPACITY, BOTTLENECK_WARN_INTERVAL_MS
                            );
                        }
                        anyhow::bail!("stat queue is full");
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        error!("report queue unavailable host={} reason=disconnected", report_host);
                        anyhow::bail!("stat queue is unavailable");
                    }
                }
            }
            Err(err) => {
                error!("report error => {err:?}");
                anyhow::bail!("invalid report payload: {err}");
            }
        }
        Ok(())
    }

    pub fn get_all_info(&self) -> Result<serde_json::Value> {
        let data = self.stats_data.lock().unwrap();
        let mut resp_json = serde_json::to_value(&*data)?;
        // for skip_serializing
        if let Some(srv_list) = resp_json["servers"].as_array_mut() {
            for (idx, stat) in data.servers.iter().enumerate() {
                if let Some(srv) = srv_list[idx].as_object_mut() {
                    srv.insert("ip_info".into(), serde_json::to_value(stat.ip_info.as_ref())?);
                    srv.insert("sys_info".into(), serde_json::to_value(stat.sys_info.as_ref())?);
                }
            }
        } else {
            // todo!()
        }

        Ok(resp_json)
    }
}

fn recovery_route_key(alert_type: &str) -> String {
    format!(
        "{}_recovered",
        alert_type.strip_suffix("_high").unwrap_or(alert_type)
    )
}

fn filter_hosts(data: &StatsResp, directory: Option<&str>, tags: &[String]) -> StatsResp {
    let normalized_tags = tags
        .iter()
        .map(|tag| tag.trim().to_lowercase())
        .filter(|tag| !tag.is_empty())
        .collect::<HashSet<_>>();
    let normalized_directory = directory.map(str::trim).filter(|value| !value.is_empty());
    // A tag-only query searches all directories. An empty query intentionally
    // stays scoped to `default` so the home page cannot load every host by
    // accident. `directory=all` is the explicit all-hosts form.
    let directory = normalized_directory.unwrap_or(if normalized_tags.is_empty() { "default" } else { "all" });
    let servers = data
        .servers
        .iter()
        .filter(|server| {
            let directory_matches = if directory.eq_ignore_ascii_case("all") {
                true
            } else if directory.eq_ignore_ascii_case("default") {
                server.gid.trim().is_empty()
            } else {
                server.gid.eq_ignore_ascii_case(directory)
            };
            let tags_match = normalized_tags.is_empty()
                || server
                    .tags
                    .iter()
                    .any(|tag| normalized_tags.contains(&tag.trim().to_lowercase()));
            directory_matches && tags_match
        })
        .cloned()
        .collect();
    StatsResp {
        updated: data.updated,
        servers,
    }
}

fn filter_host(data: &StatsResp, name: &str, gid: Option<&str>) -> StatsResp {
    let name = name.trim();
    let gid = gid.map(str::trim);
    let servers = data
        .servers
        .iter()
        .filter(|server| server.name == name && gid.is_none_or(|expected| server.gid == expected))
        .cloned()
        .collect();
    StatsResp {
        updated: data.updated,
        servers,
    }
}

fn build_hosts_catalog(data: &StatsResp) -> serde_json::Value {
    let mut default_count = 0_usize;
    let mut directories = BTreeMap::<String, usize>::new();
    let mut tags = BTreeMap::<String, (String, usize)>::new();
    for server in &data.servers {
        let gid = server.gid.trim();
        if gid.is_empty() {
            default_count += 1;
        } else {
            *directories.entry(gid.to_string()).or_default() += 1;
        }
        let mut host_tags = HashSet::new();
        for tag in &server.tags {
            let normalized = tag.trim().to_lowercase();
            if normalized.is_empty() || !host_tags.insert(normalized.clone()) {
                continue;
            }
            let entry = tags.entry(normalized).or_insert_with(|| (tag.trim().to_string(), 0));
            entry.1 += 1;
        }
    }
    serde_json::json!({
        "default": {"id": "default", "name": "默认", "count": default_count},
        "directories": directories.into_iter().map(|(id, count)| {
            serde_json::json!({"id": id, "name": id, "count": count})
        }).collect::<Vec<_>>(),
        "tags": tags.into_values().map(|(name, count)| {
            serde_json::json!({"name": name, "count": count})
        }).collect::<Vec<_>>()
    })
}

#[cfg(test)]
mod host_query_tests {
    use super::*;

    fn configured(name: &str, gid: &str) -> Host {
        Host {
            name: name.to_string(),
            password: "secret".to_string(),
            gid: gid.to_string(),
            ..Host::default()
        }
    }

    fn stat(name: &str, gid: &str, tags: &[&str]) -> Arc<HostStat> {
        Arc::new(HostStat {
            name: name.to_string(),
            gid: gid.to_string(),
            tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
            ..HostStat::default()
        })
    }

    fn data() -> StatsResp {
        StatsResp {
            updated: 42,
            servers: vec![
                stat("default-1", "", &["AWS", "MYSQL"]),
                stat("fintech-1", "FINTECH", &["AWS"]),
                stat("mail-1", "MAIL", &["MAILSVR"]),
            ],
        }
    }

    #[test]
    fn no_query_and_default_return_only_hosts_without_gid() {
        let result = filter_hosts(&data(), None, &[]);
        assert_eq!(result.servers.len(), 1);
        assert_eq!(result.servers[0].name, "default-1");
    }

    #[test]
    fn directory_query_is_unique_and_tag_only_query_uses_all_directories() {
        let by_directory = filter_hosts(&data(), Some("FINTECH"), &[]);
        assert_eq!(by_directory.servers[0].name, "fintech-1");

        let tags = vec!["mysql".to_string(), "MAILSVR".to_string()];
        let by_tags = filter_hosts(&data(), None, &tags);
        let names = by_tags
            .servers
            .iter()
            .map(|server| server.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["default-1", "mail-1"]);
    }

    #[test]
    fn all_directories_and_tags_use_and_or_semantics() {
        let tags = vec!["mysql".to_string(), "MAILSVR".to_string()];
        let result = filter_hosts(&data(), Some("all"), &tags);
        let names = result
            .servers
            .iter()
            .map(|server| server.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["default-1", "mail-1"]);
    }

    #[test]
    fn named_directory_and_tags_are_combined_with_and() {
        let tags = vec!["AWS".to_string(), "MAILSVR".to_string()];
        let result = filter_hosts(&data(), Some("FINTECH"), &tags);
        assert_eq!(result.servers.len(), 1);
        assert_eq!(result.servers[0].name, "fintech-1");

        let no_match = filter_hosts(&data(), Some("FINTECH"), &["MYSQL".to_string(), "MAILSVR".to_string()]);
        assert!(no_match.servers.is_empty());
    }

    #[test]
    fn default_directory_and_tags_are_combined_with_and() {
        let tags = vec!["MYSQL".to_string(), "MAILSVR".to_string()];
        let result = filter_hosts(&data(), Some("default"), &tags);
        assert_eq!(result.servers.len(), 1);
        assert_eq!(result.servers[0].name, "default-1");
    }

    #[test]
    fn single_host_query_uses_optional_gid_and_ignores_catalog_filters() {
        let result = filter_host(&data(), "fintech-1", None);
        assert_eq!(result.updated, 42);
        assert_eq!(result.servers.len(), 1);
        assert_eq!(result.servers[0].gid, "FINTECH");

        let exact = filter_host(&data(), "fintech-1", Some("FINTECH"));
        assert_eq!(exact.servers.len(), 1);

        let wrong_directory = filter_host(&data(), "fintech-1", Some("MAIL"));
        assert!(wrong_directory.servers.is_empty());
    }

    #[test]
    fn catalog_counts_default_directories_and_tags() {
        let catalog = build_hosts_catalog(&data());
        assert_eq!(catalog["default"]["count"], 1);
        assert_eq!(catalog["directories"][0]["id"], "FINTECH");
        assert_eq!(catalog["tags"][0]["name"], "AWS");
        assert_eq!(catalog["tags"][0]["count"], 2);
    }

    #[test]
    fn configured_hosts_are_kept_after_group_gc_even_with_gid() {
        let configured_hosts = HashMap::from([(String::from("fixed"), configured("fixed", "FINTECH"))]);
        assert!(should_keep_runtime_host(&configured_hosts, "fixed", 0, 10_000, 30));
        assert!(!should_keep_runtime_host(&configured_hosts, "dynamic", 1, 10_000, 30));
        assert!(should_keep_runtime_host(
            &configured_hosts,
            "dynamic",
            9_980,
            10_000,
            30
        ));
    }

    #[test]
    fn configured_host_seed_is_offline_but_keeps_server_metadata() {
        let host = configured("fixed", "FINTECH");
        let stat = configured_host_stat(&host);
        assert_eq!(stat.name, "fixed");
        assert_eq!(stat.gid, "FINTECH");
        assert!(!stat.online4 && !stat.online6);
        assert!(!stat.notify);
    }

    #[test]
    fn fixed_host_report_does_not_follow_agent_gid() {
        let configured_hosts = HashMap::from([(String::from("fixed"), configured("fixed", "FINTECH"))]);
        let configured_groups = HashMap::from([(
            String::from("OTHER"),
            HostGroup {
                gid: "OTHER".to_string(),
                password: "group-secret".to_string(),
                location: String::new(),
                r#type: String::new(),
                notify: true,
                pos: 0,
                weight: 0,
                labels: String::new(),
                tags: Vec::new(),
            },
        )]);
        let mut runtime_hosts = configured_hosts.clone();
        let mut report = HostStat {
            name: "fixed".to_string(),
            gid: "OTHER".to_string(),
            ..HostStat::default()
        };

        assert!(prepare_report_host(
            &configured_hosts,
            &configured_groups,
            &mut runtime_hosts,
            &mut report,
        ));
        assert_eq!(runtime_hosts["fixed"].gid, "FINTECH");
    }

    #[test]
    fn dynamic_host_is_created_from_group_and_expires() {
        let configured_hosts = HashMap::new();
        let configured_groups = HashMap::from([(
            String::from("FINTECH"),
            HostGroup {
                gid: "FINTECH".to_string(),
                password: "group-secret".to_string(),
                location: String::new(),
                r#type: String::new(),
                notify: true,
                pos: 0,
                weight: 0,
                labels: String::new(),
                tags: Vec::new(),
            },
        )]);
        let mut runtime_hosts = HashMap::new();
        let mut report = HostStat {
            name: "dynamic".to_string(),
            gid: "FINTECH".to_string(),
            ..HostStat::default()
        };

        assert!(prepare_report_host(
            &configured_hosts,
            &configured_groups,
            &mut runtime_hosts,
            &mut report,
        ));
        assert_eq!(runtime_hosts["dynamic"].gid, "FINTECH");
        assert!(!should_keep_runtime_host(&configured_hosts, "dynamic", 1, 100, 30));
    }

    #[test]
    fn existing_dynamic_host_accepts_legacy_report_without_gid() {
        let configured_hosts = HashMap::new();
        let configured_groups = HashMap::new();
        let mut runtime_hosts = HashMap::from([("dynamic".to_string(), configured("dynamic", "FINTECH"))]);
        let mut report = HostStat {
            name: "dynamic".to_string(),
            gid: String::new(),
            ..HostStat::default()
        };

        assert!(prepare_report_host(
            &configured_hosts,
            &configured_groups,
            &mut runtime_hosts,
            &mut report,
        ));
    }

    #[test]
    fn first_report_after_startup_is_not_a_node_up_event() {
        assert!(!should_notify_up(0, 100, true, 30, false));
        assert!(!should_notify_up(50, 100, true, 30, false));
        assert!(should_notify_up(50, 100, true, 30, true));
        assert!(!should_notify_up(50, 100, false, 30, true));
    }

    #[test]
    fn ingress_report_prevents_false_offline_while_worker_is_delayed() {
        let processed_at = 100;
        let ingress_at = 129;
        let effective = effective_last_received(processed_at, Some(ingress_at));
        assert_eq!(effective, ingress_at);
        assert!(!is_report_offline(effective, 131, 30));
        assert!(is_report_offline(effective, 160, 30));
    }

    #[test]
    fn processed_timestamp_remains_fallback_for_legacy_state() {
        assert_eq!(effective_last_received(100, None), 100);
        assert_eq!(effective_last_received(100, Some(90)), 100);
        assert!(is_report_offline(100, 131, 30));
        assert!(!is_report_offline(100, 130, 30));
    }

    #[test]
    fn notifier_queue_depth_is_reverted_when_receiver_is_gone() {
        let (sender, receiver) = channel();
        let depth = AtomicUsize::new(0);
        let host = stat("node-1", "", &[]);
        assert_eq!(
            enqueue_notification(
                &sender,
                &depth,
                "test",
                Event::NodeDown,
                Arc::clone(&host),
                AlertDispatch::default(),
            )
            .unwrap(),
            1
        );
        let _ = receiver.recv().unwrap();
        depth.fetch_sub(1, Ordering::Relaxed);
        assert_eq!(depth.load(Ordering::Relaxed), 0);

        drop(receiver);
        assert!(enqueue_notification(
            &sender,
            &depth,
            "test",
            Event::NodeDown,
            host,
            AlertDispatch::default(),
        )
        .is_err());
        assert_eq!(depth.load(Ordering::Relaxed), 0);
    }
}
