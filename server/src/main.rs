#![deny(warnings)]
// #![allow(unused)]
#[macro_use]
extern crate log;
extern crate pretty_env_logger;
#[macro_use]
extern crate prettytable;

use clap::{Parser, Subcommand};
use once_cell::sync::OnceCell;
use std::process;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tokio::signal;

use axum::{
    http::{Method, Uri},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use tower_http::cors::{Any, CorsLayer};

mod alerts;
mod assets;
mod auth;
mod config;
mod grpc;
mod history;
mod http;
mod jinja;
mod jwt;
mod notifier;
mod payload;
mod stats;

static G_CONFIG: OnceCell<crate::config::Config> = OnceCell::new();
static G_STATS_MGR: OnceCell<crate::stats::StatsMgr> = OnceCell::new();

#[derive(Parser, Debug)]
#[command(author, version = env!("APP_VERSION"), about, long_about = None)]
struct Args {
    #[arg(short, long, global = true, default_value = "config.toml")]
    config: String,
    #[arg(short = 't', long, help = "config test, default:false")]
    config_test: bool,
    #[arg(long = "notify-test", help = "notify test, default:false")]
    notify_test: bool,
    #[arg(long = "cloud", help = "cloud mode, load cfg from env var: SRV_CONF")]
    cloud: bool,
    #[command(subcommand)]
    command: Option<ServerCommand>,
}

#[derive(Subcommand, Debug)]
enum ServerCommand {
    /// Inspect, compact, or delete historical metric data.
    History(history::HistoryCli),
    /// Manage and migrate server configuration files.
    #[command(subcommand)]
    Config(ConfigCommand),
}

#[derive(Subcommand, Debug)]
enum ConfigCommand {
    /// Upgrade an old config.toml, preserving a timestamped backup.
    Migrate {
        /// Write the migrated file to another path instead of replacing the input.
        #[arg(short, long)]
        output: Option<String>,
        /// Show the migrated content without writing or creating a backup.
        #[arg(long)]
        dry_run: bool,
        /// Allow replacing an existing different output path.
        #[arg(long)]
        force: bool,
    },
}

fn create_app_router() -> Router {
    let cors_layer = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST])
        .allow_origin(Any);

    Router::new()
        .route("/report", post(http::report))
        .route("/json/stats.json", get(http::get_stats_json)) // 兼容就旧主题
        .route("/api/hosts/catalog", get(http::get_hosts_catalog))
        .route("/api/hosts", get(http::get_hosts))
        // .route("/config.pub.json", get(http::get_site_config_json)) // TODO
        .route("/api/admin/authorize", post(jwt::authorize))
        .route("/api/admin/{path}", get(http::admin_api)) // stats.json || config.json
        .route("/api/history", get(http::get_history))
        // .route("/admin", get(assets::admin_index_handler))
        .route("/detail", get(http::get_detail))
        .route("/map", get(http::get_map))
        .route("/i", get(http::init_client))
        .route("/", get(assets::index_handler))
        .fallback(fallback)
        .layer(cors_layer)
}

async fn fallback(uri: Uri) -> impl IntoResponse {
    assets::static_handler(&uri)
}

/// Waits for Ctrl-C or SIGTERM to initiate graceful shutdown.
#[allow(clippy::missing_panics_doc)]
pub async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }

    println!("signal received, starting graceful shutdown");
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let mut logger = pretty_env_logger::formatted_builder();
    if let Ok(filters) = std::env::var("RUST_LOG") {
        // `formatted_builder()` does not automatically parse RUST_LOG. Keep
        // the systemd-provided filter effective instead of silently falling
        // back to env_logger's default error-only level.
        logger.parse_filters(&filters);
    } else {
        logger.filter_level(log::LevelFilter::Info);
    }
    logger.init();
    let args = Args::parse();

    eprintln!("✨ {} {}", env!("CARGO_BIN_NAME"), env!("APP_VERSION"));

    // config test
    if args.config_test {
        if let Err(error) = config::test_from_file(&args.config) {
            eprintln!("❌ config test failed: {error:#}");
            process::exit(1);
        }
        eprintln!("✨ the conf file {} syntax is ok", &args.config);
        eprintln!("✨ the conf file {} test is successful", &args.config);
        process::exit(0);
    }

    if let Some(ServerCommand::Config(ConfigCommand::Migrate { output, dry_run, force })) = &args.command {
        let report = config::migrate_file(&args.config, output.as_deref(), *dry_run, *force)?;
        eprintln!("✨ {}", report.message);
        if let Some(backup) = report.backup {
            eprintln!("✨ backup: {}", backup.display());
        }
        return Ok(());
    }

    // config load
    if let Some(cfg) = if args.cloud {
        // export SRV_CONF=$(cat config.toml)
        // echo "$SRV_CONF"
        eprintln!("✨ run in cloud mode, load config from env");
        config::from_env()
    } else {
        eprintln!("✨ run in normal mode, load conf from local file `{}", &args.config);
        config::from_file(&args.config)
    } {
        debug!("{}", serde_json::to_string_pretty(&cfg).unwrap());
        G_CONFIG.set(cfg).unwrap();
    } else {
        error!("can't parse config");
        process::exit(1);
    }

    if let Some(ServerCommand::History(command)) = &args.command {
        history::run_history_cli(&G_CONFIG.get().unwrap().history, command)?;
        return Ok(());
    }

    // init tpl
    http::init_jinja_tpl().unwrap();

    // init notifier
    *notifier::NOTIFIER_HANDLE.lock().unwrap() = Some(Handle::current());
    let cfg = G_CONFIG.get().unwrap();
    let notifies: Arc<Mutex<Vec<Box<dyn notifier::Notifier + Send>>>> = Arc::new(Mutex::new(Vec::new()));
    if cfg.tgbot.enabled {
        let o = Box::new(notifier::tgbot::TGBot::new(&cfg.tgbot, &cfg.alerts));
        notifies.lock().unwrap().push(o);
    }
    if cfg.wechat.enabled {
        let o = Box::new(notifier::wechat::WeChat::new(&cfg.wechat, &cfg.alerts));
        notifies.lock().unwrap().push(o);
    }
    if cfg.email.enabled {
        let o = Box::new(notifier::email::Email::new(&cfg.email, &cfg.alerts));
        notifies.lock().unwrap().push(o);
    }
    if cfg.log.enabled {
        let o = Box::new(notifier::log::Log::new(&cfg.log, &cfg.alerts));
        notifies.lock().unwrap().push(o);
    }
    if cfg.webhook.enabled {
        let o = Box::new(notifier::webhook::Webhook::new(&cfg.webhook, &cfg.alerts));
        notifies.lock().unwrap().push(o);
    }
    let enabled_notifiers = notifies
        .lock()
        .unwrap()
        .iter()
        .map(|notifier| notifier.kind())
        .collect::<Vec<_>>();
    info!("alert notifier channels enabled={enabled_notifiers:?}");
    // init notifier end

    // notify test
    if args.notify_test {
        for notifier in &*notifies.lock().unwrap() {
            eprintln!("send test message to {}", notifier.kind());
            notifier.notify_test().unwrap();
        }
        thread::sleep(Duration::from_millis(7000)); // TODO: wait
        eprintln!("Please check for notifications");
        process::exit(0);
    }

    // init mgr
    let mut mgr = crate::stats::StatsMgr::new();
    mgr.init(G_CONFIG.get().unwrap(), notifies)?;
    if G_STATS_MGR.set(mgr).is_err() {
        error!("can't set G_STATS_MGR");
        process::exit(1);
    }

    // serv grpc
    tokio::spawn(async move { grpc::serv_grpc(cfg).await });

    let http_addr = cfg.http_addr.clone();
    eprintln!("🚀 listening on http://{http_addr}");

    let listener = TcpListener::bind(&http_addr).await.unwrap();
    axum::serve(listener, create_app_router())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();

    Ok(())
}
