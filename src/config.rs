use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 1080;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            host: DEFAULT_HOST.to_string(),
            port: DEFAULT_PORT,
        }
    }
}

impl AppConfig {
    pub fn load_or_default(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read config from {}", path.display()))?;

        if content.trim().is_empty() {
            return Ok(Self::default());
        }

        let raw: RawConfig = toml::from_str(&content)
            .with_context(|| format!("failed to parse config from {}", path.display()))?;

        Ok(raw.into_app_config())
    }

    pub fn listen_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let content = toml::to_string_pretty(self).context("failed to serialize config")?;

        fs::write(path, content)
            .with_context(|| format!("failed to write config to {}", path.display()))
    }
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    host: Option<String>,
    port: Option<u16>,
    listen: Option<String>,
}

impl RawConfig {
    fn into_app_config(self) -> AppConfig {
        let mut config = AppConfig::default();

        if let Some(listen) = self.listen {
            if let Some((host, port)) = parse_listen_addr(&listen) {
                config.host = host;
                config.port = port;
            }
        }

        if let Some(host) = self.host {
            config.host = host;
        }

        if let Some(port) = self.port {
            config.port = port;
        }

        config
    }
}

fn parse_listen_addr(value: &str) -> Option<(String, u16)> {
    let (host, port) = value.rsplit_once(':')?;
    let port = port.parse().ok()?;
    Some((host.trim_matches(['[', ']']).to_string(), port))
}
