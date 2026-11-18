//! Runs a command where some folders are read-only to it, with bubblewrap on Linux.
//!
//! The command sees the whole file system as it is, so its tools, settings and caches keep
//! working; only the folders it is told to leave alone refuse its writes. Network, processes
//! and devices are shared with the app.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// `cmd`, run with each path in `read_only` read-only to it.
///
/// The paths in `writable` stay writable, and a path inside another takes its own access, so a
/// file can be read-only inside a writable folder. Paths that don't exist are left out.
/// Returns `cmd` as it is where bubblewrap is missing or can't start, which the log says once.
#[must_use]
pub fn read_only(cmd: Command, read_only: &[PathBuf], writable: &[PathBuf]) -> Command {
    let Some(bwrap) = bwrap() else {
        return cmd;
    };
    let mut wrapped = Command::new(bwrap);
    wrapped.args([
        "--bind",
        "/",
        "/",
        "--dev-bind",
        "/dev",
        "/dev",
        "--die-with-parent",
    ]);
    // A path inside another is bound after it, so it keeps its own access whatever the order
    // of the lists.
    let mut binds: Vec<(&PathBuf, &str)> = read_only
        .iter()
        .map(|p| (p, "--ro-bind"))
        .chain(writable.iter().map(|p| (p, "--bind")))
        .filter(|(p, _)| p.exists())
        .collect();
    binds.sort_by_key(|(p, _)| p.components().count());
    for (path, bind) in binds {
        wrapped.arg(bind).arg(path).arg(path);
    }
    wrapped
        .arg("--")
        .arg(cmd.get_program())
        .args(cmd.get_args());
    for (key, value) in cmd.get_envs() {
        match value {
            Some(value) => wrapped.env(key, value),
            None => wrapped.env_remove(key),
        };
    }
    if let Some(dir) = cmd.get_current_dir() {
        wrapped.current_dir(dir);
    }
    wrapped
}

/// Whether `read_only` can wrap commands here, so a caller that must not run unsandboxed can
/// refuse instead.
#[must_use]
pub fn available() -> bool {
    bwrap().is_some()
}

/// bubblewrap, once it has run a command with a read-only folder on this machine. None off
/// Linux, without bubblewrap, or where the kernel refuses it the namespaces it needs.
fn bwrap() -> Option<&'static Path> {
    static BWRAP: OnceLock<Option<PathBuf>> = OnceLock::new();
    BWRAP
        .get_or_init(|| {
            if !cfg!(target_os = "linux") {
                return None;
            }
            let Ok(path) = which::which("bwrap") else {
                log::info!("tasks run without a sandbox: bubblewrap is not installed");
                return None;
            };
            let runs = Command::new(&path)
                .args([
                    "--bind",
                    "/",
                    "/",
                    "--ro-bind",
                    "/etc",
                    "/etc",
                    "--",
                    "true",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if !runs {
                log::warn!("tasks run without a sandbox: bubblewrap could not start one");
            }
            runs.then_some(path)
        })
        .as_deref()
}
