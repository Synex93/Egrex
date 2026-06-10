mod cli;
mod config;
mod proxy;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command, SetCommand};
use config::AppConfig;

const CONFIG_PATH: &str = "config.toml";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command.unwrap_or(Command::Run) {
        Command::Run => {
            let config = AppConfig::load_or_default(CONFIG_PATH)?;
            proxy::run(config.listen_addr()).await?;
        }
        Command::Set { command } => {
            let mut config = AppConfig::load_or_default(CONFIG_PATH)?;

            match command {
                SetCommand::Host { value } => config.host = value,
                SetCommand::Port { value } => config.port = value,
            }

            config.save(CONFIG_PATH)?;
            println!("listen = {}", config.listen_addr());
        }
    }

    Ok(())
}
