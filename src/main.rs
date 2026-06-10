mod checker;
mod cli;
mod config;
mod daemon;
mod fofa;
mod paths;
mod pool;
mod proxy;
mod traffic;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command, SetCommand};
use config::AppConfig;
use daemon::{Status, StopResult};
use paths::RuntimePaths;
use std::sync::Arc;
use tokio::sync::RwLock;
use url::Url;

const CANDIDATE_TARGET: usize = 200;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let paths = RuntimePaths::init()?;

    match cli.command.unwrap_or(Command::Run) {
        Command::Run => {
            let config = AppConfig::load_or_default(&paths.config)?;
            let traffic = traffic::TrafficCounter::init(&paths.traffic)?;
            let pool =
                pool::ProxyPool::init(&config, &paths.online, &paths.candidates, &paths.fofa_state)
                    .await?;
            let config = Arc::new(RwLock::new(config));
            pool.spawn_maintainers(config.clone());
            proxy::run(
                config,
                &paths.config,
                traffic,
                pool,
                &paths.pid_lock,
                &paths.stop_lock,
            )
            .await?;
        }
        Command::Start => {
            let config = AppConfig::load_or_default(&paths.config)?;
            let listen_addr = config.listen_addr();
            let pid = daemon::start(&paths.pid_lock, listen_addr.clone())?;
            println!("egrex started in background with pid {pid}");
            println!("listen = {listen_addr}");
            println!("data_dir = {}", paths.data_dir.display());
        }
        Command::Stop { force } => match daemon::stop(&paths.pid_lock, &paths.stop_lock, force)? {
            StopResult::Stopped { pid, force } => {
                if force {
                    println!("egrex force stopped pid {pid}");
                } else {
                    println!("egrex stopped gracefully pid {pid}");
                }
            }
            StopResult::StaleRemoved { pid } => {
                println!("egrex was not running, removed stale pid.lock for pid {pid}");
            }
            StopResult::NotStarted => println!("egrex is not running"),
        },
        Command::Status => match daemon::status(&paths.pid_lock)? {
            Status::Running { pid, listen_addr } => {
                let traffic = traffic::read(&paths.traffic)?;
                let config = AppConfig::load_or_default(&paths.config)?;
                println!("egrex is running with pid {pid}");
                println!(
                    "listen = {}",
                    listen_addr.unwrap_or_else(|| "unknown".to_string())
                );
                println!("data_dir = {}", paths.data_dir.display());
                println!("upload = {}", format_bytes(traffic.upload_bytes));
                println!("download = {}", format_bytes(traffic.download_bytes));
                println!(
                    "total = {}",
                    format_bytes(traffic.upload_bytes + traffic.download_bytes)
                );
                print_pool_status(&config, &paths)?;
            }
            Status::Stale { pid, listen_addr } => {
                println!("egrex is not running, but pid.lock contains stale pid {pid}");
                println!(
                    "listen = {}",
                    listen_addr.unwrap_or_else(|| "unknown".to_string())
                );
                println!("data_dir = {}", paths.data_dir.display());
                let config = AppConfig::load_or_default(&paths.config)?;
                print_pool_status(&config, &paths)?;
            }
            Status::NotStarted => {
                println!("egrex is not running");
                println!("data_dir = {}", paths.data_dir.display());
                let config = AppConfig::load_or_default(&paths.config)?;
                print_pool_status(&config, &paths)?;
            }
        },
        Command::Update { days, size, page } => {
            let config = AppConfig::load_or_default(&paths.config)?;
            let store = fofa::update_many(
                &config,
                &paths.candidates,
                days,
                size,
                page,
                CANDIDATE_TARGET,
            )
            .await?;

            println!("updated upstream hosts");
            println!("query = {}", store.query);
            println!("after = {}", store.after);
            println!("next_page = {}", store.next_page);
            println!("hosts = {}", store.hosts.len());
            println!("output = {}", paths.candidates.display());
        }
        Command::Set { command } => {
            let mut config = AppConfig::load_or_default(&paths.config)?;

            match command {
                SetCommand::Host { value } => config.host = value,
                SetCommand::Port { value } => config.port = value,
                SetCommand::CheckUrl { value } => {
                    Url::parse(&value)?;
                    config.check_url = value;
                }
                SetCommand::MaxLatency { value } => config.max_latency = value.max(1),
                SetCommand::FofaApi { value } => {
                    Url::parse(&value)?;
                    config.fofa_api = value;
                }
                SetCommand::FofaKey { value } => config.fofa_key = value,
            }

            config.save(&paths.config)?;
            println!("listen = {}", config.listen_addr());
            println!("config = {}", paths.config.display());
        }
    }

    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = UNITS[0];

    for next_unit in UNITS.iter().skip(1) {
        if value < 1024.0 {
            break;
        }

        value /= 1024.0;
        unit = next_unit;
    }

    if unit == "B" {
        format!("{bytes} B")
    } else {
        format!("{value:.2} {unit}")
    }
}

fn print_pool_status(config: &AppConfig, paths: &RuntimePaths) -> Result<()> {
    println!("check_url = {}", config.check_url);
    println!("max_latency = {} ms", config.max_latency);
    println!("fofa_after = {}", fofa::query_after(pool::FOFA_DAYS));
    match fofa::read_state(&paths.fofa_state)? {
        Some(state) => println!("fofa_next_page = {}", state.next_page),
        None => println!("fofa_next_page = 1"),
    }
    println!("online_pool = {}/80", count_pool_lines(&paths.online)?);
    println!(
        "candidate_pool = {}/{}",
        count_pool_lines(&paths.candidates)?,
        CANDIDATE_TARGET
    );
    println!(
        "stop_requested = {}",
        if daemon::shutdown_requested(&paths.stop_lock) {
            "yes"
        } else {
            "no"
        }
    );
    Ok(())
}

fn count_pool_lines(path: &std::path::Path) -> Result<usize> {
    if !path.exists() {
        return Ok(0);
    }

    Ok(checker::read_upstreams(path)?.len())
}
