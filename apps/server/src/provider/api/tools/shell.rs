//! The `bash` tool: a command in the user's shell, with a time limit and its output capped.

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

pub(super) const BASH_TIMEOUT: u64 = 120;

pub(super) const MAX_BASH_TIMEOUT: u64 = 600;

/// Output a command's stdout or stderr keeps in memory.
const MAX_PIPE: usize = 1_000_000;

pub(super) fn bash(command: &str, timeout: u64, root: &Path) -> Result<String, String> {
    // The command stays in the agent's process group, so stopping the turn stops it too.
    // The turn sets SHELL to the shell Settings chose. On Windows it must name a file, since Git
    // Bash sets it to a path of its own.
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty() && (!cfg!(windows) || Path::new(s).is_file()))
        .or_else(|| crate::system::effective_shell(None))
        .unwrap_or_else(|| if cfg!(windows) { "cmd" } else { "/bin/sh" }.into());
    let mut cmd = crate::system::host_command(&shell);
    cmd.args(crate::system::shell_args(&shell, command));
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
