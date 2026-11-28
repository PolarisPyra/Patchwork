//! Chats: chats and tasks, their messages, and creating, changing and removing them.

// Rust guideline compliant 2026-10-09

mod attachments;
pub mod tools;
pub mod delegate;
mod granular;
mod messages;
mod lifecycle;
mod list;
mod options;
pub(crate) mod title;
mod types;

#[doc(inline)]
pub use patchwork_model::chats::{
    Approval, ApprovalStatus, Attachment, AttachmentKind, BackgroundTask, CheckFailure,
    CheckStatus, EditLines, MAX_TOOL_OUTPUT, Message, MessageStatus, Part, PlanStep, Question,
    QuestionOption, Refusal, ReviewStatus, Role, StepStatus, SubAgent, TaskStatus, TokenUsage,
    ToolCall, TurnChecks, TurnReview, TurnTrees, answer_with_attachments, image_media_type,
    tool_output,
};
pub(crate) use messages::{ApprovalExt, AttachmentExt, MessageExt};
#[doc(inline)]
pub use granular::{AgentPick, Granular, Reserves};
pub(crate) use granular::{project_key, sub_agent_of};
#[doc(inline)]
pub use lifecycle::NewChat;
#[doc(inline)]
pub use patchwork_model::chats::{Ask, AskKind, Standing, asks};
#[doc(inline)]
pub use attachments::is_long_paste;
pub use title::{title_from, without_leading_attachments};
#[doc(inline)]
pub use patchwork_model::chats::{
    AccessMode, Chat, ChatParent, Goal, GoalStatus, LastPlace, ResumePoint,
};
pub(crate) use types::{ChatExt, NOT_FOUND};
