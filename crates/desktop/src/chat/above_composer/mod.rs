//! What sits between the transcript and the composer: the waiting request, the plan, the goal,
//! the agent's questions, queued messages, and the strips about the chat's folder and worktree.

// Rust guideline compliant 2026-10-09

mod approval_banner;
pub mod agent_questions;
pub mod goal_bar;
pub mod interrupted_reply;
pub mod missing_folder;
pub mod plan_steps;
pub mod proposed_plan;
pub mod queued_messages;
pub mod secret_request;
pub mod worktree_status;
