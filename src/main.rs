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
            println!("{} egrex started in background", green("running"));
            println!("pid = {pid}");
            println!("listen = {}", cyan(&listen_addr));
            println!("data_dir = {}", paths.data_dir.display());
        }
        Command::Stop { force } => match daemon::stop(&paths.pid_lock, &paths.stop_lock, force)? {
            StopResult::Stopped { pid, force } => {
                if force {
                    println!("{} egrex force stopped pid {pid}", red("stopped"));
                } else {
                    println!("{} egrex stopped gracefully pid {pid}", green("stopped"));
                }
            }
            StopResult::StaleRemoved { pid } => {
                println!(
                    "{} egrex was not running, removed stale pid.lock for pid {pid}",
                    yellow("stale")
                );
            }
            StopResult::NotStarted => println!("{} egrex is not running", red("offline")),
        },
        Command::Status => match daemon::status(&paths.pid_lock)? {
            Status::Running { pid, listen_addr } => {
                let traffic = traffic::read(&paths.traffic)?;
                let config = AppConfig::load_or_default(&paths.config)?;
                println!("status = {}", green("running"));
                println!("pid = {pid}");
                println!(
                    "listen = {}",
                    cyan(&listen_addr.unwrap_or_else(|| "unknown".to_string()))
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
                println!("status = {}", yellow("stale"));
                println!("pid = {pid}");
                println!(
                    "listen = {}",
                    cyan(&listen_addr.unwrap_or_else(|| "unknown".to_string()))
                );
                println!("data_dir = {}", paths.data_dir.display());
                let config = AppConfig::load_or_default(&paths.config)?;
                print_pool_status(&config, &paths)?;
            }
            Status::NotStarted => {
                println!("status = {}", red("offline"));
                println!("data_dir = {}", paths.data_dir.display());
                let config = AppConfig::load_or_default(&paths.config)?;
                print_pool_status(&config, &paths)?;
            }
        },
        Command::Update {
            days,
            size,
            limit,
            page,
        } => {
            let config = AppConfig::load_or_default(&paths.config)?;
            let store =
                fofa::update_many(&config, &paths.candidates, days, size, page, limit).await?;
            fofa::write_state(
                &paths.fofa_state,
                &fofa::FofaState {
                    query_after: store.after.clone(),
                    next_page: store.next_page,
                    exhausted: store.exhausted,
                },
            )?;

            println!("updated upstream hosts");
            println!("query = {}", store.query);
            println!("after = {}", store.after);
            println!("next_page = {}", store.next_page);
            println!("exhausted = {}", if store.exhausted { "yes" } else { "no" });
            println!("hosts = {}", store.hosts.len());
            println!("state = {}", paths.fofa_state.display());
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
                SetCommand::CheckFallbackUrl { value } => {
                    Url::parse(&value)?;
                    config.check_fallback_url = value;
                }
                SetCommand::MaxLatency { value } => config.max_latency = value.max(1),
                SetCommand::FofaApi { value } => {
                    Url::parse(&value)?;
                    config.fofa_api = value;
                }
                SetCommand::FofaKey { value } => config.fofa_key = value,
            }

            config.save(&paths.config)?;
            println!("{} config updated", green("ok"));
            println!("listen = {}", cyan(&config.listen_addr()));
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
    println!("check_fallback_url = {}", config.check_fallback_url);
    println!("max_latency = {} ms", config.max_latency);
    println!("fofa_after = {}", fofa::query_after(pool::FOFA_DAYS));
    match fofa::read_state(&paths.fofa_state)? {
        Some(state) => {
            println!("fofa_next_page = {}", state.next_page);
            println!(
                "fofa_exhausted = {}",
                if state.exhausted { "yes" } else { "no" }
            );
        }
        None => {
            println!("fofa_next_page = 1");
            println!("fofa_exhausted = no");
        }
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

fn green(value: &str) -> String {
    color("32", value)
}

fn yellow(value: &str) -> String {
    color("33", value)
}

fn red(value: &str) -> String {
    color("31", value)
}

fn cyan(value: &str) -> String {
    color("36", value)
}

fn color(code: &str, value: &str) -> String {
    format!("\x1b[{code}m{value}\x1b[0m")
}

fn count_pool_lines(path: &std::path::Path) -> Result<usize> {
    if !path.exists() {
        return Ok(0);
    }

    Ok(checker::read_upstreams(path)?.len())
}
