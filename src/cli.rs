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
    Set {
        #[command(subcommand)]
        command: SetCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum SetCommand {
    Host { value: String },
    Port { value: u16 },
}
