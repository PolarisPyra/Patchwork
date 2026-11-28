//! What sits between the transcript and the composer.
//!
//! The waiting request, the plan, the goal, the agent's questions, queued messages, the chat
//! carried on in a terminal, and the strips about the chat's folder and worktree.

// Rust guideline compliant 2026-11-27

pub mod approval_banner;
pub mod agent_questions;
pub mod goal_bar;
pub mod in_terminal;
pub mod interrupted_reply;
pub mod missing_folder;
pub mod plan_steps;
pub mod proposed_plan;
pub mod queued_messages;
pub mod secret_request;
pub mod worktree_status;
