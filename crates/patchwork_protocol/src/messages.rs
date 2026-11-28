//! Messages as clients see them, with their parts in the web client's kinds.

// Rust guideline compliant 2026-10-09

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Who wrote a message, and what an agent asks the person: the same on the wire as they are
/// saved.
pub use patchwork_model::chats::{Question, QuestionOption, Role};

/// Where a message is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageStatus {
    /// Waiting to be sent after the running turn.
    Queued,
    Streaming,
    Complete,
    Failed,
    /// Stopped by the person, or cut short by the app closing.
    Stopped,
}

/// A message, as the web client's `Message` type has it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub chat_id: String,
    pub role: Role,
    pub parts: Vec<Part>,
    pub status: MessageStatus,
    pub files: Vec<String>,
    pub created_at: u64,
    pub ended_at: Option<u64>,
    pub usage: Option<Usage>,
    /// Cut short by the app closing; only written when true.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub interrupted: bool,
}

/// Tokens a reply used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    /// The size of the context the agent is working in.
    pub context: u64,
    /// How large that context may grow, when the agent reports it.
    pub window: Option<u64>,
}

/// One piece of a message, in the order the client shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Part {
    Text {
        text: String,
    },
    Reasoning {
        text: String,
    },
    Tool {
        id: String,
        name: String,
        summary: String,
        status: ToolStatus,
        output: String,
    },
    Approval {
        id: String,
        title: String,
        detail: String,
        questions: Vec<Question>,
        state: ApprovalState,
        /// The answers by question id.
        answers: BTreeMap<String, Vec<String>>,
    },
    /// The agent's checklist.
    Plan {
        steps: Vec<PlanStep>,
    },
    Error {
        text: String,
    },
}

/// Where a tool call is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolStatus {
    Running,
    Done,
    Failed,
}

/// What became of an approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalState {
    Pending,
    Allowed,
    Denied,
}

/// One step of an agent's checklist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub text: String,
    pub status: PlanStepStatus,
}

/// Where a checklist step is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlanStepStatus {
    Pending,
    InProgress,
    Completed,
}
