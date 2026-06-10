use anyhow::Result;
use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    sync::{Mutex, RwLock},
    time,
};

use crate::{checker, config::AppConfig, fofa};

const ONLINE_TARGET: usize = 80;
const CANDIDATE_LOW_WATERMARK: usize = 200;
const CANDIDATE_REFILL_LIMIT: usize = 1000;
const BOOTSTRAP_CHECK_LIMIT: usize = 100;
const CHECK_CONCURRENCY: usize = 32;
const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
const CHECK_CACHE_TTL: Duration = Duration::from_secs(10);
const MAINTAIN_INTERVAL: Duration = Duration::from_secs(10);
pub const FOFA_DAYS: i64 = 30;
const FOFA_PAGE_SIZE: u32 = 1000;

#[derive(Debug, Clone)]
pub struct ProxyPool {
    inner: Arc<Mutex<PoolData>>,
    online_path: PathBuf,
    candidates_path: PathBuf,
    fofa_state_path: PathBuf,
}

#[derive(Debug, Default)]
struct PoolData {
    online: Vec<String>,
    candidates: Vec<String>,
    last_checked: HashMap<String, Instant>,
    cursor: usize,
}

impl ProxyPool {
    pub async fn init(
        config: &AppConfig,
        online_path: impl AsRef<Path>,
        candidates_path: impl AsRef<Path>,
        fofa_state_path: impl AsRef<Path>,
    ) -> Result<Self> {
        let online_path = online_path.as_ref().to_path_buf();
        let candidates_path = candidates_path.as_ref().to_path_buf();
        let fofa_state_path = fofa_state_path.as_ref().to_path_buf();
        let online = read_existing(&online_path)?;
        let mut candidates = read_existing(&candidates_path)?;

        if candidates.len() < CANDIDATE_LOW_WATERMARK {
            let fetched = fofa::fetch_hosts_with_state(
                config,
                &fofa_state_path,
                FOFA_DAYS,
                FOFA_PAGE_SIZE,
                CANDIDATE_REFILL_LIMIT,
            )
            .await?;
            candidates = merge_hosts(candidates, fetched.hosts);
            checker::write_upstreams(&candidates_path, &candidates)?;
        }

        let pool = Self {
            inner: Arc::new(Mutex::new(PoolData {
                online,
                candidates,
                last_checked: HashMap::new(),
                cursor: 0,
            })),
            online_path,
            candidates_path,
            fofa_state_path,
        };

        if pool.online_len().await == 0 {
            pool.promote_candidates(config, ONLINE_TARGET).await?;
        }

        Ok(pool)
    }

    pub fn spawn_maintainers(&self, config: Arc<RwLock<AppConfig>>) {
        let online_pool = self.clone();
        let online_config = config.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(MAINTAIN_INTERVAL);
            loop {
                interval.tick().await;
                let config = online_config.read().await.clone();
                if let Err(err) = online_pool.maintain_online(&config).await {
                    tracing::warn!(error = %err, "failed to maintain online proxy pool");
                }
            }
        });

        let candidate_pool = self.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(MAINTAIN_INTERVAL);
            loop {
                interval.tick().await;
                let config = config.read().await.clone();
                if let Err(err) = candidate_pool.maintain_candidates(&config).await {
                    tracing::warn!(error = %err, "failed to maintain candidate proxy pool");
                }
            }
        });
    }

    pub async fn select_upstream(
        &self,
        check_urls: &[String],
        max_latency: Duration,
    ) -> Result<Option<String>> {
        let attempts = self.online_len().await;

        for _ in 0..attempts {
            let Some(upstream) = self.next_online().await else {
                return Ok(None);
            };

            if !self.needs_check(&upstream).await {
                return Ok(Some(upstream));
            }

            if checker::check_one(&upstream, check_urls, CHECK_TIMEOUT, max_latency)
                .await
                .is_some()
            {
                self.mark_checked(&upstream).await;
                return Ok(Some(upstream));
            }

            self.remove_online(&upstream).await?;
        }

        Ok(None)
    }

    pub async fn mark_failure(&self, upstream: &str) -> Result<()> {
        self.remove_online(upstream).await
    }

    async fn maintain_online(&self, config: &AppConfig) -> Result<()> {
        let expired = {
            let inner = self.inner.lock().await;
            let now = Instant::now();
            inner
                .online
                .iter()
                .filter(|upstream| {
                    inner
                        .last_checked
                        .get(*upstream)
                        .is_none_or(|checked_at| now.duration_since(*checked_at) >= CHECK_CACHE_TTL)
                })
                .cloned()
                .collect::<Vec<_>>()
        };

        let alive = checker::check_hosts(
            expired.clone(),
            &check_urls(config),
            CHECK_CONCURRENCY,
            CHECK_TIMEOUT,
            max_latency(config),
        )
        .await?;
        let alive = alive
            .into_iter()
            .map(|upstream| upstream.host)
            .collect::<BTreeSet<_>>();

        {
            let mut inner = self.inner.lock().await;
            let now = Instant::now();
            for upstream in expired {
                if alive.contains(&upstream) {
                    inner.last_checked.insert(upstream, now);
                } else {
                    inner.online.retain(|item| item != &upstream);
                    inner.last_checked.remove(&upstream);
                }
            }
            write_pool(&self.online_path, &inner.online)?;
        }

        let missing = ONLINE_TARGET.saturating_sub(self.online_len().await);
        if missing > 0 {
            self.promote_candidates(config, missing).await?;
        }

        Ok(())
    }

    async fn maintain_candidates(&self, config: &AppConfig) -> Result<()> {
        let current_len = {
            let inner = self.inner.lock().await;
            inner.candidates.len()
        };

        if current_len >= CANDIDATE_LOW_WATERMARK {
            return Ok(());
        }

        let fetched = fofa::fetch_hosts_with_state(
            config,
            &self.fofa_state_path,
            FOFA_DAYS,
            FOFA_PAGE_SIZE,
            CANDIDATE_REFILL_LIMIT,
        )
        .await?;
        let mut inner = self.inner.lock().await;
        let online = inner.online.iter().cloned().collect::<BTreeSet<_>>();
        let merged = merge_hosts(inner.candidates.clone(), fetched.hosts);
        inner.candidates = merged
            .into_iter()
            .filter(|upstream| !online.contains(upstream))
            .collect();
        write_pool(&self.candidates_path, &inner.candidates)?;

        Ok(())
    }

    async fn promote_candidates(&self, config: &AppConfig, needed: usize) -> Result<()> {
        if needed == 0 {
            return Ok(());
        }

        let test_batch = {
            let inner = self.inner.lock().await;
            inner
                .candidates
                .iter()
                .take(BOOTSTRAP_CHECK_LIMIT.max(needed))
                .cloned()
                .collect::<Vec<_>>()
        };

        if test_batch.is_empty() {
            self.maintain_candidates(config).await?;
            return Ok(());
        }

        let alive = checker::check_hosts(
            test_batch.clone(),
            &check_urls(config),
            CHECK_CONCURRENCY,
            CHECK_TIMEOUT,
            max_latency(config),
        )
        .await?;
        let tested = test_batch.into_iter().collect::<BTreeSet<_>>();

        let mut inner = self.inner.lock().await;
        let mut online = inner.online.iter().cloned().collect::<BTreeSet<_>>();
        let now = Instant::now();
        for upstream in alive.into_iter().map(|upstream| upstream.host).take(needed) {
            if online.insert(upstream.clone()) {
                inner.online.push(upstream.clone());
                inner.last_checked.insert(upstream, now);
            }
        }
        inner
            .candidates
            .retain(|upstream| !tested.contains(upstream));
        write_pool(&self.online_path, &inner.online)?;
        write_pool(&self.candidates_path, &inner.candidates)?;

        Ok(())
    }

    async fn online_len(&self) -> usize {
        self.inner.lock().await.online.len()
    }

    async fn next_online(&self) -> Option<String> {
        let mut inner = self.inner.lock().await;
        if inner.online.is_empty() {
            return None;
        }

        let upstream = inner.online[inner.cursor % inner.online.len()].clone();
        inner.cursor = inner.cursor.wrapping_add(1);
        Some(upstream)
    }

    async fn needs_check(&self, upstream: &str) -> bool {
        let inner = self.inner.lock().await;
        inner
            .last_checked
            .get(upstream)
            .is_none_or(|checked_at| Instant::now().duration_since(*checked_at) >= CHECK_CACHE_TTL)
    }

    async fn mark_checked(&self, upstream: &str) {
        self.inner
            .lock()
            .await
            .last_checked
            .insert(upstream.to_string(), Instant::now());
    }

    async fn remove_online(&self, upstream: &str) -> Result<()> {
        let mut inner = self.inner.lock().await;
        inner.online.retain(|item| item != upstream);
        inner.last_checked.remove(upstream);
        write_pool(&self.online_path, &inner.online)
    }
}

fn max_latency(config: &AppConfig) -> Duration {
    Duration::from_millis(config.max_latency.max(1))
}

pub fn check_urls(config: &AppConfig) -> Vec<String> {
    [config.check_url.clone(), config.check_fallback_url.clone()]
        .into_iter()
        .filter(|url| !url.trim().is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn read_existing(path: &Path) -> Result<Vec<String>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    checker::read_upstreams(path)
}

fn write_pool(path: &Path, hosts: &[String]) -> Result<()> {
    checker::write_upstreams(path, hosts)
}

fn merge_hosts(current: Vec<String>, fetched: Vec<String>) -> Vec<String> {
    current
        .into_iter()
        .chain(fetched)
        .filter(|host| !host.trim().is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
