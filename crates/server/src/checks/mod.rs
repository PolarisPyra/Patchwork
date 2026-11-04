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

mod detect;
mod parse;
mod ops;
mod run;
#[cfg(test)]
mod tests;

pub use detect::{command, Command};
pub use parse::{failures, tests_ran};
pub use run::{Outcome, run};

use std::time::Duration;

/// How long the check command may run before it is stopped. Long enough for a cold build, short
/// enough that a hung command does not hold the reply's card at "Running" forever.
pub const LIMIT: Duration = Duration::from_secs(5 * 60);
/// Most of the output kept with a reply. The end is what names the failures.
const KEEP: usize = 16 << 10;
/// Most failures listed for one run.
const MOST: usize = 12;

pub(crate) use ops::worth_checking;
