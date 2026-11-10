//! Settings → Scheduled: prompts that start a task at a fixed time,
//! in a project or in a chat. The app runs them on a timer of its own while it is open.

// Rust guideline compliant 2026-10-09

mod state;
mod editor;
mod render;

pub use render::render;
pub use state::ScheduledState;
