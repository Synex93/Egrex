use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "egrex", version, about = "Local SOCKS5 proxy forwarder")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Run,
    Start,
    Stop {
        #[arg(long, short)]
        force: bool,
    },
    Status,
    Update {
        #[arg(long, default_value_t = 7)]
        days: i64,
        #[arg(long, default_value_t = 100)]
        size: u32,
        #[arg(long, default_value_t = 1)]
        page: u32,
    },
    Set {
        #[command(subcommand)]
        command: SetCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum SetCommand {
    Host {
        value: String,
    },
    Port {
        value: u16,
    },
    #[command(alias = "check_url")]
    CheckUrl {
        value: String,
    },
    #[command(alias = "max_latency")]
    MaxLatency {
        value: u64,
    },
    #[command(alias = "fofa_api")]
    FofaApi {
        value: String,
    },
    #[command(alias = "fofa_key")]
    FofaKey {
        value: String,
    },
}
