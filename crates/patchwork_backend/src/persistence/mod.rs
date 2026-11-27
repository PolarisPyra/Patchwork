//! App state persisted in SQLite (`patchwork.db` in the data folder): opening the file and
//! migrating it, and how values are written to columns as text, are here.
//!
//! - `store`: the `Store` the rest of the app uses, with its in-memory cache.
//! - `migrations`: the schema, as numbered SQL files, and the record of which have run.
//! - `settings`, `chats`, `costs`, `quilts`: how each table's rows map to the app's types.
//! - `devices`: the devices paired with this computer and the codes that pair them.

// Rust guideline compliant 2026-10-09

mod quilts;
mod agents;
mod costs;
mod devices;
mod folders;
mod migrations;
mod scheduled;
mod settings;
mod store;
mod chats;

#[doc(inline)]
pub(crate) use store::DB_FILE;
pub use store::Store;
use store::{dec, enc, err, from_json, json};
