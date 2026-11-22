//! The sidebar footer's Usage button and its popover: the plan
//! limits of each connected agent that has them, as rings. A tab per agent on the left, each
//! ringed by its highest limit, and a refresh button that reads only the agent being looked at.

// Rust guideline compliant 2026-10-09

mod limits;
mod ring;
mod popover;

pub use limits::{UsageState};
pub use popover::{footer};
