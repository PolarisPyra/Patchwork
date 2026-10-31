//! What the backend streams to the UI while agents work.

use std::collections::{HashMap};

use serde::Serialize;
use ts_rs::TS;

use crate::permissions::RuleRef;
use crate::provider::ProviderSlashCommand;
use crate::threads::{
    Approval, BackgroundTask, Goal, Message, MessageStatus, PlanStep, Refusal, Thread, TokenUsage,
    ToolCall, TurnTrees,
};

/// Emitted on the Tauri event channel `"agent-event"`.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AgentEventEnvelope {
    pub thread_id: String,
    pub event: AgentEvent,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentEvent {
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
    /// The thread's goal changed; None once it is cleared.
    GoalUpdated {
        goal: Option<Goal>,
    },
    /// The reply ended and the agent is checking whether it met the goal (Claude). The
    /// message's next output ends the check.
    GoalChecking {
        message_id: String,
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
        attempt: u32,
        max_attempts: u32,
    },
    /// Provider-native slash command names advertised by the active CLI session.
    SlashCommandsUpdated {
        commands: Vec<ProviderSlashCommand>,
    },
    /// The chat's custom agent changed (see `Thread::role`).
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
    /// A message sent mid-turn is now waiting for the turn to end (`queued`), or no longer is.
    MessageQueued {
        message_id: String,
        queued: bool,
    },
    /// A queued message was promoted into the running turn.
    MessageSteered {
        message_id: String,
    },
    /// A queued message was taken back, so it leaves the thread.
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
    /// Terminal event for a turn. `status` is complete | error | cancelled.
    TurnCompleted {
        message_id: String,
        status: MessageStatus,
        error: Option<String>,
    },
    /// A thread the server created, such as a subagent's.
    ThreadCreated {
        thread: Thread,
    },
    /// The thread was named after what its first message is about.
    TitleUpdated {
        title: String,
    },
}
