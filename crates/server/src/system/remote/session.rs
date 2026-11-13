//! One helper per host that stays running and answers [`super::op`] requests a line at a time.
//!
//! Starting the helper for each request costs a new channel on the connection and a new process
//! on the host, which on a Windows host takes far longer than the answer. A session pays that
//! once: each request after is one line there and one line back. A request that comes while the
//! session answers another, or that the session fails, goes to a helper of its own instead.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::sync::{Arc, Mutex, PoisonError};

use super::connection::ssh;
use super::upload::Installed;

/// The argument that answers one request per line on stdin until it closes.
pub const OPS_ARG: &str = "--remote-ops";

/// Each host's session, or nothing where none is running.
static SESSIONS: Mutex<Option<HashMap<String, Slot>>> = Mutex::new(None);

type Slot = Arc<Mutex<State>>;

/// The most sessions in a row that may end without one answer before no more are started.
const MOST_UNANSWERED: u8 = 2;

/// A host's session and how it has fared.
#[derive(Default)]
struct State {
    session: Option<Session>,
    /// The sessions started since one last answered. A host whose helper cannot hold a session
    /// would otherwise pay for a failed start before every request.
    unanswered: u8,
}

/// A helper running on a host, with the pipes to ask it over.
struct Session {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Session {
    /// Starts the helper `installed` on `host`.
    fn open(host: &str, installed: &Installed) -> Result<Self, Error> {
        let mut child = ssh(host)?
            .args(["-T", "--", host, &installed.line(&[OPS_ARG])])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Could not run ssh: {e}"))?;
        if let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) {
            Ok(Self {
                child,
                stdin,
                stdout: BufReader::new(stdout),
            })
        } else {
            let _ = child.kill();
            let _ = child.wait();
            Err("Could not reach ssh's input and output".into())
        }
    }

    /// Sends the request `body` and reads the reply's line. None when the helper has gone.
    fn exchange(&mut self, body: &[u8]) -> Option<Vec<u8>> {
        self.stdin.write_all(body).ok()?;
        self.stdin.write_all(b"\n").ok()?;
        self.stdin.flush().ok()?;
        let mut line = Vec::new();
        let read = self.stdout.read_until(b'\n', &mut line).ok()?;
        (read > 0 && line.ends_with(b"\n")).then_some(line)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn slot(host: &str) -> Slot {
    Arc::clone(
        SESSIONS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert_with(HashMap::new)
            .entry(host.to_string())
            .or_default(),
    )
}

/// The reply to the request `body` from `host`'s session, which is started when none runs.
///
/// None when the session is answering another request, cannot be started, or went away without
/// answering; the caller then asks a helper of its own, which reports what is wrong. A host
/// whose sessions keep ending unanswered gets no more of them until [`close`].
pub(super) fn ask(host: &str, installed: &Installed, body: &[u8]) -> Option<Vec<u8>> {
    let slot = slot(host);
    let mut state = slot.try_lock().ok()?;
    if state.session.is_none() {
        if state.unanswered >= MOST_UNANSWERED {
            return None;
        }
        state.unanswered += 1;
        state.session = Session::open(host, installed).ok();
    }
    let reply = state.session.as_mut()?.exchange(body);
    if reply.is_some() {
        state.unanswered = 0;
    } else {
        state.session = None;
    }
    reply
}

/// Ends `host`'s session, so the next request starts a new one.
pub(super) fn close(host: &str) {
    *slot(host).lock().unwrap_or_else(PoisonError::into_inner) = State::default();
}
