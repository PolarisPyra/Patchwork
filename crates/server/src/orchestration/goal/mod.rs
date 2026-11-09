//! A thread's `/goal`: what the app keeps about it, how turns pursue it, and stopping it.

mod run;
mod state;
mod stop;

pub(in crate::orchestration) use run::{end_input, pursued, settle};
pub(in crate::orchestration) use stop::clear_command;
pub(crate) use state::{
    clear_dismissal, is_dismissed, next_goal, signature, update_is_dismissed, update_releases_held,
};
pub use stop::is_upkeep;
