//! What the composer reads from the window it sits in, and what it asks the window to do.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, App, FocusHandle, Window};
use patchwork_client::local::core::chats::{Attachment, Chat, NewChat};
use patchwork_client::local::core::projects::Project;
use patchwork_design_theme::Theme;

/// The worktree's pieces the window draws for the composer: its strip, the files it changed,
/// the worktree picker, and the line that says how the chat's folder works.
pub struct WorktreeParts {
    /// The worktree's status strip, while the chat has one.
    pub strip: Option<AnyElement>,
    /// The files the worktree changed.
    pub files: Option<AnyElement>,
    /// The worktree picker on the composer's outer row.
    pub pick: AnyElement,
    /// What the chat's folder is, while there is something to say about it.
    pub told: Option<AnyElement>,
}

impl std::fmt::Debug for WorktreeParts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorktreeParts")
            .field("strip", &self.strip.is_some())
            .field("files", &self.files.is_some())
            .field("told", &self.told.is_some())
            .finish_non_exhaustive()
    }
}

/// A message the composer sends into the open chat, or into a new chat made with `new`.
#[derive(Debug)]
pub struct Outgoing {
    /// What is written.
    pub text: String,
    /// What is attached.
    pub attachments: Vec<Attachment>,
    /// The chat to make when none is open.
    pub new: NewChat,
    /// The custom agent the chat runs as.
    pub role: Option<String>,
    /// Waits for the running turn rather than steering it.
    pub queue: bool,
}

/// The composer's view of the window: the open chat, its project, and the panels above the
/// composer that change what sending does.
pub trait ChatHost {
    /// The open chat.
    fn chat<'a>(&self, cx: &'a App) -> Option<&'a Chat>;

    /// The project of the open chat, or the active one.
    fn project<'a>(&self, cx: &'a App) -> Option<&'a Project>;

    /// The SSH host the chat runs on; none is this computer.
    fn machine(&self, cx: &App) -> Option<String>;

    /// The project's folder is gone, so nothing can run.
    fn folder_missing(&self, cx: &App) -> bool;

    /// An agent's page is shown, where a message starts a run of the agent.
    fn agent_shown(&self, cx: &App) -> bool;

    /// Messages wait for the running turn.
    fn has_queued(&self, cx: &App) -> bool;

    /// A queued message is being edited in the composer.
    fn editing_queued(&self, cx: &App) -> bool;

    /// The composer answers the agent's question.
    fn answering(&self, cx: &App) -> bool;

    /// Where the keys go when a menu closes and nothing else had them.
    fn focus(&self, cx: &App) -> Option<FocusHandle>;

    /// The worktree's pieces, drawn by the window.
    fn worktree(&self, t: &Theme, cx: &mut App) -> WorktreeParts;

    /// Answers the agent's question with `answer`.
    fn answer(&self, answer: String, cx: &mut App);

    /// Starts a run of the shown agent; false when it could not start.
    fn send_to_agent(
        &self,
        text: String,
        attachments: Vec<Attachment>,
        new: NewChat,
        cx: &mut App,
    ) -> bool;

    /// Sends `message`, first asking what a new granular chat runs with when its project asks.
    fn send(&self, message: Outgoing, window: &mut Window, cx: &mut App);
}

/// What the composer asks of the window.
#[derive(Clone, Debug)]
pub enum ChatEvent {
    /// Stop the running reply.
    Stop,
    /// Steer the running reply with the first queued message.
    SteerFirstQueued,
    /// Put the last queued message into the composer to edit.
    EditLatestQueued,
    /// Save the queued message being edited.
    UpdateQueued,
    /// Close the usage popover.
    DismissUsage,
    /// Send a command to the agent, such as `/compact`.
    Command(&'static str),
    /// Start a new chat.
    NewChat,
    /// Open `Some` project's newest chat, or a new chat in no project.
    SelectProject(Option<String>),
    /// Read the chats again.
    Reload,
    /// Open Settings at a page.
    OpenSettings(&'static str),
}
