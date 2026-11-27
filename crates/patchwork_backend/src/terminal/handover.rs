//! Terminals handed from one run of the app to the next, so a restart by a reloader keeps them.
//!
//! The run that quits hands over each running shell's pseudo-terminal, a duplicate of its
//! controlling side, and what its screen showed. The reloader holds the pseudo-terminals open
//! meanwhile, so the shells never see a hangup, and the next run takes them over. The shells
//! were the first run's children, so the next one finds out they ended by their pid instead of
//! waiting on them.
//!
//! A terminal's folder notes in a file `owner`, beside it, the run that holds it, so a run that
//! starts meanwhile doesn't end its shells as ones a killed Patchwork left behind.

// Rust guideline compliant 2026-10-09

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub use patchwork_oxidation_client::fds::{receive_fds, send_fds};
use portable_pty::{Child, ChildKiller, ExitStatus, MasterPty, PtySize};
use serde::{Deserialize, Serialize};

use super::TerminalInfo;

/// What a terminal hands to the next run, but for its pseudo-terminal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Handover {
    /// The terminal as its tab shows it, with its id.
    pub info: TerminalInfo,
    /// The shell's pid.
    pub pid: Option<i32>,
    /// The output that redraws its screen.
    pub screen: String,
    /// Its private folder: its prompt hooks and the note of its shell.
    pub dir: Option<PathBuf>,
}

/// The note, beside the terminals' folders, of the run that holds them.
const OWNER: &str = "owner";
/// What the note says while the terminals are handed over and not yet taken.
const HANDED: &str = "handed over";
/// How long terminals may wait to be taken before a run that starts may end them.
const HANDED_FOR: Duration = Duration::from_secs(60);
/// How often an adopted shell is checked for having ended.
const CHECK_EVERY: Duration = Duration::from_millis(200);

/// Notes that the terminal with the folder `dir` is handed over.
pub(super) fn mark_handed(dir: &Path) {
    note_owner(dir, HANDED);
}

/// Notes that the terminal with the folder `dir` is held by the run named `instance`.
pub(super) fn mark_held(dir: &Path, instance: &str) {
    note_owner(dir, instance);
}

fn note_owner(dir: &Path, owner: &str) {
    let Some(parent) = dir.parent() else { return };
    if let Err(e) = std::fs::write(parent.join(OWNER), owner) {
        log::warn!(error:% = e; "could not note who holds a terminal: {{error}}");
    }
}

/// Who holds the terminals in `folder`: a run's name, or None while they are handed over and
/// not yet taken. Err when no run claims them.
pub(super) fn owner(folder: &Path) -> Result<Option<String>, ()> {
    let path = folder.join(OWNER);
    let text = std::fs::read_to_string(&path).map_err(drop)?;
    if text != HANDED {
        return Ok(Some(text));
    }
    let age = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|at| SystemTime::now().duration_since(at).ok());
    match age {
        Some(age) if age < HANDED_FOR => Ok(None),
        _ => Err(()),
    }
}

/// A pseudo-terminal's controlling side, taken over from the run before.
#[derive(Debug)]
pub(super) struct Master {
    fd: OwnedFd,
}

impl Master {
    pub(super) fn new(fd: OwnedFd) -> Self {
        Self { fd }
    }

    fn file(&self) -> std::io::Result<File> {
        Ok(File::from(self.fd.try_clone()?))
    }
}

impl MasterPty for Master {
    fn resize(&self, size: PtySize) -> anyhow::Result<()> {
        let size = libc::winsize {
            ws_row: size.rows,
            ws_col: size.cols,
            ws_xpixel: size.pixel_width,
            ws_ypixel: size.pixel_height,
        };
        // SAFETY: the descriptor is open for as long as `self`, and `size` is a valid `winsize`.
        if unsafe { libc::ioctl(self.fd.as_raw_fd(), libc::TIOCSWINSZ, &raw const size) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }

    fn get_size(&self) -> anyhow::Result<PtySize> {
        // SAFETY: `winsize` is plain data, filled in by the call below before it is read.
        let mut size: libc::winsize = unsafe { std::mem::zeroed() };
        // SAFETY: the descriptor is open for as long as `self`, and `size` is writable.
        if unsafe { libc::ioctl(self.fd.as_raw_fd(), libc::TIOCGWINSZ, &raw mut size) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(PtySize {
            rows: size.ws_row,
            cols: size.ws_col,
            pixel_width: size.ws_xpixel,
            pixel_height: size.ws_ypixel,
        })
    }

    fn try_clone_reader(&self) -> anyhow::Result<Box<dyn Read + Send>> {
        Ok(Box::new(self.file()?))
    }

    fn take_writer(&self) -> anyhow::Result<Box<dyn Write + Send>> {
        Ok(Box::new(self.file()?))
    }

    fn process_group_leader(&self) -> Option<libc::pid_t> {
        // SAFETY: the descriptor is open for as long as `self`.
        match unsafe { libc::tcgetpgrp(self.fd.as_raw_fd()) } {
            pid if pid > 0 => Some(pid),
            _ => None,
        }
    }

    fn as_raw_fd(&self) -> Option<std::os::fd::RawFd> {
        Some(self.fd.as_raw_fd())
    }

    fn tty_name(&self) -> Option<PathBuf> {
        None
    }
}

/// A shell another run started, known by its pid alone.
#[derive(Debug, Clone, Copy)]
pub(super) struct Adopted {
    pid: i32,
}

impl Adopted {
    pub(super) fn new(pid: i32) -> Self {
        Self { pid }
    }

    fn gone(self) -> bool {
        // SAFETY: signal 0 sends nothing; it only asks whether the pid exists.
        let found = unsafe { libc::kill(self.pid, 0) } == 0;
        !found && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    }

    fn unknown_code() -> std::io::Error {
        std::io::Error::other(
            "the shell was started by an earlier run, so its exit code is unknown",
        )
    }
}

impl ChildKiller for Adopted {
    fn kill(&mut self) -> std::io::Result<()> {
        // SAFETY: sends a hangup, as closing its terminal would, to the shell's pid.
        if unsafe { libc::kill(self.pid, libc::SIGHUP) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(*self)
    }
}

impl Child for Adopted {
    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        if self.gone() {
            return Err(Self::unknown_code());
        }
        Ok(None)
    }

    fn wait(&mut self) -> std::io::Result<ExitStatus> {
        while !self.gone() {
            std::thread::sleep(CHECK_EVERY);
        }
        Err(Self::unknown_code())
    }

    fn process_id(&self) -> Option<u32> {
        u32::try_from(self.pid).ok()
    }
}
