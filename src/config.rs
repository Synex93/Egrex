use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 1080;
const DEFAULT_CHECK_URL: &str = "https://cloudflare.com/cdn-cgi/trace";
const DEFAULT_FOFA_API: &str = "https://fofa.info/api/v1/";
const DEFAULT_MAX_LATENCY: u64 = 5000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub check_url: String,
    pub max_latency: u64,
    pub fofa_api: String,
    pub fofa_key: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            host: DEFAULT_HOST.to_string(),
            port: DEFAULT_PORT,
            check_url: DEFAULT_CHECK_URL.to_string(),
            max_latency: DEFAULT_MAX_LATENCY,
            fofa_api: DEFAULT_FOFA_API.to_string(),
            fofa_key: String::new(),
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
    check_url: Option<String>,
    max_latency: Option<u64>,
    fofa_api: Option<String>,
    fofa_key: Option<String>,
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

        if let Some(check_url) = self.check_url {
            config.check_url = check_url;
        }

        if let Some(max_latency) = self.max_latency {
            config.max_latency = max_latency;
        }

        if let Some(fofa_api) = self.fofa_api {
            config.fofa_api = fofa_api;
        }

        if let Some(fofa_key) = self.fofa_key {
            config.fofa_key = fofa_key;
        }

        config
    }
}

fn parse_listen_addr(value: &str) -> Option<(String, u16)> {
    let (host, port) = value.rsplit_once(':')?;
    let port = port.parse().ok()?;
    Some((host.trim_matches(['[', ']']).to_string(), port))
}
