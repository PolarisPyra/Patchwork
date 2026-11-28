//! The sidebar footer's Usage button and its popover: the plan limits of each connected agent
//! that has them, every agent at once, each limit a bar with when it starts over, and a refresh
//! button per agent that reads only that agent.

#![expect(
    clippy::unused_self,
    reason = "a view acts through its `Context<Self>`, so some of its methods never read the view itself"
)]

// Rust guideline compliant 2026-10-09

mod limits;
mod popover;

pub use limits::{Usage, UsageEvent};
