//! Asking an agent CLI about itself: running it briefly with a time limit, and reading its version, sign-in and models.

use std::io::Read;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use crate::workspace::{signal_process_group, SIGKILL};
use super::{antigravity, api, claude, codex, opencode, AgentKind, AgentStatus, ModelOption};
use super::{agent_command, located};

/// Upper bound for any single agent CLI probe (`--version`, `auth status`, ...).
pub(super) const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Probe {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Runs the agent CLI with `args`. Returns None if it cannot start or exceeds
/// `PROBE_TIMEOUT` (the process group is killed in that case).
pub fn probe(kind: AgentKind, args: &[&str]) -> Option<Probe> {
    probe_until(kind, args, |_| false)
}

/// Like `probe`, but also ends the process once `done` accepts the stdout so far, and returns
/// that output as a success. Some CLIs print their answer and then keep running, so waiting
/// for them to exit would only ever reach the timeout.
pub fn probe_until(kind: AgentKind, args: &[&str], done: impl Fn(&str) -> bool) -> Option<Probe> {
    let cmd = agent_command(kind).ok()?;
    run_probe(cmd, args, PROBE_TIMEOUT, done)
}

/// Like `probe`, but runs in `dir` and allows `timeout`.
pub fn probe_in(kind: AgentKind, args: &[&str], dir: &Path, timeout: Duration) -> Option<Probe> {
    let mut cmd = agent_command(kind).ok()?;
    cmd.current_dir(dir);
    run_probe(cmd, args, timeout, |_| false)
}

pub(crate) fn run_probe(
    mut cmd: Command,
    args: &[&str],
    timeout: Duration,
    done: impl Fn(&str) -> bool,
) -> Option<Probe> {
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd.spawn().ok()?;
    let (out, out_reader) = read_pipe(child.stdout.take()?);
    let (err, err_reader) = read_pipe(child.stderr.take()?);

    let deadline = Instant::now() + timeout;
    let success = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Kill leftover group members so a grandchild cannot hold the pipes open.
                signal_process_group(child.id(), SIGKILL);
                let _ = out_reader.join();
                let _ = err_reader.join();
                break status.success();
            }
            Ok(None) if done(&text(&out)) => {
                stop(&mut child);
                break true;
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                stop(&mut child);
                return None;
            }
        }
    };
    Some(Probe {
        success,
        stdout: text(&out),
        stderr: text(&err),
    })
}

/// Kills the CLI and anything it started in its process group.
pub(super) fn stop(child: &mut Child) {
    signal_process_group(child.id(), SIGKILL);
    let _ = child.kill();
    let _ = child.wait();
}

/// Copies a pipe into a shared buffer on its own thread, so a probe can read what has arrived
/// without waiting for the pipe to close.
pub(super) fn read_pipe(pipe: impl Read + Send + 'static) -> (Arc<Mutex<Vec<u8>>>, JoinHandle<()>) {
    read_pipe_upto(pipe, usize::MAX)
}

/// Reads a pipe to its end, keeping at most about `limit` bytes: past that, the start and the
/// latest output are kept with a note between them, and the middle is dropped.
pub(crate) fn read_pipe_upto(
    mut pipe: impl Read + Send + 'static,
    limit: usize,
) -> (Arc<Mutex<Vec<u8>>>, JoinHandle<()>) {
    const CUT: &[u8] = b"\n[... output cut ...]\n";
    let buf = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&buf);
    let handle = std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        while let Ok(n) = pipe.read(&mut chunk) {
            if n == 0 {
                break;
            }
            let mut buf = sink.lock().unwrap_or_else(PoisonError::into_inner);
            buf.extend_from_slice(&chunk[..n]);
            // Cut only once the buffer is twice the limit, so the copy happens rarely.
            if buf.len() > limit.saturating_mul(2) {
                let end = buf.len() - limit / 2;
                buf.splice(limit / 2..end, CUT.iter().copied());
            }
        }
    });
    (buf, handle)
}

pub(crate) fn text(buf: &Mutex<Vec<u8>>) -> String {
    let bytes = buf.lock().unwrap_or_else(PoisonError::into_inner);
    String::from_utf8_lossy(&bytes).into_owned()
}

pub fn detect_agent(kind: AgentKind) -> AgentStatus {
    let mut status = AgentStatus {
        kind,
        installed: false,
        version: None,
        signed_in: false,
        detail: "Not installed".into(),
    };
    // API providers run in Patchwork itself, so there is no CLI to probe.
    if kind == AgentKind::Api {
        api::auth(&mut status);
        return status;
    }
    let Some(version) = probe(kind, &["--version"]).filter(|p| p.success) else {
        if let Some(path) = located(kind) {
            status.detail = if path.is_file() {
                format!("Could not run {}", path.display())
            } else {
                format!("No file at {}", path.display())
            };
        }
        return status;
    };
    status.installed = true;
    status.version = version_in(&version.stdout);
    match kind {
        AgentKind::Claude => claude::auth(&mut status),
        AgentKind::Codex => codex::auth(&mut status),
        AgentKind::Antigravity => antigravity::auth(&mut status),
        AgentKind::OpenCode => opencode::auth(&mut status),
        AgentKind::Api => {}
    }
    status
}

/// The CLI's version, as `--version` prints it.
pub fn cli_version(kind: AgentKind) -> Option<String> {
    probe(kind, &["--version"])
        .filter(|p| p.success)
        .and_then(|p| version_in(&p.stdout))
}

/// "2.1.303 (Claude Code)", "codex-cli 0.163.0" and "0.64.0" all carry the version as a numeric word.
fn version_in(stdout: &str) -> Option<String> {
    stdout
        .split_whitespace()
        .map(|w| w.trim_start_matches('v'))
        .find(|w| w.starts_with(|c: char| c.is_ascii_digit()))
        .map(String::from)
}

/// Models offered for `kind`, starting with the CLI default.
pub fn list_models(kind: AgentKind) -> Vec<ModelOption> {
    let (default_model, models) = match kind {
        AgentKind::Claude => (claude::configured_model(), claude::models()),
        AgentKind::Codex => (codex::configured_model(), codex::models()),
        AgentKind::Antigravity => (None, antigravity::models()),
        AgentKind::OpenCode => (None, opencode::models()),
        AgentKind::Api => (
            crate::provider::api::config::default_model(),
            crate::provider::api::config::models(),
        ),
    };
    let default_model = default_model.filter(|m| !m.trim().is_empty());
    // The default holds what the model it names holds, when that model is listed.
    let context_window = default_model.as_deref().and_then(|id| {
        models
            .iter()
            .find(|m| m.id.as_deref() == Some(id))?
            .context_window
    });
    let mut out = vec![ModelOption {
        id: None,
        label: "Default".into(),
        default_model,
        is_legacy: false,
        efforts: None,
        context_window,
    }];
    out.extend(models);
    out
}
