//! Time-bounded agent CLI probes (`--version`, `auth status`, etc.).

// Rust guideline compliant 2026-10-09

use std::io::Read;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use patchwork_platform::Locked;
use patchwork_platform::processes::running::{SIGKILL, signal_process_group};

use crate::locate::command_at;

/// Upper bound for any single agent CLI probe (`--version`, `auth status`, ...).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub struct Probe {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Runs the CLI at `bin` with `args` up to `PROBE_TIMEOUT`.
#[must_use]
pub fn probe_at(bin: &Path, args: &[&str]) -> Option<Probe> {
    probe_command(command_at(bin), args, PROBE_TIMEOUT, |_| false)
}

/// Runs `cmd` with `args`, returning early if `done` accepts stdout so far, or `None` on timeout.
pub fn probe_command(
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
    let mut child = crate::command_size::spawn(&mut cmd, "an agent CLI probe").ok()?;
    let (out, out_reader) = read_pipe(child.stdout.take()?);
    let (err, err_reader) = read_pipe(child.stderr.take()?);

    let deadline = Instant::now() + timeout;
    let success = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                signal_process_group(child.id(), SIGKILL);
                settle(&out_reader);
                settle(&err_reader);
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

fn settle(reader: &JoinHandle<()>) {
    let until = Instant::now() + Duration::from_secs(2);
    while !reader.is_finished() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Kills the CLI and anything it started in its process group.
pub fn stop(child: &mut Child) {
    signal_process_group(child.id(), SIGKILL);
    let _ = child.kill();
    let _ = child.wait();
}

fn read_pipe(mut pipe: impl Read + Send + 'static) -> (Arc<Mutex<Vec<u8>>>, JoinHandle<()>) {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&buf);
    let handle = std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        while let Ok(n) = pipe.read(&mut chunk) {
            if n == 0 {
                break;
            }
            sink.locked().extend_from_slice(&chunk[..n]);
        }
    });
    (buf, handle)
}

fn text(buf: &Mutex<Vec<u8>>) -> String {
    let bytes = buf.locked();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Extracts the first numeric version word from `--version` output.
///
/// Handles formats like `"2.1.303 (Claude Code)"`, `"codex-cli 0.163.0"`, and `"0.64.0"`.
#[must_use]
pub fn version_in(stdout: &str) -> Option<String> {
    stdout
        .split_whitespace()
        .map(|w| w.trim_start_matches('v'))
        .find(|w| w.starts_with(|c: char| c.is_ascii_digit()))
        .map(String::from)
}
