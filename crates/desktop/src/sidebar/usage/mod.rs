//! The sidebar footer's Usage button and its popover: the plan
//! limits of each connected agent that has them, as rings. A tab per agent on the left, each
//! ringed by its highest limit, and a refresh button that reads only the agent being looked at.

// Rust guideline compliant 2026-10-09

mod state;
mod ring;
mod popover;

pub use state::{UsageState};
pub use popover::{footer};
