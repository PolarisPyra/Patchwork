//! What the core tells the window: events about chats, and commands for the window alone.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;

use serde::Serialize;

use crate::permissions::RuleRef;
use crate::provider::ProviderSlashCommand;
use crate::chats::{
    Approval, BackgroundTask, Goal, Message, MessageStatus, PlanStep, Refusal, Chat, TokenUsage,
    ToolCall, TurnChecks, TurnTrees,
};

/// What the core tells an observer added with [`Core::observe`](crate::Core::observe).
#[derive(Debug, Clone, Copy)]
pub enum Signal<'a> {
    /// A chat event, as every observer hears of it.
    Event(&'a ChatEventEnvelope),
    /// The window sent message `message_id` in chat `chat_id`, which was saved without an event.
    MessageAdded {
        chat_id: &'a str,
        message_id: &'a str,
    },
}

/// Hears of each [`Signal`] with the core it came from.
pub type Observer = Box<dyn Fn(&crate::Core, &Signal<'_>) + Send + Sync>;

/// A chat event and the chat it is about.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatEventEnvelope {
    pub chat_id: String,
    pub event: ChatEvent,
}

/// What happened to a chat or its messages, which every observer hears of.
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ChatEvent {
    /// A new assistant message placeholder (status = streaming).
    MessageStarted {
        message: Message,
    },
    /// Append `text` to the message body.
    TextDelta {
        message_id: String,
        text: String,
    },
    /// Part of the agent's reasoning. Streamed pieces carry no times; the saved block
    /// carries when it started and ended.
    Thinking {
        message_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        started_at: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ended_at: Option<u64>,
    },
    ToolStarted {
        message_id: String,
        tool: ToolCall,
    },
    ToolFinished {
        message_id: String,
        tool_id: String,
        is_error: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
    },
    /// Replace the message's plan with `steps`.
    PlanUpdated {
        message_id: String,
        steps: Vec<PlanStep>,
    },
    FileTouched {
        message_id: String,
        path: String,
    },
    /// The agent is waiting for the user to allow or deny `approval`.
    ApprovalRequested {
        message_id: String,
        approval: Approval,
    },
    ApprovalResolved {
        message_id: String,
        approval_id: String,
        allowed: bool,
        /// The user's answers to the agent's questions, by question id.
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        answers: HashMap<String, Vec<String>>,
        /// The rule that answered it, or that the user saved when answering it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rule: Option<RuleRef>,
        /// Answered by a rule, without asking.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        auto: bool,
    },
    /// The agent's request for a secret was answered, or stopped waiting.
    SecretResolved {
        message_id: String,
        approval_id: String,
        status: crate::vault::requests::SecretStatus,
    },
    /// The chat's goal changed; None once it is cleared.
    GoalUpdated {
        goal: Option<Goal>,
    },
    /// The agent is summarizing the conversation to free up context. Saved as a compaction part
    /// where it happened; the message's next output ends it.
    Compacting {
        message_id: String,
    },
    /// The agent finished compacting, and goes on from `summary`, which its compaction part
    /// keeps.
    Compacted {
        message_id: String,
        summary: Option<String>,
    },
    /// The agent's model request failed or stalled and the agent is retrying it. Streamed, not
    /// saved; the message's next output ends it.
    Retrying {
        message_id: String,
        reason: String,
        /// Which try this is, when the agent says.
        attempt: Option<u32>,
        /// How many tries it makes, when the agent says.
        max_attempts: Option<u32>,
    },
    /// The agent has said nothing since `since` (ms since epoch), as a command that never ends
    /// leaves it. Streamed, not saved; the message's next output ends it.
    Quiet {
        message_id: String,
        since: u64,
    },
    /// Provider-native slash command names advertised by the active CLI session.
    SlashCommandsUpdated {
        commands: Vec<ProviderSlashCommand>,
    },
    /// The chat's custom agent changed (see `Chat::role`).
    RoleChanged {
        role: Option<String>,
    },
    /// The chat switched to a custom agent's model.
    ModelChanged {
        model: Option<String>,
    },
    /// The tokens a reply has used so far; replaces the count it had.
    Usage {
        message_id: String,
        usage: TokenUsage,
    },
    /// In plan mode, the agent proposed this plan (Markdown).
    PlanProposed {
        message_id: String,
        text: String,
    },
    /// A steering message cut the reply short.
    MessageCutOff {
        message_id: String,
    },
    /// A safety classifier blocked the reply, or part of it.
    Refused {
        message_id: String,
        refusal: Refusal,
    },
    /// Claude wrote another message of the reply to its session: the reply's cursor moves to it.
    Cursor {
        message_id: String,
        cursor: String,
    },
    /// A message sent mid-turn is now waiting for the turn to end (`queued`), or no longer is.
    MessageQueued {
        message_id: String,
        queued: bool,
    },
    /// A queued message was promoted into the running turn.
    MessageSteered {
        message_id: String,
    },
    /// A queued message was taken back, so it leaves the chat.
    MessageRemoved {
        message_id: String,
    },
    /// A queued message was edited in place.
    MessageEdited {
        message: Message,
    },
    /// A task the agent left running past its reply is listed under the message it started in.
    BackgroundTaskStarted {
        message_id: String,
        task: BackgroundTask,
    },
    /// A listed task changed: its status, description, or progress.
    BackgroundTaskUpdated {
        message_id: String,
        task: BackgroundTask,
    },
    /// A listed task ended, with its final status.
    BackgroundTaskEnded {
        message_id: String,
        task: BackgroundTask,
    },
    /// The project's snapshots from before and after the turn that wrote the message.
    TurnRecorded {
        message_id: String,
        turn: TurnTrees,
    },
    /// The project's check command started, or came to something, for the reply it checks.
    ChecksRecorded {
        message_id: String,
        checks: TurnChecks,
    },
    /// Terminal event for a turn. `status` is complete | error | cancelled.
    TurnCompleted {
        message_id: String,
        status: MessageStatus,
        error: Option<String>,
    },
    /// A chat the core created, such as a subagent's.
    ChatCreated {
        chat: Chat,
    },
    /// A chat the core made and then took back, such as one a scheduled task could not start in.
    ChatDeleted,
    /// The chat was named after what its first message is about.
    TitleUpdated {
        title: String,
    },
    /// The chat apps the chat is continued on changed.
    ChatAppsChanged,
    /// The chat's agent, model or access was changed from a chat app, or it was set
    /// there to resume once its usage limit resets.
    OptionsChanged,
}

impl ChatEvent {
    /// The saved message the event changes, when it changes one that exists already.
    ///
    /// None for a message it adds (`MessageStarted`) or removes (`MessageRemoved`), and for
    /// events about the chat as a whole.
    #[must_use]
    pub fn message_id(&self) -> Option<&str> {
        match self {
            Self::MessageEdited { message } => Some(&message.id),
            Self::TextDelta { message_id, .. }
            | Self::Thinking { message_id, .. }
            | Self::ToolStarted { message_id, .. }
            | Self::ToolFinished { message_id, .. }
            | Self::PlanUpdated { message_id, .. }
            | Self::FileTouched { message_id, .. }
            | Self::TurnRecorded { message_id, .. }
            | Self::ChecksRecorded { message_id, .. }
            | Self::ApprovalRequested { message_id, .. }
            | Self::ApprovalResolved { message_id, .. }
            | Self::SecretResolved { message_id, .. }
            | Self::Usage { message_id, .. }
            | Self::MessageQueued { message_id, .. }
            | Self::MessageSteered { message_id }
            | Self::MessageCutOff { message_id }
            | Self::Compacting { message_id }
            | Self::Compacted { message_id, .. }
            | Self::Retrying { message_id, .. }
            | Self::Quiet { message_id, .. }
            | Self::Refused { message_id, .. }
            | Self::Cursor { message_id, .. }
            | Self::PlanProposed { message_id, .. }
            | Self::BackgroundTaskStarted { message_id, .. }
            | Self::BackgroundTaskUpdated { message_id, .. }
            | Self::BackgroundTaskEnded { message_id, .. }
            | Self::TurnCompleted { message_id, .. } => Some(message_id),
            Self::MessageStarted { .. }
            | Self::MessageRemoved { .. }
            | Self::GoalUpdated { .. }
            | Self::SlashCommandsUpdated { .. }
            | Self::RoleChanged { .. }
            | Self::ModelChanged { .. }
            | Self::ChatCreated { .. }
            | Self::ChatDeleted
            | Self::TitleUpdated { .. }
            | Self::ChatAppsChanged
            | Self::OptionsChanged => None,
        }
    }

    /// The message whose shape for phones and the web client the event adds or changes.
    ///
    /// Those clients show a message's text, parts, status, files and usage, not its snapshots,
    /// checks, background tasks, cursor or passing notices.
    #[must_use]
    pub fn client_message_id(&self) -> Option<&str> {
        match self {
            Self::MessageStarted { message } => Some(&message.id),
            Self::TurnRecorded { .. }
            | Self::ChecksRecorded { .. }
            | Self::BackgroundTaskStarted { .. }
            | Self::BackgroundTaskUpdated { .. }
            | Self::BackgroundTaskEnded { .. }
            | Self::Cursor { .. }
            | Self::Retrying { .. }
            | Self::Quiet { .. } => None,
            _ => self.message_id(),
        }
    }

    /// Whether the event streams text or reasoning into a message, which the window applies in
    /// place.
    #[must_use]
    pub fn is_stream_delta(&self) -> bool {
        matches!(self, Self::TextDelta { .. } | Self::Thinking { .. })
    }

    /// Whether chat apps that continue the chat hear of the event.
    #[must_use]
    pub fn reaches_chat_apps(&self) -> bool {
        matches!(
            self,
            Self::MessageStarted { .. }
                | Self::TextDelta { .. }
                | Self::ToolStarted { .. }
                | Self::ApprovalRequested { .. }
                | Self::ApprovalResolved { .. }
                | Self::TurnCompleted { .. }
                | Self::ChatDeleted
        )
    }

    /// Whether the event can make a notification: a request, an ended reply, or a goal.
    #[must_use]
    pub fn may_notify(&self) -> bool {
        matches!(
            self,
            Self::ApprovalRequested { .. }
                | Self::TurnCompleted { .. }
                | Self::GoalUpdated { goal: Some(_) }
        )
    }
}

/// Something the core asks the window to do. Only the desktop receives these.
#[derive(Debug)]
pub enum WindowCommand {
    /// Show a notification for the chat while Patchwork is not the focused window.
    Notify {
        chat_id: String,
        notice: crate::notifications::DesktopNotice,
    },
    /// The settings were changed from outside the window, as a chat app does: read them anew
    /// and apply the theme.
    ReloadSettings,
    /// A quilt was made or changed: the Quilts page reads them anew.
    ReloadQuilts,
    /// Open the chat and come forward, as a chat app asked.
    OpenChat { chat_id: String },
    /// Restart into the installed update; the core checked that nothing is lost by it.
    RestartToUpdate,
    /// Show something other than a chat, as a chat app asked.
    Show { place: Place },
    /// Where updating stands changed, so the window draws it anew.
    RedrawUpdate,
    /// A second launch of the app asked this window to come forward.
    Raise,
    /// Ask the person a sign-in question git or ssh waits on.
    AskCredentials(crate::source_control::CredentialRequest),
}

/// What the core sends the window: chat events to apply, and commands to carry out.
#[derive(Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "events, nearly all that is sent, went by value before; boxing would allocate per delta"
)]
pub enum ToWindow {
    Event(ChatEventEnvelope),
    Command(WindowCommand),
}

/// What a chat app asked the window to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Place {
    /// The project, on its newest chat.
    Project { id: String },
    /// A file of the project, in the file panel.
    #[serde(rename_all = "camelCase")]
    File { project_id: String, path: String },
    /// Settings, on the section with this id, or where it last was.
    Settings { section: Option<String> },
}
