//! The integrated terminal: the shell it starts, the running shells, each in a pseudo-terminal,
//! and the processes each one owns.
//!
//! A terminal belongs to Patchwork rather than to the window showing it: a window that reloads
//! attaches again and gets the screen back. One no window shows is closed after two hours idle
//! at its prompt, and closing one ends every process it started.

mod activity;
mod launch;
mod pty;
mod range;
mod session;

pub use launch::{Launch, launch, shells};
pub use range::sweep;
pub use session::{FrameSink, Terminals};

use serde::Serialize;
use ts_rs::TS;

/// A terminal, running or ended, as the panel lists it.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "terminal.ts")]
#[serde(rename_all = "camelCase")]
pub struct TerminalInfo {
    pub id: u32,
    /// The shell's file name, such as "zsh", for the terminal's tab.
    pub name: String,
    pub cwd: String,
    pub running: bool,
    /// How the shell exited, once it has: its code, or None when it could not be told.
    pub exit_code: Option<u32>,
}

/// A window attached to a terminal: what the screen holds so far, and the attachment, which
/// types into the shell until another window attaches.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "terminal.ts")]
#[serde(rename_all = "camelCase")]
pub struct AttachedTerminal {
    pub attachment: u32,
    pub info: TerminalInfo,
    /// The shell's output so far, which redraws the screen and its scrollback.
    pub screen: String,
}

/// What an attached window is sent after the screen.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "terminal.ts")]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TerminalFrame {
    Output {
        data: String,
    },
    /// The shell exited; output its programs still print follows.
    Exit {
        code: Option<u32>,
    },
}

/// A shell a terminal can start, for the shell choice on its New terminal button.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "terminal.ts")]
pub struct TerminalShell {
    pub path: String,
    /// The shell's file name, such as "zsh".
    pub name: String,
}
