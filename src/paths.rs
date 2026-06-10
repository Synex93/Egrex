use anyhow::{Context, Result};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

const DATA_DIR_NAME: &str = ".egrex";

#[derive(Debug, Clone)]
pub struct RuntimePaths {
    pub data_dir: PathBuf,
    pub config: PathBuf,
    pub pid_lock: PathBuf,
    pub stop_lock: PathBuf,
    pub traffic: PathBuf,
    pub online: PathBuf,
    pub candidates: PathBuf,
    pub fofa_state: PathBuf,
}

impl RuntimePaths {
    pub fn init() -> Result<Self> {
        let data_dir = data_dir()?;
        fs::create_dir_all(&data_dir)
            .with_context(|| format!("failed to create data directory {}", data_dir.display()))?;

        let paths = Self {
            config: data_dir.join("config.toml"),
            pid_lock: data_dir.join("pid.lock"),
            stop_lock: data_dir.join("stop.lock"),
            traffic: data_dir.join("traffic.lock"),
            online: data_dir.join("online.lock"),
            candidates: data_dir.join("candidates.lock"),
            fofa_state: data_dir.join("fofa_state.toml"),
            data_dir,
        };

        paths.migrate_legacy_files()?;
        Ok(paths)
    }

    fn migrate_legacy_files(&self) -> Result<()> {
        migrate_if_missing("config.toml", &self.config)?;
        migrate_if_missing("traffic.lock", &self.traffic)?;
        migrate_if_missing("online.lock", &self.online)?;
        migrate_if_missing("candidates.lock", &self.candidates)?;
        Ok(())
    }
}

fn data_dir() -> Result<PathBuf> {
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .context("failed to resolve home directory from HOME or USERPROFILE")?;

    Ok(home.join(DATA_DIR_NAME))
}

fn migrate_if_missing(legacy_name: &str, target: &Path) -> Result<()> {
    if target.exists() {
        return Ok(());
    }

    let legacy = Path::new(legacy_name);
    if !legacy.exists() {
        return Ok(());
    }

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    fs::copy(legacy, target).with_context(|| {
        format!(
            "failed to migrate {} to {}",
            legacy.display(),
            target.display()
        )
    })?;

    Ok(())
}
