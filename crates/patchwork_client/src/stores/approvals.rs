//! The approvals and questions the agents wait on.

// Rust guideline compliant 2026-10-09

use patchwork_protocol::messages::{ApprovalState, Message, Part};

/// An approval or a question an agent waits on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingApproval {
    /// The chat it is in.
    pub chat_id: String,
    /// The message it belongs to.
    pub message_id: String,
    /// The approval's own id, which an answer names.
    pub id: String,
    /// What the agent asks to do.
    pub title: String,
    /// More of it, such as the command.
    pub detail: String,
}

/// Approvals the agents wait on, in every chat a push or a load has shown.
#[derive(Debug, Clone, Default)]
pub struct ApprovalsStore {
    /// The approvals, in the order they were seen.
    pub list: Vec<PendingApproval>,
}

impl ApprovalsStore {
    /// Keeps `message`'s waiting approvals, in place of what it had before.
    pub fn note(&mut self, message: &Message) {
        self.forget_message(&message.id);
        self.list
            .extend(message.parts.iter().filter_map(|part| match part {
                Part::Approval {
                    id,
                    title,
                    detail,
                    state: ApprovalState::Pending,
                    ..
                } => Some(PendingApproval {
                    chat_id: message.chat_id.clone(),
                    message_id: message.id.clone(),
                    id: id.clone(),
                    title: title.clone(),
                    detail: detail.clone(),
                }),
                _ => None,
            }));
    }

    /// Drops approval `id`, once it is answered.
    pub fn remove(&mut self, id: &str) {
        self.list.retain(|a| a.id != id);
    }

    /// Drops every approval in chat `chat_id`.
    pub fn forget_chat(&mut self, chat_id: &str) {
        self.list.retain(|a| a.chat_id != chat_id);
    }

    /// Drops every approval on message `message_id`.
    pub fn forget_message(&mut self, message_id: &str) {
        self.list.retain(|a| a.message_id != message_id);
    }
}
