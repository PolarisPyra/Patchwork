//! A thread's `/goal`: what the app keeps about it, how turns pursue it, and stopping it.

// Rust guideline compliant 2026-10-09

mod run;
mod state;
mod stop;

pub(in crate::orchestration) use run::{end_input, pursued, settle};
pub(in crate::orchestration) use stop::clear_command;
pub(crate) use state::{
    clear_dismissal, is_dismissed, next_goal, note_cut_off, signature, take_cut_off,
    update_is_dismissed, update_releases_held,
};
#[doc(inline)]
pub use stop::{Upkeep, is_upkeep, upkeep};
