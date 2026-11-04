//! The `bash` tool: a command in bash, in the environment the user's shell sets up, with a time
//! limit and its output capped.

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

pub(super) const BASH_TIMEOUT: u64 = 120;

pub(super) const MAX_BASH_TIMEOUT: u64 = 600;

/// Output a command's stdout or stderr keeps in memory.
const MAX_PIPE: usize = 1_000_000;

pub(super) fn bash(command: &str, timeout: u64, root: &Path) -> Result<String, String> {
    // The command stays in the agent's process group, so stopping the turn stops it too.
    let mut cmd = crate::system::shell_command(command)
        .ok_or_else(|| "No shell was found to run the command in".to_string())?;
    cmd.current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to run the command: {e}"))?;
    let (Some(out_pipe), Some(err_pipe)) = (child.stdout.take(), child.stderr.take()) else {
        return Err("The command's output is unavailable".into());
    };
    // Kept well above what the model is shown, so the head and tail it sees are the real ones.
    let (out, out_reader) = crate::provider::read_pipe_upto(out_pipe, MAX_PIPE);
    let (err, err_reader) = crate::provider::read_pipe_upto(err_pipe, MAX_PIPE);
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                kill_tree(child.id());
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    // A process the command left running can hold the pipes open; its output so far is kept.
    let grace = Instant::now() + Duration::from_secs(1);
    while !(out_reader.is_finished() && err_reader.is_finished()) && Instant::now() < grace {
        std::thread::sleep(Duration::from_millis(10));
    }
    let (stdout, stderr) = (crate::provider::text(&out), crate::provider::text(&err));
    let mut text = stdout.trim_end().to_string();
    if !stderr.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(stderr.trim_end());
    }
    match status {
        None => Err(
            format!("{text}\n\nThe command was stopped after {timeout} s")
                .trim_start()
                .into(),
        ),
        Some(s) if s.success() => Ok(if text.is_empty() {
            "(no output)".into()
        } else {
            text
        }),
        Some(s) => {
            let code = s.code().map_or("a signal".into(), |c| c.to_string());
            Err(format!("{text}\n\nExit code {code}").trim_start().into())
        }
    }
}

/// Kills the command at `pid` and every process it started, so a server or a watcher it left
/// running does not outlive its time limit. They share the agent's process group, so they are
/// found through /proc on Linux; Windows kills the tree with `taskkill`, and elsewhere only the
/// shell is killed.
fn kill_tree(pid: u32) {
    #[cfg(target_os = "linux")]
    {
        let mut tree = vec![pid];
        let mut at = 0;
        while let Some(&parent) = tree.get(at) {
            at += 1;
            let tasks = std::fs::read_dir(format!("/proc/{parent}/task"))
                .into_iter()
                .flatten();
            for task in tasks.flatten() {
                let children =
                    std::fs::read_to_string(task.path().join("children")).unwrap_or_default();
                tree.extend(
                    children
                        .split_whitespace()
                        .filter_map(|c| c.parse::<u32>().ok()),
                );
            }
        }
        for pid in tree.into_iter().filter(|&pid| pid > 1) {
            // SAFETY: kill(2) has no memory-safety preconditions; a process that already exited
            // only yields an ignored error.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        }
    }
    #[cfg(windows)]
    crate::workspace::signal_process_group(pid, crate::workspace::SIGKILL);
    #[cfg(not(any(target_os = "linux", windows)))]
    let _ = pid;
}
