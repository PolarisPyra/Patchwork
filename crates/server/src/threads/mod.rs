//! Threads: chats and tasks, their messages, and creating, changing and removing them.

mod attachments;
mod messages;
mod ops;
pub(crate) mod title;
mod types;

pub use messages::{
    Approval, ApprovalStatus, Attachment, AttachmentKind, BackgroundTask, CheckFailure,
    CheckStatus, EditLines, image_media_type, MAX_TOOL_OUTPUT, Message, MessageStatus, Part,
    PlanStep, Question, QuestionOption, Refusal, Role, StepStatus, SubAgent, TaskStatus,
    TokenUsage, ToolCall, tool_output, TurnChecks, TurnTrees,
};
pub use ops::NewThread;
pub use title::{title_from, without_leading_attachments};
pub use types::{AccessMode, Goal, GoalStatus, Thread, ThreadParent};

/// The error for a chat or task that no longer exists. People see chat for either.
pub(crate) const NOT_FOUND: &str = "Chat not found";
