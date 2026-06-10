use anyhow::{Context, Result};
use reqwest::{Client, Proxy};
use std::{collections::BTreeSet, fs, path::Path, sync::Arc, time::Duration};
use tokio::sync::Semaphore;

const CHECK_ATTEMPTS_PER_URL: usize = 3;

#[derive(Debug, Clone)]
pub struct AliveUpstream {
    pub host: String,
    pub latency: Duration,
}

pub async fn check_hosts(
    upstreams: Vec<String>,
    check_urls: &[String],
    concurrency: usize,
    timeout: Duration,
    max_latency: Duration,
) -> Result<Vec<AliveUpstream>> {
    let semaphore = Arc::new(Semaphore::new(concurrency.max(1)));
    let mut tasks = Vec::with_capacity(upstreams.len());

    for upstream in upstreams {
        let permit = semaphore.clone().acquire_owned().await?;
        let check_urls = check_urls.to_vec();

        tasks.push(tokio::spawn(async move {
            let _permit = permit;
            check_one(&upstream, &check_urls, timeout, max_latency)
                .await
                .map(|latency| AliveUpstream {
                    host: upstream,
                    latency,
                })
        }));
    }

    let mut alive = Vec::new();
    for task in tasks {
        if let Some(upstream) = task.await.context("failed to join check task")? {
            alive.push(upstream);
        }
    }

    alive.sort_by_key(|upstream| upstream.latency);

    Ok(alive)
}

pub fn read_upstreams(path: &Path) -> Result<Vec<String>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read upstreams from {}", path.display()))?;

    let upstreams = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('#'))
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    Ok(upstreams)
}

pub fn write_upstreams(path: &Path, upstreams: &[String]) -> Result<()> {
    let mut content = upstreams.join("\n");
    if !content.is_empty() {
        content.push('\n');
    }

    fs::write(path, content)
        .with_context(|| format!("failed to write upstreams to {}", path.display()))
}

pub async fn check_one(
    upstream: &str,
    check_urls: &[String],
    timeout: Duration,
    max_latency: Duration,
) -> Option<Duration> {
    let proxy = match Proxy::all(format!("socks5h://{upstream}")) {
        Ok(proxy) => proxy,
        Err(err) => {
            tracing::debug!(%upstream, error = %err, "invalid upstream proxy");
            return None;
        }
    };

    let client = match Client::builder()
        .proxy(proxy)
        .timeout(timeout)
        .danger_accept_invalid_certs(true)
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            tracing::debug!(%upstream, error = %err, "failed to build check client");
            return None;
        }
    };

    for check_url in check_urls.iter().filter(|url| !url.trim().is_empty()) {
        for attempt in 1..=CHECK_ATTEMPTS_PER_URL {
            let started_at = std::time::Instant::now();
            match client.get(check_url).send().await {
                Ok(response) => {
                    let status = response.status();
                    let latency = started_at.elapsed();
                    if latency > max_latency {
                        tracing::debug!(%upstream, %check_url, attempt, status = %status, latency_ms = latency.as_millis(), max_latency_ms = max_latency.as_millis(), "upstream check latency exceeded");
                        continue;
                    }

                    if !status.is_success() {
                        tracing::debug!(%upstream, %check_url, attempt, status = %status, latency_ms = latency.as_millis(), "upstream check returned non-success status");
                        continue;
                    }

                    tracing::debug!(%upstream, %check_url, attempt, status = %status, latency_ms = latency.as_millis(), "upstream check passed");
                    return Some(latency);
                }
                Err(err) => {
                    tracing::debug!(%upstream, %check_url, attempt, error = %err, "upstream check failed");
                }
            }
        }
    }

    None
}
