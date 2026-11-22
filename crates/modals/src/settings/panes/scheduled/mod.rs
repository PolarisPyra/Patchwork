//! Settings → Scheduled: prompts that start a task at a fixed time,
//! in a project or in a chat. The app runs them on a timer of its own while it is open.

// Rust guideline compliant 2026-10-09

mod tasks;
mod editor;
mod layout;

pub use layout::render;
pub use tasks::ScheduledState;
