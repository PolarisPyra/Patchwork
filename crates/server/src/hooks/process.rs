//! Running one hook command: in the project folder, local or on its host, in a process range of
//! its own that is stopped at the timeout and once the command exits.

// Rust guideline compliant 2026-10-09

use std::io::{Read, Write};
use std::process::{ExitStatus, Stdio};
use std::time::{Duration, Instant};

use crate::Error;

/// How a hook command ended, and what it wrote.
pub(super) struct Finished {
    pub(super) status: ExitStatus,
    pub(super) stdout: String,
    pub(super) stderr: String,
}

/// Runs `command` in `root`, local or remote, with `env` set and `input` on its stdin.
///
/// # Errors
///
/// Returns an error when there is no shell, the command does not start, or it runs past
/// `timeout_secs`, in which case it and everything it started are stopped.
pub(super) fn run_hook_process(
    root: &str,
    command: &str,
    env: &[(&str, &str)],
    input: Option<String>,
    timeout_secs: u64,
) -> Result<Finished, Error> {
    let mut cmd =
        crate::system::remote::shell_in(root, command).ok_or("there is no shell to run it in")?;
    cmd.envs(env.iter().copied());
    let cmd = crate::system::remote::supervised(cmd)?;
    let stdin = if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    };
    let (mut child, range) =
        crate::system::contained::spawn(cmd, stdin).map_err(|e| format!("did not start: {e}"))?;
    let stdout = read(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let stderr = read(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    if let (Some(mut stdin), Some(input)) = (child.stdin.take(), input) {
        // On its own thread, so a hook that does not read its input cannot hold off the timeout.
        std::thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        });
    }
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            // Short, so a quick hook returns at once instead of after a fixed wait.
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                range.stop();
                let _ = child.wait();
                return Err(format!("ran past {timeout_secs}s and was stopped").into());
            }
        }
    };
    // What it left running holds the output pipes open, and would outlive the hook.
    range.stop();
    Ok(Finished {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

/// Reads a pipe to its end on a thread of its own.
fn read(pipe: Option<Box<dyn Read + Send>>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_string(&mut text);
        }
        text
    })
}
