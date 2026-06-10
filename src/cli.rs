use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "egrex",
    version,
    about = "Local SOCKS5 proxy pool forwarder",
    long_about = "Egrex starts a local SOCKS5 proxy and rotates outbound connections through verified upstream SOCKS5 nodes. Runtime files are stored under ~/.egrex, and the background service maintains candidate and online pools automatically."
)]
pub struct Cli {
    /// Command to run. If omitted, the proxy runs in foreground mode.
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the local SOCKS5 proxy in the foreground.
    Run,
    /// Run the local SOCKS5 proxy in the background and create pid.lock.
    Start,
    /// Stop the background proxy. By default, existing connections are allowed to finish.
    Stop {
        /// Force stop the background process without waiting for active connections.
        #[arg(long, short)]
        force: bool,
    },
    /// Show process, listen address, traffic, and pool status.
    Status,
    /// Fetch SOCKS5 upstream candidates from FOFA.
    Update {
        /// Search for SOCKS5 nodes discovered within the last number of days.
        #[arg(long, default_value_t = 10)]
        days: i64,
        /// Number of results to fetch from one page.
        #[arg(long, default_value_t = 1000)]
        size: u32,
        /// FOFA search page number.
        #[arg(long, default_value_t = 1)]
        page: u32,
    },
    /// Update a config value and save it to ~/.egrex/config.toml.
    Set {
        #[command(subcommand)]
        command: SetCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum SetCommand {
    /// Set the local SOCKS5 listen host, for example 127.0.0.1.
    Host {
        /// Listen host address.
        value: String,
    },
    /// Set the local SOCKS5 listen port, for example 1080.
    Port {
        /// Listen port.
        value: u16,
    },
    /// Set the URL used to check upstream proxy availability.
    #[command(alias = "check_url")]
    CheckUrl {
        /// HTTP/HTTPS URL used to check upstream proxy quality.
        value: String,
    },
    /// Set the maximum allowed upstream proxy latency in milliseconds.
    #[command(alias = "max_latency")]
    MaxLatency {
        /// Maximum latency in milliseconds.
        value: u64,
    },
    /// Set the FOFA API base URL. Defaults to https://fofa.info/api/v1/.
    #[command(alias = "fofa_api")]
    FofaApi {
        /// FOFA API base URL.
        value: String,
    },
    /// Set the FOFA API key.
    #[command(alias = "fofa_key")]
    FofaKey {
        /// FOFA API key. It is saved to ~/.egrex/config.toml and should not be committed.
        value: String,
    },
}
