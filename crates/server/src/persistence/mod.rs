//! App state persisted in SQLite (`patchwork.db` in the data folder): opening the file and
//! migrating it, and how values are written to columns as text, are here.
//!
//! - `store`: the `Store` the rest of the app uses, with its in-memory cache.
//! - `migrations`: the schema, as numbered SQL files, and the record of which have run.
//! - `settings`, `threads`: how each table's rows map to the app's types.
//! - `legacy`: data from before the database, or from before it moved.

// Rust guideline compliant 2026-10-09

mod legacy;
mod migrations;
mod scheduled;
mod settings;
mod store;
mod threads;

#[doc(inline)]
pub use store::Store;
use store::{dec, enc, err, from_json, json};

#[cfg(test)]
mod tests;
