//! One running turn: its shared state, and the spawn that starts it.

// Rust guideline compliant 2026-10-09

mod launch;
mod running;
mod turn;
mod watch;

pub(crate) use running::STILL_STARTING;
pub(in crate::orchestration) use running::Starting;
pub(in crate::orchestration) use running::{PendingApproval, RunningTurn};
pub use running::TurnRequest;
pub(in crate::orchestration) use turn::{Turn, assistant_message};
