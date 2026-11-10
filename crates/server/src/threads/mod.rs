//! Threads: chats and tasks, their messages, and creating, changing and removing them.

// Rust guideline compliant 2026-10-09

mod attachments;
mod messages;
mod lifecycle;
mod list;
mod options;
pub(crate) mod title;
mod types;

#[doc(inline)]
pub use messages::{
    Approval, ApprovalStatus, Attachment, AttachmentKind, BackgroundTask, CheckFailure,
    CheckStatus, EditLines, image_media_type, MAX_TOOL_OUTPUT, Message, MessageStatus, Part,
    PlanStep, Question, QuestionOption, Refusal, Role, StepStatus, SubAgent, TaskStatus,
    TokenUsage, ToolCall, tool_output, TurnChecks, TurnTrees,
};
#[doc(inline)]
pub use lifecycle::NewThread;
#[doc(inline)]
pub use title::{title_from, without_leading_attachments};
#[doc(inline)]
pub use types::{AccessMode, Goal, GoalStatus, Thread, ThreadParent};
pub(crate) use types::NOT_FOUND;
