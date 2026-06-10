use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrafficSnapshot {
    pub upload_bytes: u64,
    pub download_bytes: u64,
}

#[derive(Debug)]
pub struct TrafficCounter {
    path: PathBuf,
    upload_bytes: AtomicU64,
    download_bytes: AtomicU64,
}

impl TrafficCounter {
    pub fn init(path: impl AsRef<Path>) -> Result<Arc<Self>> {
        let path = path.as_ref().to_path_buf();
        let counter = Arc::new(Self {
            path,
            upload_bytes: AtomicU64::new(0),
            download_bytes: AtomicU64::new(0),
        });

        counter.save()?;
        Ok(counter)
    }

    pub fn add(&self, upload_bytes: u64, download_bytes: u64) -> Result<()> {
        self.upload_bytes.fetch_add(upload_bytes, Ordering::Relaxed);
        self.download_bytes
            .fetch_add(download_bytes, Ordering::Relaxed);
        self.save()
    }

    fn snapshot(&self) -> TrafficSnapshot {
        TrafficSnapshot {
            upload_bytes: self.upload_bytes.load(Ordering::Relaxed),
            download_bytes: self.download_bytes.load(Ordering::Relaxed),
        }
    }

    fn save(&self) -> Result<()> {
        let content = toml::to_string_pretty(&self.snapshot())
            .context("failed to serialize traffic stats")?;
        fs::write(&self.path, content)
            .with_context(|| format!("failed to write traffic stats to {}", self.path.display()))
    }
}

pub fn read(path: impl AsRef<Path>) -> Result<TrafficSnapshot> {
    Ok(read_snapshot(path.as_ref())?.unwrap_or_default())
}

fn read_snapshot(path: &Path) -> Result<Option<TrafficSnapshot>> {
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read traffic stats from {}", path.display()))?;

    if content.trim().is_empty() {
        return Ok(Some(TrafficSnapshot::default()));
    }

    let snapshot = toml::from_str(&content)
        .with_context(|| format!("failed to parse traffic stats from {}", path.display()))?;
    Ok(Some(snapshot))
}
