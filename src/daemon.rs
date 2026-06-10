use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

#[derive(Debug, Serialize, Deserialize)]
struct PidLock {
    pid: u32,
    listen_addr: Option<String>,
}

pub fn start(lock_path: impl AsRef<Path>, listen_addr: String) -> Result<u32> {
    let lock_path = lock_path.as_ref();

    if let Some(lock) = read_lock(lock_path)? {
        let pid = lock.pid;
        if process_is_running(pid) {
            bail!("egrex is already running with pid {pid}");
        }

        fs::remove_file(lock_path)
            .with_context(|| format!("failed to remove stale {}", lock_path.display()))?;
    }

    let exe = std::env::current_exe().context("failed to resolve current executable")?;
    let mut command = Command::new(exe);
    command
        .arg("run")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    configure_background_process(&mut command);

    let mut child = command
        .spawn()
        .context("failed to start egrex in background")?;
    let pid = child.id();

    thread::sleep(Duration::from_millis(300));
    if let Some(status) = child
        .try_wait()
        .context("failed to check background process status")?
    {
        bail!("background process exited immediately with status {status}");
    }

    write_lock(
        lock_path,
        &PidLock {
            pid,
            listen_addr: Some(listen_addr),
        },
    )?;
    Ok(pid)
}

pub fn status(lock_path: impl AsRef<Path>) -> Result<Status> {
    let lock_path = lock_path.as_ref();

    let Some(lock) = read_lock(lock_path)? else {
        return Ok(Status::NotStarted);
    };
    let pid = lock.pid;

    if process_is_running(pid) {
        Ok(Status::Running {
            pid,
            listen_addr: lock.listen_addr,
        })
    } else {
        Ok(Status::Stale {
            pid,
            listen_addr: lock.listen_addr,
        })
    }
}

pub fn update_listen_addr(lock_path: impl AsRef<Path>, listen_addr: String) -> Result<()> {
    let lock_path = lock_path.as_ref();
    let Some(mut lock) = read_lock(lock_path)? else {
        return Ok(());
    };

    lock.listen_addr = Some(listen_addr);
    write_lock(lock_path, &lock)
}

pub fn stop(
    lock_path: impl AsRef<Path>,
    stop_path: impl AsRef<Path>,
    force: bool,
) -> Result<StopResult> {
    let lock_path = lock_path.as_ref();
    let stop_path = stop_path.as_ref();

    let Some(lock) = read_lock(lock_path)? else {
        return Ok(StopResult::NotStarted);
    };
    let pid = lock.pid;

    if !process_is_running(pid) {
        fs::remove_file(lock_path)
            .with_context(|| format!("failed to remove stale {}", lock_path.display()))?;
        return Ok(StopResult::StaleRemoved { pid });
    }

    if force {
        terminate_process(pid, true)?;
    } else {
        request_graceful_stop(stop_path)?;
    }

    for _ in 0..100 {
        if !process_is_running(pid) {
            remove_if_exists(stop_path)?;
            fs::remove_file(lock_path)
                .with_context(|| format!("failed to remove {}", lock_path.display()))?;
            return Ok(StopResult::Stopped { pid, force });
        }

        thread::sleep(Duration::from_millis(100));
    }

    bail!(
        "process {pid} did not stop after graceful stop request, retry with `egrex stop --force`"
    );
}

#[derive(Debug)]
pub enum Status {
    Running {
        pid: u32,
        listen_addr: Option<String>,
    },
    Stale {
        pid: u32,
        listen_addr: Option<String>,
    },
    NotStarted,
}

#[derive(Debug)]
pub enum StopResult {
    Stopped { pid: u32, force: bool },
    StaleRemoved { pid: u32 },
    NotStarted,
}

pub fn shutdown_requested(stop_path: impl AsRef<Path>) -> bool {
    stop_path.as_ref().exists()
}

pub fn clear_shutdown_request(stop_path: impl AsRef<Path>) -> Result<()> {
    remove_if_exists(stop_path.as_ref())
}

fn read_lock(path: &Path) -> Result<Option<PidLock>> {
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read pid lock from {}", path.display()))?;
    let lock: PidLock = toml::from_str(&content)
        .with_context(|| format!("failed to parse pid lock from {}", path.display()))?;

    Ok(Some(lock))
}

fn write_lock(path: &Path, lock: &PidLock) -> Result<()> {
    let content = toml::to_string_pretty(lock).context("failed to serialize pid lock")?;
    fs::write(path, content)
        .with_context(|| format!("failed to write pid lock to {}", path.display()))
}

fn request_graceful_stop(path: &Path) -> Result<()> {
    fs::write(path, "stop\n")
        .with_context(|| format!("failed to write stop request to {}", path.display()))
}

fn remove_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path).with_context(|| format!("failed to remove {}", path.display()))?;
    }

    Ok(())
}

#[cfg(windows)]
fn configure_background_process(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    command.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn configure_background_process(_command: &mut Command) {}

#[cfg(windows)]
fn process_is_running(pid: u32) -> bool {
    let filter = format!("PID eq {pid}");
    let Ok(output) = Command::new("tasklist")
        .args(["/FI", &filter, "/FO", "CSV", "/NH"])
        .output()
    else {
        return false;
    };

    if !output.status.success() {
        return false;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().any(|line| {
        line.split(',')
            .nth(1)
            .is_some_and(|value| value.trim_matches('"') == pid.to_string())
    })
}

#[cfg(windows)]
fn terminate_process(pid: u32, force: bool) -> Result<()> {
    let force = if force { Some("/F") } else { None };
    let pid = pid.to_string();
    let mut args = vec!["/PID", pid.as_str()];
    if let Some(force) = force {
        args.push(force);
    }

    let status = Command::new("taskkill")
        .args(args)
        .status()
        .context("failed to run taskkill")?;

    if !status.success() {
        bail!("taskkill failed with status {status}");
    }

    Ok(())
}

#[cfg(not(windows))]
fn process_is_running(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(not(windows))]
fn terminate_process(pid: u32, _force: bool) -> Result<()> {
    let status = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .context("failed to run kill")?;

    if !status.success() {
        bail!("kill failed with status {status}");
    }

    Ok(())
}
