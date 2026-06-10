use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{Duration, Local};
use serde::Deserialize;
use std::{collections::BTreeSet, fs, path::Path};
use url::Url;

use crate::config::AppConfig;

const SEARCH_PATH: &str = "search/all";

#[derive(Debug)]
pub struct UpstreamStore {
    pub query: String,
    pub hosts: Vec<String>,
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

    let query = build_query(days);
    let qbase64 = STANDARD.encode(query.as_bytes());
    let mut hosts = BTreeSet::new();
    let mut page = start_page.max(1);

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
            break;
        }

        for host in page_hosts {
            hosts.insert(host);
            if hosts.len() >= limit {
                break;
            }
        }

        page += 1;
    }

    Ok(UpstreamStore {
        query,
        hosts: hosts.into_iter().collect(),
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

fn build_query(days: i64) -> String {
    let days = days.max(1);
    let after = Local::now() - Duration::days(days);
    let after = after.format("%Y-%m-%d");

    format!(
        "protocol==\"socks5\" && \"Version:5 Method:No Authentication(0x00)\" && after=\"{after}\" && country=\"CN\""
    )
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
