//! Starting and stopping the agent process, reading its output, and writing to its stdin.
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::process::ChildStderr;
use std::process::ChildStdin;
use std::process::ExitStatus;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::time::Duration;

use crate::workspace;
use super::queue::Steers;


/// How much of the agent's stderr is kept for error reporting.
pub(super) const STDERR_TAIL_BYTES: usize = 4096;

/// How long a stopped agent gets to exit after SIGTERM before it is sent SIGKILL.
pub(super) const KILL_AFTER: Duration = Duration::from_secs(5);

/// How long quitting the app waits for agents to exit after SIGTERM.
pub(super) const QUIT_GRACE: Duration = Duration::from_secs(2);

/// The longest line of agent output that is kept; longer lines are dropped whole.
pub(super) const MAX_LINE_BYTES: usize = 8 << 20;

/// How long a turn may run before the agent is stopped as timed out.
pub(super) const TURN_TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);

/// The agent's stdin; None once the turn has finished.
pub(super) type Stdin = Arc<Mutex<Option<ChildStdin>>>;

/// Writes one line to the agent's stdin, if it is still open.
pub(super) fn write_line(stdin: &Mutex<Option<ChildStdin>>, line: &str) -> bool {
    write_to(
        &mut stdin.lock().unwrap_or_else(PoisonError::into_inner),
        line,
    )
}

pub(super) fn write_to(stdin: &mut Option<ChildStdin>, line: &str) -> bool {
    let Some(pipe) = stdin.as_mut() else {
        return false;
    };
    match writeln!(pipe, "{line}").and_then(|()| pipe.flush()) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("failed to write to agent: {e}");
            false
        }
    }
}

/// Sends SIGTERM to the agent's process group, then SIGKILL if the agent has not exited
/// after `KILL_AFTER`.
pub(super) fn terminate(pid: u32, exited: Arc<AtomicBool>) {
    workspace::signal_process_group(pid, workspace::SIGTERM);
    std::thread::spawn(move || {
        std::thread::sleep(KILL_AFTER);
        if !exited.load(Ordering::SeqCst) {
            workspace::signal_process_group(pid, workspace::SIGKILL);
        }
    });
}

/// Why the agent's process ended, when it left no error message of its own.
pub(super) fn exit_error(exit: Option<ExitStatus>) -> String {
    if let Some(code) = exit.and_then(|s| s.code()) {
        return format!("Agent exited with code {code}");
    }
    // systemd sends SIGKILL to an agent whose scope ran out of memory.
    #[cfg(unix)]
    if exit.and_then(|s| std::os::unix::process::ExitStatusExt::signal(&s)) == Some(libc::SIGKILL) {
        return "The agent was killed, likely for using too much memory".into();
    }
    "Agent exited unexpectedly".into()
}

/// Closes stdin, which lets the agent exit, unless it still owes an answer to a steering message.
pub(super) fn close_stdin_unless_steered(stdin: &Stdin, steers: &Steers) {
    let mut stdin = stdin.lock().unwrap_or_else(PoisonError::into_inner);
    if steers
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_empty()
    {
        stdin.take();
    }
}

/// The next line of agent output, or None once the output has ended or cannot be read.
/// A line longer than `MAX_LINE_BYTES` is dropped whole, without being held in memory.
pub(super) fn next_line(reader: &mut impl BufRead) -> Option<String> {
    let mut line = Vec::new();
    let mut oversized = false;
    loop {
        let buf = match reader.fill_buf() {
            Ok(buf) => buf,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        };
        if buf.is_empty() {
            return (!line.is_empty() && !oversized)
                .then(|| String::from_utf8_lossy(&line).into_owned());
        }
        let (chunk, found) = match buf.iter().position(|&b| b == b'\n') {
            Some(i) => (&buf[..i], true),
            None => (buf, false),
        };
        let used = chunk.len() + usize::from(found);
        if !oversized && line.len() + chunk.len() > MAX_LINE_BYTES {
            oversized = true;
            line = Vec::new();
            log::warn!("dropped agent output line over {MAX_LINE_BYTES} bytes");
        }
        if !oversized {
            line.extend_from_slice(chunk);
        }
        reader.consume(used);
        if found {
            if !oversized {
                return Some(String::from_utf8_lossy(&line).into_owned());
            }
            oversized = false;
            line.clear();
        }
    }
}

/// Reads all of stderr but keeps only roughly the last `STDERR_TAIL_BYTES`.
pub(super) fn read_stderr_tail(stderr: ChildStderr) -> String {
    let mut reader = BufReader::new(stderr);
    let mut tail = String::new();
    let mut line = Vec::new();
    while reader.read_until(b'\n', &mut line).unwrap_or(0) > 0 {
        tail.push_str(&String::from_utf8_lossy(&line));
        line.clear();
        if tail.len() > STDERR_TAIL_BYTES {
            let mut cut = tail.len() - STDERR_TAIL_BYTES;
            while !tail.is_char_boundary(cut) {
                cut += 1;
            }
            tail.drain(..cut);
        }
    }
    tail
}

/// The last few non-empty stderr lines, used as the error text when the CLI fails without a message.
pub(super) fn last_lines(tail: &str) -> Option<String> {
    let mut lines: Vec<&str> = tail
        .lines()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .take(3)
        .collect();
    lines.reverse();
    (!lines.is_empty()).then(|| lines.join("\n"))
}
