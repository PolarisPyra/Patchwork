//! A chat's `/goal`: what the app keeps about it, how turns pursue it, and stopping it.

// Rust guideline compliant 2026-10-09

mod run;
mod state;
mod stop;

pub(in crate::orchestration) use run::{carry_on_later, settle};
pub(crate) use run::agent_text;
#[doc(inline)]
pub use run::is_goal_continuation;
pub(crate) use state::{clear_dismissal, is_dismissed, new_goal, note_cut_off, signature, take_cut_off};
#[doc(inline)]
pub use stop::{Upkeep, is_upkeep, upkeep};
