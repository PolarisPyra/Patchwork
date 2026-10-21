//! Runs each agent in its own systemd scope, under one slice with a memory cap.
//! When an agent's work exhausts memory, systemd then stops that agent alone
//! instead of the app, whose cgroup would otherwise hold every agent's memory.
//! Each build of the app has its own slice inside that one, so it can stop the
//! agents a crashed run of itself left behind without touching another build's.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

const SLICE: &str = "app-patchwork-agents.slice";

/// This build's slice inside `SLICE`, named after the AppImage or executable path.
fn build_slice() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| {
        let path = std::env::var_os("APPIMAGE")
            .map(PathBuf::from)
            .or_else(|| std::env::current_exe().ok())
            .unwrap_or_default();
        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        // A dash in a slice name nests it: this one sits inside `SLICE`.
        format!(
            "{}-{:016x}.slice",
            SLICE.trim_end_matches(".slice"),
            hasher.finish()
        )
    })
}

/// `systemd-run` set to start the command that follows in a new scope under this build's
/// slice. The command keeps systemd-run's pid, so signals and waits reach the agent
/// itself. A scope stops as a whole when the kernel kills one of its processes for memory,
/// and stopping it sends SIGKILL to whatever is left after 5 seconds.
fn scope_command(run: &Path) -> Command {
    let mut cmd = Command::new(run);
    cmd.args(["--user", "--scope", "--quiet", "--collect"])
        .arg(format!("--slice={}", build_slice()))
        .args([
            "--property=OOMPolicy=stop",
            "--property=TimeoutStopSec=5s",
            "--",
        ]);
    cmd
}

/// Sets up agent scopes ahead of the first turn, stopping agents that a previous run of
/// this build left running when it crashed or was killed.
pub fn prepare() {
    systemd_run();
}

/// `systemd-run`, once it has started a test scope and the slice's limit is set.
/// None without a systemd user manager; agents then start directly.
fn systemd_run() -> Option<&'static PathBuf> {
    static RUN: OnceLock<Option<PathBuf>> = OnceLock::new();
    RUN.get_or_init(|| {
        std::env::var_os("XDG_RUNTIME_DIR")?;
        let run = which::which("systemd-run").ok()?;
        systemctl(&["stop", build_slice()]);
        let started = scope_command(&run)
            .arg("true")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !started {
            log::info!("agent scopes are off: systemd-run could not start a scope");
            return None;
        }
        limit_slice();
        Some(run)
    })
    .as_ref()
}

/// Caps the agents together at a quarter of RAM before they are throttled and three
/// eighths before systemd kills one. Agents get no swap, so they cannot push the rest of
/// the desktop into swap.
fn limit_slice() {
    let Some(total) = total_memory() else {
        return;
    };
    let high = format!("MemoryHigh={}", total / 4);
    let max = format!("MemoryMax={}", total / 8 * 3);
    if !systemctl(&[
        "set-property",
        "--runtime",
        SLICE,
        &high,
        &max,
        "MemorySwapMax=0",
    ]) {
        log::warn!("could not set the memory limit on {SLICE}");
    }
}

/// Runs `systemctl --user` with `args`; whether it succeeded.
fn systemctl(args: &[&str]) -> bool {
    Command::new("systemctl")
        .arg("--user")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Total RAM in bytes, from /proc/meminfo.
fn total_memory() -> Option<u64> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = info.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

/// `cmd` run in a new scope under the agents' slice, or `cmd` itself when scopes are
/// unavailable. Arguments added afterwards still go to the agent.
pub fn scoped(cmd: Command) -> Command {
    let Some(run) = systemd_run() else {
        return cmd;
    };
    let mut scoped = scope_command(run);
    scoped.arg(cmd.get_program()).args(cmd.get_args());
    for (key, value) in cmd.get_envs() {
        match value {
            Some(value) => scoped.env(key, value),
            None => scoped.env_remove(key),
        };
    }
    if let Some(dir) = cmd.get_current_dir() {
        scoped.current_dir(dir);
    }
    scoped
}
