//! The running terminals, each a shell in a pseudo-terminal, and the windows attached to them.
//! A terminal outlives the window that opened it: a window that reloads attaches again and gets
//! its screen back. The window attached last types into the shell. One no window holds is closed
//! once it has sat idle at its prompt for [`UNATTENDED`].

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use portable_pty::{ChildKiller, MasterPty};

use super::activity::{self, Activity, ShellActivity};
use super::{AttachedTerminal, Launch, TerminalFrame, TerminalInfo, pty, range};
use crate::Error;
use crate::system::processes::{self, Process};

/// Where a window gets a terminal's frames; false once the window is gone.
pub type FrameSink = Box<dyn Fn(&TerminalFrame) -> bool + Send + Sync>;

/// The most output a terminal keeps to redraw a window that attaches.
const SCREEN_BYTES: usize = 1 << 20;
/// How long a terminal no window holds may sit idle before it is closed.
const UNATTENDED: Duration = Duration::from_secs(2 * 60 * 60);
/// How often such a terminal is checked, and how soon a close that failed is tried again.
const OBSERVE_EVERY: Duration = Duration::from_secs(30);
const RETRY_CLOSE: Duration = Duration::from_secs(60);

/// The running terminals, by id.
#[derive(Default)]
pub struct Terminals {
    next: AtomicU32,
    attachments: Arc<AtomicU32>,
    open: Arc<Mutex<HashMap<u32, Arc<Term>>>>,
    /// Whether the thread that closes unattended terminals has started.
    reclaiming: AtomicBool,
}

/// One terminal and what it owns.
pub(super) struct Term {
    pub(super) pid: Option<i32>,
    /// The shell as it started, which tells it apart from a later process with its pid.
    pub(super) root: Option<Process>,
    pub(super) master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    pub(super) killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    pub(super) exited: AtomicBool,
    pub(super) activity: Option<ShellActivity>,
    /// The terminal's private folder: its prompt hooks, and a note of its shell.
    dir: Option<PathBuf>,
    /// The processes the shell was seen to start.
    pub(super) tracked: Mutex<Vec<Process>>,
    /// The last activity answer and its revision.
    pub(super) observed: Mutex<(String, u64)>,
    /// Held while the terminal closes, so a second close waits for the first.
    closing: Mutex<()>,
    state: Mutex<State>,
}

struct State {
    info: TerminalInfo,
    screen: String,
    /// Whether the start of `screen` was cut to keep it within [`SCREEN_BYTES`].
    trimmed: bool,
    /// The screen and modes as all of the output left them.
    model: vt100::Parser,
    followers: Vec<Follower>,
    /// The attachment that types into the shell.
    controller: Option<u32>,
    closing: bool,
    /// When a close that failed is tried again.
    retry_at: Option<Instant>,
    /// Changes with input and with each window that comes or goes, which voids what was seen.
    epoch: u64,
    idle: Option<Idle>,
}

struct Follower {
    attachment: u32,
    page: String,
    sink: FrameSink,
}

#[derive(Clone, Copy)]
struct Idle {
    since: Instant,
    observed_at: Instant,
    revision: u64,
}

impl std::fmt::Debug for Terminals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Terminals").finish_non_exhaustive()
    }
}

impl Terminals {
    /// Starts the shell [`super::launch`] picked at the size of the view, attached to `page`,
    /// whose frames go to `sink`. It blocks while the shell starts, so call it off the main thread.
    ///
    /// # Errors
    ///
    /// Returns an error when the shell cannot be started in a pseudo-terminal, or when its output
    /// or input cannot be attached.
    pub fn spawn(
        &self,
        mut launch: Launch,
        cols: u16,
        rows: u16,
        page: &str,
        sink: FrameSink,
    ) -> Result<AttachedTerminal, Error> {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let begun = Instant::now();
        let view = pty::size(cols, rows);
        let dir = range::create_dir();
        let activity = dir
            .as_deref()
            .and_then(|dir| activity::prepare(&mut launch, dir));
        let started = pty::start(&launch, cols, rows);
        let (master, child) = match started {
            Ok(started) => started,
            Err(e) => {
                remove(dir.as_deref());
                return Err(e);
            }
        };
        let pid = child.process_id().and_then(|pid| i32::try_from(pid).ok());
        let root = pid.and_then(processes::find);
        if let (Some(dir), Some(root)) = (&dir, &root) {
            range::record(dir, root);
        }
        let killer = child.clone_killer();
        let reader = master.try_clone_reader().map_err(|e| e.to_string());
        let writer = master.take_writer().map_err(|e| e.to_string());
        let (reader, writer) = match (reader, writer) {
            (Ok(reader), Ok(writer)) => (reader, writer),
            (Err(e), _) | (_, Err(e)) => {
                let mut killer = killer;
                let _ = killer.kill();
                remove(dir.as_deref());
                return Err(e.into());
            }
        };
        let attachment = self.attachments.fetch_add(1, Ordering::Relaxed) + 1;
        let info = TerminalInfo {
            id,
            name: super::launch::file_name(&launch.shell),
            cwd: launch.cwd.to_string_lossy().into_owned(),
            running: true,
            exit_code: None,
        };
        let term = Arc::new(Term {
            pid,
            root,
            master: Mutex::new(master),
            writer: Mutex::new(writer),
            killer: Mutex::new(killer),
            exited: AtomicBool::new(false),
            activity,
            dir,
            tracked: Mutex::default(),
            observed: Mutex::default(),
            closing: Mutex::new(()),
            state: Mutex::new(State {
                info: info.clone(),
                screen: String::new(),
                trimmed: false,
                model: vt100::Parser::new(view.rows, view.cols, 0),
                followers: vec![Follower {
                    attachment,
                    page: page.to_string(),
                    sink,
                }],
                controller: Some(attachment),
                closing: false,
                retry_at: None,
                epoch: 0,
                idle: None,
            }),
        });
        self.open
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, Arc::clone(&term));
        log::info!(
            id:%,
            shell:% = launch.shell,
            args:% = launch.args.join(" "),
            cwd:% = launch.cwd.display(),
            millis:% = begun.elapsed().as_millis();
            "terminal {{id}}: started {{shell}} {{args}} in {{cwd}} ({{millis}} ms)"
        );
        pty::pump(id, term, reader, child);
        let open = Arc::clone(&self.open);
        if !self.reclaiming.swap(true, Ordering::Relaxed) {
            std::thread::spawn(move || reclaim(&open));
        }
        Ok(AttachedTerminal {
            attachment,
            info,
            screen: String::new(),
        })
    }

    /// The terminals that are open, running or ended, oldest first.
    pub fn list(&self) -> Vec<TerminalInfo> {
        let mut list: Vec<TerminalInfo> = self
            .terms()
            .iter()
            .filter_map(|term| {
                let state = term.lock();
                (!state.closing).then(|| state.info.clone())
            })
            .collect();
        list.sort_by_key(|info| info.id);
        list
    }

    /// Attaches `page` to terminal `id`: the screen so far, then its frames go to `sink`. The
    /// page types into the shell from now on.
    ///
    /// # Errors
    ///
    /// Returns an error when the terminal has closed or is closing.
    pub fn attach(&self, id: u32, page: &str, sink: FrameSink) -> Result<AttachedTerminal, Error> {
        let term = self.get(id)?;
        let mut state = term.lock();
        if state.closing {
            return Err("The terminal has closed".into());
        }
        let attachment = self.attachments.fetch_add(1, Ordering::Relaxed) + 1;
        state.followers.push(Follower {
            attachment,
            page: page.to_string(),
            sink,
        });
        state.controller = Some(attachment);
        state.invalidate();
        Ok(AttachedTerminal {
            attachment,
            info: state.info.clone(),
            screen: state.replay(),
        })
    }

    /// Stops sending terminal `id`'s frames to `attachment`.
    pub fn detach(&self, id: u32, attachment: u32) {
        if let Ok(term) = self.get(id) {
            term.lock().unfollow(|f| f.attachment == attachment);
        }
    }

    /// Detaches every terminal from `page`, as it loads again or closes.
    pub fn release_page(&self, page: &str) {
        for term in self.terms() {
            term.lock().unfollow(|f| f.page == page);
        }
    }

    /// Types `data` into the shell, from the attachment that types into it.
    ///
    /// # Errors
    ///
    /// Returns an error when the terminal has closed, another window types into it, or writing to
    /// the shell fails.
    pub fn write(&self, id: u32, attachment: u32, data: &str) -> Result<(), Error> {
        let term = self.get(id)?;
        term.lock().control(attachment, &term)?;
        if let Some(activity) = &term.activity {
            activity.invalidate();
        }
        let mut writer = term.writer.lock().unwrap_or_else(PoisonError::into_inner);
        writer
            .write_all(data.as_bytes())
            .and_then(|()| writer.flush())
            .map_err(|e| (e.to_string()).into())
    }

    /// Follows the view's size, so full-screen programs lay out to it.
    ///
    /// # Errors
    ///
    /// Returns an error when the terminal has closed, another window types into it, or resizing the
    /// pseudo-terminal fails.
    pub fn resize(&self, id: u32, attachment: u32, cols: u16, rows: u16) -> Result<(), Error> {
        let term = self.get(id)?;
        let size = pty::size(cols, rows);
        {
            let mut state = term.lock();
            state.control(attachment, &term)?;
            state.model.screen_mut().set_size(size.rows, size.cols);
        };
        term.master
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .resize(size)
            .map_err(|e| (e.to_string()).into())
    }

    /// Closes terminal `id`, ending its shell and every process it started. It blocks for up
    /// to a few seconds, so call it off the main thread. A close that fails leaves the terminal
    /// closed to windows and is tried again a minute later.
    ///
    /// # Errors
    ///
    /// Returns an error when the shell or a process it started is still running after being asked
    /// to stop.
    pub fn close(&self, id: u32) -> Result<(), Error> {
        close(&self.open, id)
    }

    /// Ends every terminal's processes at once, as Patchwork quits.
    pub fn kill_all(&self) {
        for term in self.terms() {
            range::kill(&term);
            remove(term.dir.as_deref());
        }
    }

    fn get(&self, id: u32) -> Result<Arc<Term>, Error> {
        self.open
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .cloned()
            .ok_or_else(|| "The terminal has closed".into())
    }

    fn terms(&self) -> Vec<Arc<Term>> {
        terms(&self.open)
    }
}

impl Term {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Keeps `data` for windows that attach later and sends it to those attached.
    pub(super) fn output(&self, data: String) {
        self.lock().output(data);
    }

    /// Marks the shell ended with `code` and tells the windows attached.
    pub(super) fn exit_frame(&self, id: u32, code: Option<u32>) {
        log::info!(id:%, code:?; "terminal {{id}}: exited with {{code}}");
        let mut state = self.lock();
        state.info.running = false;
        state.info.exit_code = code;
        state.send(&TerminalFrame::Exit { code });
    }
}

impl State {
    /// What was seen of the shell no longer counts.
    fn invalidate(&mut self) {
        self.epoch += 1;
        self.idle = None;
    }

    /// Whether `attachment` may type into the running shell; input voids what was seen.
    fn control(&mut self, attachment: u32, term: &Term) -> Result<(), Error> {
        if self.closing || term.exited.load(Ordering::Acquire) {
            return Err("The terminal has closed".into());
        }
        if self.controller != Some(attachment) {
            return Err("Another window types into this terminal".into());
        }
        self.invalidate();
        Ok(())
    }

    /// Drops the followers `which` picks; the window attached last of the rest types next.
    fn unfollow(&mut self, which: impl Fn(&Follower) -> bool) {
        let before = self.followers.len();
        self.followers.retain(|f| !which(f));
        if self.followers.len() != before {
            self.left();
        }
    }

    fn left(&mut self) {
        if !self
            .followers
            .iter()
            .any(|f| Some(f.attachment) == self.controller)
        {
            self.controller = self.followers.last().map(|f| f.attachment);
        }
        self.invalidate();
    }

    /// Sends `frame` to every window attached, dropping those that are gone.
    fn send(&mut self, frame: &TerminalFrame) {
        let before = self.followers.len();
        self.followers.retain(|f| (f.sink)(frame));
        if self.followers.len() != before {
            self.left();
        }
    }

    /// The output kept to redraw a window. Once its start was cut, the screen and modes as they
    /// stand follow it, since the cut may have taken the switch that set them.
    fn replay(&self) -> String {
        if !self.trimmed {
            return self.screen.clone();
        }
        let screen = self.model.screen();
        let alternate = if screen.alternate_screen() {
            "\x1b[?1049h"
        } else {
            "\x1b[?1049l"
        };
        let state = String::from_utf8_lossy(&screen.state_formatted()).into_owned();
        format!("{}{alternate}{state}", self.screen)
    }

    fn output(&mut self, data: String) {
        self.model.process(data.as_bytes());
        self.screen.push_str(&data);
        if self.screen.len() > SCREEN_BYTES {
            // From the first line that starts within the limit, so the screen redraws from a
            // line's start.
            let mut cut = self.screen.len() - SCREEN_BYTES;
            while !self.screen.is_char_boundary(cut) {
                cut += 1;
            }
            let cut = self.screen[cut..].find('\n').map_or(cut, |at| cut + at + 1);
            self.screen.drain(..cut);
            self.trimmed = true;
        }
        self.send(&TerminalFrame::Output { data });
    }
}

fn terms(open: &Mutex<HashMap<u32, Arc<Term>>>) -> Vec<Arc<Term>> {
    open.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .values()
        .cloned()
        .collect()
}

fn close(open: &Mutex<HashMap<u32, Arc<Term>>>, id: u32) -> Result<(), Error> {
    let Some(term) = open
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&id)
        .cloned()
    else {
        return Ok(());
    };
    let _closing = term.closing.lock().unwrap_or_else(PoisonError::into_inner);
    if !open
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .contains_key(&id)
    {
        return Ok(());
    }
    {
        let mut state = term.lock();
        state.closing = true;
        state.retry_at = None;
        state.followers.clear();
        state.controller = None;
        state.invalidate();
    };
    match range::close(&term) {
        Ok(()) => {
            open.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&id);
            remove(term.dir.as_deref());
            log::info!(id:%; "terminal {{id}}: closed");
            Ok(())
        }
        Err(e) => {
            term.lock().retry_at = Some(Instant::now() + RETRY_CLOSE);
            log::warn!(id:%, error:% = e; "terminal {{id}}: {{error}}");
            Err(e)
        }
    }
}

/// Closes terminals no window has held while they sat idle for [`UNATTENDED`], each seen idle
/// at every check with nothing new recorded, and tries again closes that failed.
fn reclaim(open: &Arc<Mutex<HashMap<u32, Arc<Term>>>>) {
    loop {
        std::thread::sleep(OBSERVE_EVERY);
        let ids: Vec<(u32, Arc<Term>)> = open
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(id, term)| (*id, Arc::clone(term)))
            .collect();
        for (id, term) in ids {
            if observe(&term) {
                let open = Arc::clone(open);
                std::thread::spawn(move || close(&open, id));
            }
        }
    }
}

/// Whether `term` is due to close.
fn observe(term: &Term) -> bool {
    let epoch = {
        let mut state = term.lock();
        if state.closing {
            let due = state.retry_at.is_some_and(|at| at <= Instant::now());
            if due {
                state.retry_at = None;
            }
            return due;
        }
        if !state.followers.is_empty() {
            return false;
        }
        state.epoch
    };
    let (activity, revision) = range::inspect(term);
    let mut state = term.lock();
    if state.closing || !state.followers.is_empty() || state.epoch != epoch {
        return false;
    }
    if activity != Activity::Idle {
        state.idle = None;
        return false;
    }
    let now = Instant::now();
    let idle = match state.idle {
        Some(idle)
            if idle.revision == revision
                && now.duration_since(idle.observed_at) <= OBSERVE_EVERY * 2 =>
        {
            Idle {
                observed_at: now,
                ..idle
            }
        }
        _ => Idle {
            since: now,
            observed_at: now,
            revision,
        },
    };
    state.idle = Some(idle);
    now.duration_since(idle.since) >= UNATTENDED
}

/// Removes a terminal's private folder.
fn remove(dir: Option<&Path>) {
    if let Some(dir) = dir
        && let Err(e) = std::fs::remove_dir_all(dir)
    {
        log::warn!(error:% = e; "could not remove the terminal's folder: {{error}}");
    }
}
