use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{Duration, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, path::Path};
use url::Url;

use crate::config::AppConfig;

const SEARCH_PATH: &str = "search/all";

#[derive(Debug)]
pub struct UpstreamStore {
    pub query: String,
    pub after: String,
    pub hosts: Vec<String>,
    pub next_page: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FofaState {
    pub query_after: String,
    pub next_page: u32,
}

#[derive(Debug, Deserialize)]
struct FofaResponse {
    error: Option<bool>,
    errmsg: Option<String>,
    results: Option<Vec<FofaResultRow>>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FofaResultRow {
    Host(String),
    Fields(Vec<String>),
}

pub async fn update_many(
    config: &AppConfig,
    output_path: impl AsRef<Path>,
    days: i64,
    page_size: u32,
    start_page: u32,
    limit: usize,
) -> Result<UpstreamStore> {
    let store = fetch_hosts(config, days, page_size, start_page, limit).await?;
    save(output_path, &store)?;
    Ok(store)
}

pub async fn fetch_hosts_with_state(
    config: &AppConfig,
    state_path: impl AsRef<Path>,
    days: i64,
    page_size: u32,
    limit: usize,
) -> Result<UpstreamStore> {
    let state_path = state_path.as_ref();
    let after = query_after(days);
    let state = read_state(state_path)?.filter(|state| state.query_after == after);
    let start_page = state.map_or(1, |state| state.next_page.max(1));

    let mut store = fetch_hosts(config, days, page_size, start_page, limit).await?;
    if store.hosts.is_empty() && start_page > 1 {
        store = fetch_hosts(config, days, page_size, 1, limit).await?;
    }

    write_state(
        state_path,
        &FofaState {
            query_after: store.after.clone(),
            next_page: store.next_page,
        },
    )?;

    Ok(store)
}

pub async fn fetch_hosts(
    config: &AppConfig,
    days: i64,
    page_size: u32,
    start_page: u32,
    limit: usize,
) -> Result<UpstreamStore> {
    if config.fofa_key.trim().is_empty() {
        bail!("fofa_key is empty, set it with `egrex set fofa-key <key>` first");
    }

    let after = query_after(days);
    let query = build_query(&after);
    let qbase64 = STANDARD.encode(query.as_bytes());
    let mut hosts = BTreeSet::new();
    let mut page = start_page.max(1);
    let mut next_page = page;

    while hosts.len() < limit {
        let url = build_search_url(
            &config.fofa_api,
            &config.fofa_key,
            &qbase64,
            page_size,
            page,
        )?;
        let response = request_search(url).await?;
        let page_hosts = parse_hosts(response);

        if page_hosts.is_empty() {
            next_page = 1;
            break;
        }

        for host in page_hosts {
            hosts.insert(host);
            if hosts.len() >= limit {
                break;
            }
        }

        page += 1;
        next_page = page;
    }

    Ok(UpstreamStore {
        query,
        after,
        hosts: hosts.into_iter().collect(),
        next_page,
    })
}

async fn request_search(url: Url) -> Result<FofaResponse> {
    let response = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .context("failed to request fofa search api")?
        .error_for_status()
        .context("fofa search api returned an error status")?
        .json::<FofaResponse>()
        .await
        .context("failed to parse fofa search api response")?;

    if response.error.unwrap_or(false) {
        bail!(
            "fofa search api returned error: {}",
            response
                .errmsg
                .unwrap_or_else(|| "unknown error".to_string())
        );
    }

    Ok(response)
}

fn parse_hosts(response: FofaResponse) -> Vec<String> {
    response
        .results
        .unwrap_or_default()
        .into_iter()
        .filter_map(|row| match row {
            FofaResultRow::Host(host) => Some(host),
            FofaResultRow::Fields(fields) => fields.into_iter().next(),
        })
        .filter(|host| !host.trim().is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub fn query_after(days: i64) -> String {
    let days = days.max(1);
    let after = Local::now() - Duration::days(days);
    after.format("%Y-%m-%d").to_string()
}

fn build_query(after: &str) -> String {
    format!(
        "protocol==\"socks5\" && \"Version:5 Method:No Authentication(0x00)\" && after=\"{after}\" && country=\"CN\""
    )
}

pub fn read_state(path: impl AsRef<Path>) -> Result<Option<FofaState>> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read fofa state from {}", path.display()))?;
    if content.trim().is_empty() {
        return Ok(None);
    }

    let state: FofaState = toml::from_str(&content)
        .with_context(|| format!("failed to parse fofa state from {}", path.display()))?;

    if state.next_page == 0 || NaiveDate::parse_from_str(&state.query_after, "%Y-%m-%d").is_err() {
        return Ok(None);
    }

    Ok(Some(state))
}

fn write_state(path: impl AsRef<Path>, state: &FofaState) -> Result<()> {
    let path = path.as_ref();
    let content = toml::to_string_pretty(state).context("failed to serialize fofa state")?;
    fs::write(path, content)
        .with_context(|| format!("failed to write fofa state to {}", path.display()))
}

fn build_search_url(api_base: &str, key: &str, qbase64: &str, size: u32, page: u32) -> Result<Url> {
    let mut url = Url::parse(api_base).context("invalid fofa_api url")?;
    let mut path = url.path().trim_end_matches('/').to_string();
    path.push('/');
    path.push_str(SEARCH_PATH);
    url.set_path(&path);

    url.query_pairs_mut()
        .append_pair("key", key)
        .append_pair("qbase64", qbase64)
        .append_pair("size", &size.to_string())
        .append_pair("page", &page.to_string())
        .append_pair("fields", "host");

    Ok(url)
}

fn save(path: impl AsRef<Path>, store: &UpstreamStore) -> Result<()> {
    let path = path.as_ref();
    let mut content = store.hosts.join("\n");
    if !content.is_empty() {
        content.push('\n');
    }

    fs::write(path, content)
        .with_context(|| format!("failed to write upstream hosts to {}", path.display()))
}
