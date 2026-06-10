mod checker;
mod cli;
mod config;
mod daemon;
mod fofa;
mod pool;
mod proxy;
mod traffic;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command, SetCommand};
use config::AppConfig;
use daemon::{Status, StopResult};
use std::sync::Arc;
use tokio::sync::RwLock;
use url::Url;

const CONFIG_PATH: &str = "config.toml";
const PID_LOCK_PATH: &str = "pid.lock";
const STOP_LOCK_PATH: &str = "stop.lock";
const TRAFFIC_PATH: &str = "traffic.lock";
const ONLINE_PATH: &str = "online.lock";
const CANDIDATES_PATH: &str = "candidates.lock";
const CANDIDATE_TARGET: usize = 200;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command.unwrap_or(Command::Run) {
        Command::Run => {
            let config = AppConfig::load_or_default(CONFIG_PATH)?;
            let traffic = traffic::TrafficCounter::init(TRAFFIC_PATH)?;
            let pool = pool::ProxyPool::init(&config, ONLINE_PATH, CANDIDATES_PATH).await?;
            let config = Arc::new(RwLock::new(config));
            pool.spawn_maintainers(config.clone());
            proxy::run(
                config,
                CONFIG_PATH,
                traffic,
                pool,
                PID_LOCK_PATH,
                STOP_LOCK_PATH,
            )
            .await?;
        }
        Command::Start => {
            let config = AppConfig::load_or_default(CONFIG_PATH)?;
            let listen_addr = config.listen_addr();
            let pid = daemon::start(PID_LOCK_PATH, listen_addr.clone())?;
            println!("egrex started in background with pid {pid}");
            println!("listen = {listen_addr}");
        }
        Command::Stop { force } => match daemon::stop(PID_LOCK_PATH, STOP_LOCK_PATH, force)? {
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
        Command::Status => match daemon::status(PID_LOCK_PATH)? {
            Status::Running { pid, listen_addr } => {
                let traffic = traffic::read(TRAFFIC_PATH)?;
                let config = AppConfig::load_or_default(CONFIG_PATH)?;
                println!("egrex is running with pid {pid}");
                println!(
                    "listen = {}",
                    listen_addr.unwrap_or_else(|| "unknown".to_string())
                );
                println!("upload = {}", format_bytes(traffic.upload_bytes));
                println!("download = {}", format_bytes(traffic.download_bytes));
                println!(
                    "total = {}",
                    format_bytes(traffic.upload_bytes + traffic.download_bytes)
                );
                print_pool_status(&config)?;
            }
            Status::Stale { pid, listen_addr } => {
                println!("egrex is not running, but pid.lock contains stale pid {pid}");
                println!(
                    "listen = {}",
                    listen_addr.unwrap_or_else(|| "unknown".to_string())
                );
                let config = AppConfig::load_or_default(CONFIG_PATH)?;
                print_pool_status(&config)?;
            }
            Status::NotStarted => {
                println!("egrex is not running");
                let config = AppConfig::load_or_default(CONFIG_PATH)?;
                print_pool_status(&config)?;
            }
        },
        Command::Update { days, size, page } => {
            let config = AppConfig::load_or_default(CONFIG_PATH)?;
            let store =
                fofa::update_many(&config, CANDIDATES_PATH, days, size, page, CANDIDATE_TARGET)
                    .await?;

            println!("updated upstream hosts");
            println!("query = {}", store.query);
            println!("hosts = {}", store.hosts.len());
            println!("output = {CANDIDATES_PATH}");
        }
        Command::Set { command } => {
            let mut config = AppConfig::load_or_default(CONFIG_PATH)?;

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

            config.save(CONFIG_PATH)?;
            println!("listen = {}", config.listen_addr());
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

fn print_pool_status(config: &AppConfig) -> Result<()> {
    println!("check_url = {}", config.check_url);
    println!("max_latency = {} ms", config.max_latency);
    println!("online_pool = {}/80", count_pool_lines(ONLINE_PATH)?);
    println!(
        "candidate_pool = {}/{}",
        count_pool_lines(CANDIDATES_PATH)?,
        CANDIDATE_TARGET
    );
    println!(
        "stop_requested = {}",
        if daemon::shutdown_requested(STOP_LOCK_PATH) {
            "yes"
        } else {
            "no"
        }
    );
    Ok(())
}

fn count_pool_lines(path: &str) -> Result<usize> {
    let path = std::path::Path::new(path);
    if !path.exists() {
        return Ok(0);
    }

    Ok(checker::read_upstreams(path)?.len())
}
