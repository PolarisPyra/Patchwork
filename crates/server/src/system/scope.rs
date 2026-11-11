//! Runs each agent in its own systemd scope, under one slice with a memory cap.
//! When an agent's work exhausts memory, systemd then stops that agent alone
//! instead of the app, whose cgroup would otherwise hold every agent's memory.
//! Each build of the app has its own slice inside that one, so it can stop the
//! agents a crashed run of itself left behind without touching another build's.

// Rust guideline compliant 2026-10-09

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

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
/// slice. The command keeps systemd-run's pid, so signals and waits reach it itself, and its
/// arguments reach it as they are: systemd 254 and later would otherwise expand each `$NAME`
/// in them. A scope stops as a whole when the kernel kills one of its processes for memory,
/// and stopping it sends SIGKILL to whatever is left after `stop_after`.
fn scope_command(run: &SystemdRun, unit: Option<&str>, stop_after: &str) -> Command {
    let mut cmd = Command::new(&run.path);
    cmd.args(["--user", "--scope", "--quiet", "--collect"])
        .args(run.literal.then_some(LITERAL_ARGV))
        .arg(format!("--slice={}", build_slice()))
        .args(unit.map(|u| format!("--unit={u}")))
        .arg("--property=OOMPolicy=stop")
        .arg(format!("--property=TimeoutStopSec={stop_after}"))
        .arg("--");
    cmd
}

/// Keeps systemd-run from expanding `$NAME` in the command it starts. Versions before 254 reject
/// it, and start a scope's command as it is given anyway.
const LITERAL_ARGV: &str = "--expand-environment=no";

/// `systemd-run`, and whether it takes [`LITERAL_ARGV`].
struct SystemdRun {
    path: PathBuf,
    literal: bool,
}

/// Sets up agent scopes ahead of the first turn, stopping the agents and commands that a
/// previous run of this build left running when it crashed or was killed.
pub fn prepare() {
    systemd_run();
}

/// `systemd-run`, once it has started a test scope and the slice's limit is set.
/// None without a systemd user manager; agents and commands then start directly.
fn systemd_run() -> Option<&'static SystemdRun> {
    static RUN: OnceLock<Option<SystemdRun>> = OnceLock::new();
    RUN.get_or_init(|| {
        std::env::var_os("XDG_RUNTIME_DIR")?;
        let path = which::which("systemd-run").ok()?;
        // An agent's own process, which runs its hooks, sits in the slice it would stop.
        if !in_agent_scope() {
            systemctl(&["stop", build_slice()]);
        }
        let starts = |literal| {
            let run = SystemdRun {
                path: path.clone(),
                literal,
            };
            let started = scope_command(&run, None, "5s")
                .arg("true")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            started.then_some(run)
        };
        let Some(run) = starts(true).or_else(|| starts(false)) else {
            log::info!("agent scopes are off: systemd-run could not start a scope");
            return None;
        };
        limit_slice();
        // Kills from before this run are not this run's agents'.
        if let Some(now) = memory_kills() {
            MEMORY_KILLS_SEEN.fetch_max(now, Ordering::Relaxed);
        }
        Some(run)
    })
    .as_ref()
}

/// Whether this process runs in an agent's scope, as Patchwork's own agent does.
fn in_agent_scope() -> bool {
    std::fs::read_to_string("/proc/self/cgroup")
        .is_ok_and(|cgroup| cgroup.contains("/patchwork-agent-"))
}

/// Caps the agents together so they leave about 6 GiB free before they are throttled and
/// 4 GiB before the kernel kills one; small machines still give them half of RAM before
/// throttling and three quarters before a kill. Throttling stalls the whole user session,
/// and systemd-oomd kills an agent once that stall lasts, so it has to start late. Swap
/// stays allowed: files agents leave in a tmpfs /tmp stay charged to the slice after they
/// exit, and without swap that memory could never be reclaimed.
fn limit_slice() {
    const GIB: u64 = 1 << 30;
    let Some(total) = total_memory() else {
        return;
    };
    let high = format!(
        "MemoryHigh={}",
        total.saturating_sub(6 * GIB).max(total / 2)
    );
    let max = format!(
        "MemoryMax={}",
        total.saturating_sub(4 * GIB).max(total / 4 * 3)
    );
    if !systemctl(&[
        "set-property",
        "--runtime",
        SLICE,
        &high,
        &max,
        // Lifts the no-swap limit that earlier builds set on the slice for this boot.
        "MemorySwapMax=infinity",
    ]) {
        log::warn!(slice = SLICE; "could not set the memory limit on {{slice}}");
    }
}

/// Stops the agent's scope with whatever is still running in it, such as commands the agent
/// started in sessions of their own, which its process group does not reach.
/// Whether the stop was queued: false when the scope has ended, or is not set up yet.
pub fn stop(unit: &str) -> bool {
    systemctl(&["stop", "--no-block", unit])
}

/// Whether anything still runs in the scope `unit`.
#[cfg(unix)]
pub fn active(unit: &str) -> bool {
    systemctl(&["is-active", "--quiet", unit])
}

/// Runs `systemctl --user` with `args`; whether it succeeded.
fn systemctl(args: &[&str]) -> bool {
    crate::system::host_command("systemctl")
        .arg("--user")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// How many processes the kernel has killed for memory in the agents' slice since boot, as the
/// slice's cgroup counts them, with the kills in every scope under it. None where there is no
/// such count: without systemd, or off Linux.
fn memory_kills() -> Option<u64> {
    static EVENTS: OnceLock<Option<PathBuf>> = OnceLock::new();
    let events = EVENTS
        .get_or_init(|| {
            let shown = crate::system::host_command("systemctl")
                .args([
                    "--user",
                    "show",
                    "--property=ControlGroup",
                    "--value",
                    SLICE,
                ])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .ok()?;
            let group = String::from_utf8(shown.stdout).ok()?;
            let group = group.trim().trim_start_matches('/');
            (!group.is_empty()).then(|| {
                PathBuf::from("/sys/fs/cgroup")
                    .join(group)
                    .join("memory.events")
            })
        })
        .as_ref()?;
    let text = std::fs::read_to_string(events).ok()?;
    let line = text.lines().find(|l| l.starts_with("oom_kill "))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// The kernel's memory kills among the agents that have been accounted for.
static MEMORY_KILLS_SEEN: AtomicU64 = AtomicU64::new(0);

/// Whether the kernel has killed a process of an agent for memory since this was last asked, or
/// since the scopes were set up. An agent that ends on a signal ran out of memory only when this
/// says so: other things send the same signal.
pub fn killed_for_memory() -> bool {
    memory_kills().is_some_and(|now| MEMORY_KILLS_SEEN.fetch_max(now, Ordering::Relaxed) < now)
}

/// Total RAM in bytes, from /proc/meminfo.
fn total_memory() -> Option<u64> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = info.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

/// `cmd` run in a new scope under the agents' slice, with the scope's unit name, or `cmd`
/// itself when scopes are unavailable. Arguments added afterwards still go to the agent.
pub fn scoped(cmd: Command) -> (Command, Option<String>) {
    in_scope(cmd, "agent", "5s")
}

/// `cmd` run in a new scope of its own, as [`scoped`] runs an agent, for a command an agent, a
/// hook or a check runs. Stopping it sends SIGKILL to whatever is left after 3 seconds.
pub fn command_scope(cmd: Command) -> (Command, Option<String>) {
    in_scope(cmd, "command", "3s")
}

fn in_scope(mut cmd: Command, kind: &str, stop_after: &str) -> (Command, Option<String>) {
    let Some(run) = systemd_run() else {
        raise_oom_score(&mut cmd);
        return (cmd, None);
    };
    let unit = format!("patchwork-{kind}-{}.scope", uuid::Uuid::new_v4().simple());
    let mut scoped = scope_command(run, Some(&unit), stop_after);
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
    raise_oom_score(&mut scoped);
    (scoped, Some(unit))
}

/// Makes the kernel kill agents before the app or the desktop when memory runs out.
/// Everything the agent starts inherits the score.
fn raise_oom_score(cmd: &mut Command) {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: the hook runs between fork and exec, and only calls open, write and
        // close, which are async-signal-safe and allocate nothing.
        unsafe {
            cmd.pre_exec(|| {
                let fd = libc::open(c"/proc/self/oom_score_adj".as_ptr(), libc::O_WRONLY);
                if fd >= 0 {
                    libc::write(fd, b"800".as_ptr().cast(), 3);
                    libc::close(fd);
                }
                Ok(())
            });
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = cmd;
}
