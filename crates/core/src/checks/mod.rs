//! The project's own check command: what to run, running it after a turn, and reading what it
//! said about the turn.
//!
//! ```text
//! detect  package.json (check, else test), Cargo.toml, pyproject.toml, go.mod, Makefile,
//!         mix.exs — the first one the project has, run with the package manager its lockfile
//!         names
//! run     through the same shell an agent's commands run in, in the project's folder, with a
//!         time limit; output is kept to its last bytes
//! parse   the failures the output names, each on its own; when it failed and named none, one
//!         claim for the command itself
//! ```
//!
//! Checks are the half of "nothing lands until you say so" that a person cannot do by reading a
//! diff: a reply says whether the project's tests still pass, and says "not run" rather than
//! nothing when they didn't run. A check that did not finish never counts as a pass.

// Rust guideline compliant 2026-10-09

mod detect;
mod parse;
mod reply;
mod run;
#[cfg(test)]
mod tests;

#[doc(inline)]
pub use detect::{command, command_at, Command};
#[doc(inline)]
pub use parse::{failures, tests_ran};
#[doc(inline)]
pub use run::{LIMIT, Outcome, run};
use run::KEEP;
pub(crate) use reply::worth_checking;
