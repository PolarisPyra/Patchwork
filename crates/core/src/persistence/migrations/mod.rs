//! Schema migrations: numbered SQL files in this folder, applied in order inside one
//! transaction each.
//!
//! The `schema_migrations` table records what has run (id, name, and when, in Unix seconds)
//! and is what gets checked: a database whose history is not a prefix of the list below was
//! written by a different build, and is refused rather than guessed at. `PRAGMA user_version`
//! is kept equal to the newest id, so an older build can still tell how far the database got.
//!
//! To change the schema, add the next numbered file and list it in `MIGRATIONS`. Never edit
//! or renumber a released one.

// Rust guideline compliant 2026-10-09

mod runner;

pub(super) use runner::run;
