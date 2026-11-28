//! Chats and their messages, as they are saved and sent.

// Rust guideline compliant 2026-11-27

mod chat;
mod messages;
mod standing;

#[doc(inline)]
pub use chat::{
    AccessMode, Chat, ChatParent, Goal, GoalStatus, LastPlace, ResumePoint, client_access_mode,
};
#[doc(inline)]
pub use messages::{
    Approval, ApprovalStatus, Attachment, AttachmentKind, BackgroundTask, CheckFailure,
    CheckStatus, EditLines, MAX_TOOL_OUTPUT, Message, MessageStatus, Part, PlanStep, Question,
    QuestionOption, Refusal, ReviewStatus, Role, StepStatus, SubAgent, TaskStatus, TokenUsage,
    ToolCall, TurnChecks, TurnReview, TurnTrees, answer_with_attachments, image_media_type,
    tool_output,
};
#[doc(inline)]
pub use standing::{Ask, AskKind, ChatStatus, Standing, asks};
