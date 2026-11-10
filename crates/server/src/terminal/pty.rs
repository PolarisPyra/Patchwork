//! The pseudo-terminal a shell runs in: opening it at the view's size with the shell started in
//! it, and the threads that carry the shell's output and its exit to the terminal.

// Rust guideline compliant 2026-10-09

use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use super::Launch;
use super::session::Term;
use crate::Error;

/// How long output gathers before it is sent, and the most sent at once.
const BATCH: Duration = Duration::from_millis(8);
const BATCH_BYTES: usize = 256 * 1024;
/// How long the end of a shell waits for the rest of its output.
const EXIT_SETTLE: Duration = Duration::from_millis(200);
const MAX_COLS: u16 = 500;
const MAX_ROWS: u16 = 200;

enum Pump {
    Data(Vec<u8>),
    Eof,
    Exited(Option<u32>),
}

/// What the pump has gathered: output not yet sent, and whether the output and the shell ended.
#[derive(Default)]
struct Batch {
    pending: Vec<u8>,
    eof: bool,
    exit: Option<(Option<u32>, Instant)>,
}

impl Batch {
    fn take(&mut self, message: Pump) {
        match message {
            Pump::Data(bytes) => self.pending.extend_from_slice(&bytes),
            Pump::Eof => self.eof = true,
            Pump::Exited(code) => self.exit = Some((code, Instant::now())),
        }
    }

    /// The output gathered, but for a character split across two reads, which waits for the rest
    /// of its bytes.
    fn whole(&mut self) -> String {
        let whole = match std::str::from_utf8(&self.pending) {
            Err(e) if e.error_len().is_none() && !self.eof => e.valid_up_to(),
            _ => self.pending.len(),
        };
        let data = String::from_utf8_lossy(&self.pending[..whole]).into_owned();
        self.pending.drain(..whole);
        data
    }
}

/// The pseudo-terminal's controlling side, and the shell running in it.
type Started = (Box<dyn MasterPty + Send>, Box<dyn Child + Send + Sync>);

/// Opens a pseudo-terminal and starts the shell in it.
pub(super) fn start(launch: &Launch, cols: u16, rows: u16) -> Result<Started, Error> {
    let pair = native_pty_system()
        .openpty(size(cols, rows))
        .map_err(|e| format!("Could not open a terminal: {e}"))?;
    let mut cmd = CommandBuilder::new(&launch.shell);
    cmd.args(&launch.args);
    cmd.cwd(&launch.cwd);
    cmd.env_clear();
    for (key, value) in &launch.env {
        cmd.env(key, value);
    }
    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("Could not start {}: {e}", launch.shell))?;
    Ok((pair.master, child))
}

pub(super) fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.clamp(1, MAX_ROWS),
        cols: cols.clamp(1, MAX_COLS),
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// Reads the shell's output and waits for it to exit, each on a thread of its own, and a third
/// keeps the screen and sends windows what gathers within [`BATCH`] as one frame: the PTY hands
/// over a few hundred bytes per read, so a noisy command would make thousands a second.
pub(super) fn pump(
    id: u32,
    term: Arc<Term>,
    mut reader: Box<dyn Read + Send>,
    mut child: Box<dyn Child + Send + Sync>,
) {
    let (tx, rx) = mpsc::channel::<Pump>();
    let output = tx.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 16 * 1024];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || output.send(Pump::Data(buf[..n].to_vec())).is_err() {
                break;
            }
        }
        let _ = output.send(Pump::Eof);
    });
    let exited = Arc::clone(&term);
    std::thread::spawn(move || {
        let code = child.wait().ok().map(|status| status.exit_code());
        exited.exited.store(true, Ordering::Release);
        let _ = tx.send(Pump::Exited(code));
    });
    std::thread::spawn(move || {
        let mut batch = Batch::default();
        let mut published = false;
        loop {
            let settle = batch
                .exit
                .filter(|_| !published)
                .map(|(_, at)| (at + EXIT_SETTLE).saturating_duration_since(Instant::now()));
            let next = match settle {
                Some(wait) => rx.recv_timeout(wait),
                None => rx
                    .recv()
                    .map_err(|_discarded| RecvTimeoutError::Disconnected),
            };
            match next {
                Ok(message) => batch.take(message),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if !batch.pending.is_empty() {
                let deadline = Instant::now() + BATCH;
                while batch.pending.len() < BATCH_BYTES {
                    match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                        Ok(message) => batch.take(message),
                        Err(_) => break,
                    }
                }
                let data = batch.whole();
                if !data.is_empty() {
                    term.output(data);
                }
            }
            if let Some((code, at)) = batch.exit
                && !published
                && (batch.eof || at.elapsed() >= EXIT_SETTLE)
            {
                published = true;
                term.exit_frame(id, code);
            }
        }
        if !published {
            term.exit_frame(id, batch.exit.and_then(|(code, _)| code));
        }
    });
}
