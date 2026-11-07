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
use std::sync::mpsc::sync_channel;
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, LazyLock, Mutex};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::alerts::AlertManager;
use crate::config::{Host, HostGroup};
use crate::history::HistoryService;
use crate::notifier::{AlertDispatch, Event, Notifier, NotifyOutcome};
use crate::payload::{HostStat, StatsResp};

const SAVE_INTERVAL: u64 = 60;
const OS_LIST: [&str; 10] = [
    "centos", "debian", "ubuntu", "arch", "windows", "macos", "pi", "android", "linux", "freebsd",
];

static STAT_SENDER: OnceCell<SyncSender<Cow<HostStat>>> = OnceCell::new();

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

fn should_notify_up(previous_latest_ts: u64, latest_ts: u64, notify: bool, offline_threshold: u64) -> bool {
    notify && previous_latest_ts != 0 && previous_latest_ts.saturating_add(offline_threshold) < latest_ts
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
    history: Option<Arc<HistoryService>>,
}

impl StatsMgr {
    pub fn new() -> Self {
        Self {
            resp_json: Arc::new(Mutex::new("{}".to_string())),
            stats_data: Arc::new(Mutex::new(StatsResp::new())),
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

        let (stat_tx, stat_rx) = sync_channel(512);
        STAT_SENDER.set(stat_tx).unwrap();
        let (notifier_tx, notifier_rx) = sync_channel(512);
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
            let history = self.history.clone();

            move || loop {
                while let Ok(mut stat) = stat_rx.recv() {
                    trace!("recv stat `{stat:?}");

                    let stat_t = stat.to_mut();

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

                        info.latest_ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                        stat_t.latest_ts = info.latest_ts;

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

                        info!("update stat `{stat_t:?}");
                        if let Ok(mut host_stat_map) = stat_map.lock() {
                            let mut notify_up = false;
                            if let Some(pre_stat) = host_stat_map.get(&stat_t.name) {
                                if stat_t.ip_info.is_none() {
                                    stat_t.ip_info = pre_stat.ip_info.clone();
                                }

                                if should_notify_up(
                                    pre_stat.latest_ts,
                                    stat_t.latest_ts,
                                    stat_t.notify,
                                    cfg.offline_threshold,
                                ) {
                                    notify_up = true;
                                }
                            }
                            let arc_stat = Arc::new(stat.into_owned());
                            if let Some(history) = history.as_ref() {
                                history.record(&arc_stat);
                            }
                            if notify_up {
                                // node up notify
                                if let Err(error) = notifier_tx.send((
                                    Event::NodeUp,
                                    Arc::clone(&arc_stat),
                                    AlertDispatch::default(),
                                )) {
                                    error!(
                                        "alert event queue failed event=NodeUp host={} error={error}",
                                        arc_stat.name
                                    );
                                }
                            }
                            host_stat_map.insert(arc_stat.name.clone(), arc_stat);
                            //trace!("{:?}", host_stat_map);
                        }
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
            let notifier_tx = notifier_tx.clone();
            let history = self.history.clone();
            let mut alert_manager = alert_manager;
            let mut latest_notify_ts = 0_u64;
            let mut latest_save_ts = 0_u64;
            let mut latest_group_gc = 0_u64;
            let mut latest_alert_check_ts = 0_u64;
            move || loop {
                thread::sleep(Duration::from_millis(500));

                let mut resp = StatsResp::new();
                let now = resp.updated;
                let mut any_notified = false;

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
                }

                if let Ok(mut host_stat_map) = stat_map.lock() {
                    for (_, stat) in host_stat_map.iter_mut() {
                        if stat.disabled {
                            resp.servers.push(Arc::clone(stat));
                            continue;
                        }
                        let mut became_offline = false;
                        let notify_event = {
                            let o = Arc::make_mut(stat);
                            // 30s 下线
                            let previous_latest_ts = o.latest_ts;
                            if o.latest_ts + cfg.offline_threshold < now {
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

                            // determine notify event (o is dropped after this block)
                            if o.notify && latest_notify_ts + cfg.notify_interval < now {
                                if o.online4 || o.online6 {
                                    Some(Event::Custom)
                                } else {
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
                                }
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

                        // Client notify — Arc::clone is O(1), no HostStat copy.
                        // A resource recovery is a distinct event from the
                        // regular Custom event, so a recovery notification is
                        // emitted once even though the metric is no longer
                        // above its trigger threshold.
                        if let Some(event) = notify_event {
                            let mut deliveries = Vec::new();
                            if matches!(&event, Event::Custom) {
                                match alert_manager.as_mut() {
                                    Some(manager) => {
                                        let evaluation = manager.evaluate_custom(stat, now);
                                        if !evaluation.recovered.is_empty() {
                                            deliveries.push((
                                                Event::AlertRecovered,
                                                AlertDispatch {
                                                    recovered: evaluation.recovered,
                                                    ..AlertDispatch::default()
                                                },
                                            ));
                                        }
                                        if evaluation.should_send {
                                            deliveries.push((
                                                Event::Custom,
                                                AlertDispatch {
                                                    suppressed_rule_ids: evaluation.suppressed_rule_ids,
                                                    ..AlertDispatch::default()
                                                },
                                            ));
                                        }
                                    }
                                    None => deliveries.push((event, AlertDispatch::default())),
                                }
                            } else {
                                deliveries.push((event, AlertDispatch::default()));
                            }
                            for (delivery_event, dispatch) in deliveries {
                                info!(
                                    "alert event detected event={delivery_event:?} host={} gid={}",
                                    stat.name,
                                    stat.gid
                                );
                                if let Err(error) = notifier_tx.send((delivery_event.clone(), Arc::clone(stat), dispatch)) {
                                    error!(
                                        "alert delivery queue_failed event={delivery_event:?} host={} error={error}",
                                        stat.name
                                    );
                                } else {
                                    info!(
                                        "alert delivery queue=accepted event={delivery_event:?} host={}",
                                        stat.name
                                    );
                                    any_notified = true;
                                }
                            }
                        }

                        resp.servers.push(Arc::clone(stat));
                    }
                    if any_notified {
                        latest_notify_ts = now;
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
            }
        });

        // notify thread
        thread::spawn(move || loop {
            while let Ok(msg) = notifier_rx.recv() {
                let (e, stat, dispatch) = msg;
                let notify_list = &*notifies.lock().unwrap();
                trace!("recv notify => {e:?}, {stat:?}");
                for n in notify_list {
                    trace!("{} notify {:?} => {:?}", n.kind(), e, stat);
                    match n.notify(&e, &stat, &dispatch) {
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
    pub fn report(&self, data: serde_json::Value) -> Result<()> {
        static SENDER: LazyLock<SyncSender<Cow<'static, HostStat>>> =
            LazyLock::new(|| STAT_SENDER.get().unwrap().clone());

        match serde_json::from_value(data) {
            Ok(stat) => {
                trace!("send stat => {stat:?} ");
                SENDER
                    .send(Cow::Owned(stat))
                    .map_err(|error| anyhow::anyhow!("stat queue is unavailable: {error}"))?;
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
        assert!(!should_notify_up(0, 100, true, 30));
        assert!(should_notify_up(50, 100, true, 30));
        assert!(!should_notify_up(50, 100, false, 30));
    }
}
