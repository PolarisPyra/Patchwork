//! Threads: chats and tasks, their messages, and creating, changing and removing them.

// Rust guideline compliant 2026-10-09

mod attachments;
pub mod chat_tools;
pub mod delegate;
mod granular;
mod messages;
mod lifecycle;
mod list;
mod options;
mod standing;
pub(crate) mod title;
mod types;
mod worktree;
mod worktree_choice;
mod worktree_cleanup;
mod worktree_condition;
pub(crate) mod worktree_home;
mod worktree_include;
mod worktree_settle;

#[doc(inline)]
pub use messages::{
    Approval, ApprovalStatus, Attachment, AttachmentKind, BackgroundTask, CheckFailure,
    CheckStatus, EditLines, answer_with_attachments, image_media_type, MAX_TOOL_OUTPUT, Message,
    MessageStatus, Part, PlanStep, Question, QuestionOption, Refusal, Role, StepStatus, SubAgent,
    ReviewStatus, TaskStatus, TokenUsage, ToolCall, tool_output, TurnChecks, TurnReview, TurnTrees,
};
#[doc(inline)]
pub use granular::{AgentPick, Granular, Reserves};
pub(crate) use granular::{fall_back, fallback_of, project_key, reviewer_of, sub_agent_of};
#[doc(inline)]
pub use lifecycle::NewThread;
#[doc(inline)]
pub use standing::{Ask, AskKind, Standing, asks};
#[doc(inline)]
pub use attachments::is_long_paste;
pub use title::{title_from, without_leading_attachments};
#[doc(inline)]
pub use types::{AccessMode, Goal, GoalStatus, LastPlace, ResumePoint, Thread, ThreadParent};
pub(crate) use types::NOT_FOUND;
#[doc(inline)]
pub use worktree::{Worktree, WorktreeMark, WorktreeState};
