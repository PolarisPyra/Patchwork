//! Starting and stopping the agent process, reading its output, and writing to its stdin.

// Rust guideline compliant 2026-10-09

use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;
use std::process::Child;
use std::process::ChildStderr;
use std::process::ChildStdin;
use std::process::ExitStatus;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::mpsc;
use std::time::Duration;

use crate::logging::{AGENT_LOG, Data, Source};
use crate::provider::AgentKind;
use crate::settings::LogLevel;
use crate::workspace;
use crate::orchestration::messages::queue::Steers;

/// How much of the agent's stderr is kept for error reporting.
pub(in crate::orchestration) const STDERR_TAIL_BYTES: usize = 4096;

/// How long a stopped agent gets to exit after SIGTERM before it is sent SIGKILL.
pub(in crate::orchestration) const KILL_AFTER: Duration = Duration::from_secs(5);

/// How long quitting the app waits for agents to exit after SIGTERM.
pub(in crate::orchestration) const QUIT_GRACE: Duration = Duration::from_secs(2);

/// How long a stopped agent gets to end its reply after being interrupted before it is sent
/// SIGTERM.
pub(in crate::orchestration) const INTERRUPT_GRACE: Duration = Duration::from_secs(5);

/// The longest line of agent output that is kept; longer lines are dropped whole.
pub(in crate::orchestration) const MAX_LINE_BYTES: usize = 8 << 20;

/// How long an idle agent stays up for the next message after its last reply, unless shell
/// commands it left running keep it up longer.
pub(in crate::orchestration) const IDLE_LIMIT: Duration = Duration::from_secs(30 * 60);

/// How often a running agent is checked: against `IDLE_LIMIT`, for a reply gone quiet, and for
/// having exited.
pub(in crate::orchestration) const WATCH_EVERY: Duration = Duration::from_secs(15);

/// How long a reply goes without output before the user is told: longer than a model takes to
/// answer, shorter than a stuck command leaves them waiting before they look.
pub(in crate::orchestration) const QUIET_AFTER: Duration = Duration::from_secs(3 * 60);

/// The agent's stdin; None once the turn has finished.
pub(in crate::orchestration) type Stdin = Arc<Mutex<Option<AgentStdin>>>;

/// The agent's stdin, with where its lines come from for the agent log. Lines go to a thread of
/// its own that writes them to the pipe, so an agent that stops reading never holds up whoever
/// wrote to it, such as Stop, nor the locks they hold. Dropping it closes the pipe once the lines
/// already sent are written.
pub(in crate::orchestration) struct AgentStdin {
    lines: mpsc::Sender<String>,
    source: Source,
}

impl AgentStdin {
    pub(in crate::orchestration) fn take(
        child: &mut Child,
        agent: AgentKind,
        thread_id: &str,
    ) -> Option<Self> {
        let source = Source::agent(agent, child.id(), thread_id, None);
        let pipe = child.stdin.take()?;
        let (lines, queued) = mpsc::channel();
        let writer = source.clone();
        std::thread::spawn(move || write_all(pipe, &queued, &writer));
        Some(Self { lines, source })
    }
}

/// Writes one line to the agent's stdin, if it is still open.
pub(in crate::orchestration) fn write_line(stdin: &Mutex<Option<AgentStdin>>, line: &str) -> bool {
    write_to(
        &mut stdin.lock().unwrap_or_else(PoisonError::into_inner),
        line,
    )
}

/// Hands one line to the agent's stdin. False once stdin is closed or a write to it has failed.
pub(in crate::orchestration) fn write_to(stdin: &mut Option<AgentStdin>, line: &str) -> bool {
    let Some(AgentStdin { lines, source }) = stdin.as_mut() else {
        return false;
    };
    AGENT_LOG.line("in", source, line);
    lines.send(line.to_string()).is_ok()
}

/// Writes the lines handed to the agent's stdin until it is closed or a write fails.
fn write_all(mut pipe: ChildStdin, lines: &mpsc::Receiver<String>, source: &Source) {
    for line in lines {
        if let Err(e) = writeln!(pipe, "{line}").and_then(|()| pipe.flush()) {
            AGENT_LOG.write(
                LogLevel::Error,
                "write_failed",
                source,
                Data::Text(e.to_string()),
            );
            log::warn!(error:% = e; "failed to write to agent: {{error}}");
            return;
        }
    }
}

/// Sends SIGTERM to the agent's process group, then SIGKILL if the agent has not exited
/// after `KILL_AFTER`.
pub(in crate::orchestration) fn terminate(pid: u32, exited: Arc<AtomicBool>) {
    workspace::signal_process_group(pid, workspace::SIGTERM);
    std::thread::spawn(move || {
        std::thread::sleep(KILL_AFTER);
        if !exited.load(Ordering::SeqCst) {
            workspace::signal_process_group(pid, workspace::SIGKILL);
        }
    });
}

/// The interrupts Stop has written to an agent, and how many of them the agent has answered by
/// ending its reply and staying up for the next message. An agent that neither does so nor exits
/// is stopped with a signal.
#[derive(Default)]
pub(in crate::orchestration) struct Interrupts {
    asked: AtomicU64,
    answered: AtomicU64,
}

impl Interrupts {
    /// Counts one more interrupt, and returns its number.
    pub(in crate::orchestration) fn ask(&self) -> u64 {
        self.asked.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Whether an interrupt is still waiting for the agent to end its reply.
    pub(in crate::orchestration) fn waiting(&self) -> bool {
        self.answered.load(Ordering::SeqCst) < self.asked.load(Ordering::SeqCst)
    }

    /// The agent ended its reply and stays up, which answers every interrupt written so far.
    pub(in crate::orchestration) fn answer(&self) {
        self.answered
            .store(self.asked.load(Ordering::SeqCst), Ordering::SeqCst);
    }

    pub(in crate::orchestration) fn answered(&self, number: u64) -> bool {
        self.answered.load(Ordering::SeqCst) >= number
    }
}

/// Why the agent's process ended, when it left no error message of its own.
pub(in crate::orchestration) fn exit_error(exit: Option<ExitStatus>) -> String {
    if let Some(code) = exit.and_then(|s| s.code()) {
        return format!("Agent exited with code {code}");
    }
    // Memory is named only when the kernel counted a kill for it among the agents: another
    // program, or another Patchwork, can send the same signal.
    if crate::system::scope::killed_for_memory() {
        return "The agent was stopped because the system ran low on memory".into();
    }
    #[cfg(unix)]
    if exit.and_then(|s| std::os::unix::process::ExitStatusExt::signal(&s)) == Some(libc::SIGKILL) {
        return "The agent was killed before it finished".into();
    }
    "Agent exited unexpectedly".into()
}

/// `error` with the fix for it on a line of its own, when it is one the person makes outside the
/// app: Claude Code signed out, or too old for the model it was asked to use. Claude's own words
/// say to run `/login`, which `claude -p` cannot do, so the fix says where to do it.
pub(in crate::orchestration) fn with_remedy(agent: AgentKind, error: String) -> String {
    if agent != AgentKind::Claude {
        return error;
    }
    let lower = error.to_lowercase();
    let remedy = if lower.contains("does not support this model") {
        "Update Claude Code: run `claude update` in a terminal, then try again, or pick another model."
    } else if [
        "not logged in",
        "/login",
        "invalid api key",
        "oauth token",
        "authentication_error",
        "authentication failed",
    ]
    .iter()
    .any(|said| lower.contains(said))
    {
        "Sign in to Claude Code: run `claude` in a terminal and type /login, then try again."
    } else {
        return error;
    };
    format!("{error}\n\n{remedy}")
}

/// Closes stdin, which lets the agent exit, unless it still owes an answer to a steering message.
pub(in crate::orchestration) fn close_stdin_unless_steered(stdin: &Stdin, steers: &Steers) {
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
pub(in crate::orchestration) fn next_line(reader: &mut impl BufRead) -> Option<String> {
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
            log::warn!(limit = MAX_LINE_BYTES; "dropped agent output line over {{limit}} bytes");
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
pub(in crate::orchestration) fn read_stderr_tail(stderr: ChildStderr, source: &Source) -> String {
    // A line is read in pieces of at most this, so output with no line ends cannot fill memory.
    const MOST_AT_ONCE: u64 = 64 * 1024;
    let mut reader = BufReader::new(stderr);
    let mut tail = String::new();
    let mut line = Vec::new();
    while (&mut reader)
        .take(MOST_AT_ONCE)
        .read_until(b'\n', &mut line)
        .unwrap_or(0)
        > 0
    {
        let text = String::from_utf8_lossy(&line);
        AGENT_LOG.write(
            LogLevel::Debug,
            "stderr",
            source,
            Data::Text(text.trim_end().into()),
        );
        tail.push_str(&text);
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
pub(in crate::orchestration) fn last_lines(tail: &str) -> Option<String> {
    let mut lines: Vec<&str> = tail
        .lines()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .take(3)
        .collect();
    lines.reverse();
    (!lines.is_empty()).then(|| lines.join("\n"))
}
