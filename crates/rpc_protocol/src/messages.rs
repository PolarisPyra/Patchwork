//! Messages as clients see them, with their parts in the web client's kinds.

// Rust guideline compliant 2026-10-09

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Who wrote a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    User,
    Assistant,
}

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

/// Something an agent asks the person, with the options it offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    /// The key its answer goes back under.
    pub id: String,
    pub header: String,
    pub question: String,
    pub options: Vec<QuestionOption>,
    pub multi_select: bool,
}

/// One option of a [`Question`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
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

#[cfg(test)]
mod tests {
    use super::{Message, MessageStatus, Part, Role};

    fn message(interrupted: bool) -> Message {
        Message {
            id: "m-1".into(),
            chat_id: "c-1".into(),
            role: Role::User,
            parts: vec![Part::Reasoning { text: "x".into() }],
            status: MessageStatus::Stopped,
            files: Vec::new(),
            created_at: 1,
            ended_at: None,
            usage: None,
            interrupted,
        }
    }

    #[test]
    fn interrupted_is_written_only_when_true() {
        assert_eq!(
            serde_json::to_string(&message(false)).expect("JSON"),
            r#"{"id":"m-1","chatId":"c-1","role":"user","parts":[{"kind":"reasoning","text":"x"}],"status":"stopped","files":[],"createdAt":1,"endedAt":null,"usage":null}"#
        );
        let interrupted = serde_json::to_string(&message(true)).expect("JSON");
        assert!(interrupted.ends_with(r#","usage":null,"interrupted":true}"#));
    }

    #[test]
    fn messages_read_back_what_they_write() {
        let written = serde_json::to_value(message(true)).expect("JSON");
        let read: Message = serde_json::from_value(written).expect("a message");
        assert_eq!(read, message(true));
    }
}
