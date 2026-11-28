//! What making a message, an approval or an attachment needs beyond its data: a new id, the
//! time, the disk, and the saved secrets. The types live in `patchwork_model`.

// Rust guideline compliant 2026-11-27

use std::collections::HashMap;

use crate::Error;
use patchwork_model::chats::{Approval, ApprovalStatus, Attachment, Message, MessageStatus, Part, Role};

/// Making an approval request.
pub(crate) trait ApprovalExt {
    /// A request waiting for the user's answer, with nothing but its title and detail.
    fn pending(title: String, detail: String) -> Self;
}

impl ApprovalExt for Approval {
    fn pending(title: String, detail: String) -> Self {
        Approval {
            id: crate::system::new_id(),
            title,
            detail,
            status: ApprovalStatus::Pending,
            questions: Vec::new(),
            answers: HashMap::new(),
            kind: None,
            suggestions: Vec::new(),
            rule: None,
            auto: false,
            secret: None,
            by_message: false,
        }
    }
}

/// Making an attachment from a path on disk.
pub(crate) trait AttachmentExt: Sized {
    /// Classifies a path that exists on disk.
    fn from_path(path: &str) -> Result<Self, Error>;
}

impl AttachmentExt for Attachment {
    fn from_path(path: &str) -> Result<Self, Error> {
        patchwork_harness::images::attachment(path)
    }
}
/// `text` with the saved secrets in it replaced, left as it is when it holds none.
fn hide(text: &mut String) {
    if let std::borrow::Cow::Owned(hidden) = crate::logging::hide(text) {
        *text = hidden;
    }
}

/// Making a message, and keeping secrets out of one.
pub(crate) trait MessageExt {
    /// A message with nothing in it yet but its text, made now with a new id.
    fn new(role: Role, text: String, status: MessageStatus) -> Self;

    /// Replaces every value of a secret Patchwork holds in what the reply says and what its
    /// tools printed, before the finished reply is saved: an agent that printed a key does
    /// not leave it in the database.
    fn hide_secrets(&mut self);
}

impl MessageExt for Message {
    fn new(role: Role, text: String, status: MessageStatus) -> Self {
        Message {
            id: crate::system::new_id(),
            role,
            text,
            created_at: crate::system::now_ms(),
            status,
            tools: Vec::new(),
            plan: Vec::new(),
            files: Vec::new(),
            error: None,
            approvals: Vec::new(),
            attachments: Vec::new(),
            steer: false,
            queued: false,
            background: Vec::new(),
            cut_off: false,
            refusal: None,
            parts: Vec::new(),
            finished_at: None,
            usage: None,
            turn: None,
            checks: None,
            review: None,
            cursor: None,
            via: None,
        }
    }

    fn hide_secrets(&mut self) {
        hide(&mut self.text);
        self.error.iter_mut().for_each(hide);
        for tool in &mut self.tools {
            hide(&mut tool.summary);
            tool.output.iter_mut().for_each(hide);
        }
        for task in &mut self.background {
            task.summary.iter_mut().for_each(hide);
            task.output.iter_mut().for_each(hide);
        }
        for approval in &mut self.approvals {
            hide(&mut approval.detail);
        }
        for part in &mut self.parts {
            match part {
                Part::Text { text } | Part::Thinking { text, .. } | Part::Plan { text } => {
                    hide(text);
                }
                Part::Compaction { summary, .. } => summary.iter_mut().for_each(hide),
                Part::Tool { .. } | Part::Approval { .. } => {}
            }
        }
    }
}
