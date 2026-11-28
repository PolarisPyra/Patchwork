//! What Source Control reads from the window that holds it, and what it asks of it.

// Rust guideline compliant 2026-10-09

use gpui::{App, EventEmitter};
use patchwork_model::chats::Chat;
use patchwork_client::local::core::orchestration::changes::turn_files::TurnFileState;
use patchwork_client::local::core::source_control::{DiffSide, FileChange};

use crate::ChangesView;

/// What Source Control reads from the window that holds it, as it is when it draws.
pub trait SourceControlHost {
    /// The project's uncommitted changes, as last read.
    fn changes<'a>(&self, cx: &'a App) -> &'a [FileChange];

    /// The chat open in the window, if any.
    fn open_chat<'a>(&self, cx: &'a App) -> Option<&'a Chat>;

    /// The folder Git reads, which may be on an SSH host.
    fn git_root<'a>(&self, cx: &'a App) -> Option<&'a str>;

    /// Whether a project is open.
    fn has_project(&self, cx: &App) -> bool;

    /// The reply whose files the list is narrowed to, as its chat and message ids.
    fn change_focus(&self, cx: &App) -> Option<(String, String)>;

    /// The focused reply's files, whether or not they still have uncommitted changes.
    fn reply_files(&self, cx: &App) -> Vec<String>;

    /// What the focused reply's turn did to the file at `path`, when its snapshots tell.
    fn turn_file_state(&self, path: &str, cx: &App) -> Option<TurnFileState>;
}

/// What Source Control asks of the window that holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceControlEvent {
    /// Read the project's changes again.
    LoadGit,
    /// Start a task called `title` that is told `text`.
    StartTask {
        /// The task's name.
        title: String,
        /// What the task's agent is told.
        text: String,
    },
    /// Send `prompt` to chat `chat`.
    SendTo {
        /// The chat to send it to.
        chat: String,
        /// What it is sent.
        prompt: String,
    },
    /// Open chat `0`.
    OpenChat(String),
    /// Open the diff of the file at `path`, on `side` when one is given.
    OpenChange {
        /// The changed file.
        path: String,
        /// The part of the diff shown: what is staged or what is not.
        side: Option<DiffSide>,
    },
    /// Open the diff of the file at `path` from commit `since` to the file as it is now.
    OpenChangeSince {
        /// The changed file.
        path: String,
        /// The commit the diff starts at.
        since: String,
    },
    /// Stop narrowing the list to a reply's files.
    ClearChangeFocus,
}

impl EventEmitter<SourceControlEvent> for ChangesView {}
