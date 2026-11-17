//! What the terminal panel is sent: the terminals, an attached window's screen and output, and the
//! shells it can start.

// Rust guideline compliant 2026-10-09

use serde::Serialize;

/// A terminal, running or ended, as the panel lists it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalInfo {
    pub id: u32,
    /// The shell's file name, such as "zsh", or the SSH host's alias, for the terminal's tab.
    pub name: String,
    pub cwd: String,
    /// The SSH host the shell runs on, or None for this computer.
    pub host: Option<String>,
    pub running: bool,
    /// How the shell exited, once it has: its code, or None when it could not be told.
    pub exit_code: Option<u32>,
}

/// A window attached to a terminal: what the screen holds so far, and the attachment, which
/// types into the shell until another window attaches.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachedTerminal {
    pub attachment: u32,
    pub info: TerminalInfo,
    /// The shell's output so far, which redraws the screen and its scrollback.
    pub screen: String,
}

/// What an attached window is sent after the screen.
#[derive(Debug, Clone, Serialize)]
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
#[derive(Debug, Clone, Serialize)]
pub struct TerminalShell {
    pub path: String,
    /// The shell's file name, such as "zsh".
    pub name: String,
}
