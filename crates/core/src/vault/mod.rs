//! The vault: the secrets Patchwork keeps, encrypted in this instance's database.
//!
//! Each secret is sealed with XChaCha20-Poly1305 under one random data key, with the secret's
//! name bound in, so a row cannot be moved to another name. The data key is in the database
//! too, sealed under a wrapping key that is kept outside it, in the system credential store. A
//! system without one makes no vault until a password is set, and saves nothing before that.
//! An earlier version kept the key in a file of the config folder there: that vault still
//! opens, and loses the file when its password is set. A debug build has its own database and
//! its own wrapping key, so it shares no secret with the released app.
//!
//! With a password set, the wrapping key is derived from the password with Argon2id and kept
//! nowhere: the vault waits, each run, until the password is entered. Setting, changing or
//! removing the password seals every secret under a new data key, so no earlier key opens it.
//! Setting it also seals the data key under a recovery key, which the user is shown once and
//! Patchwork keeps nowhere: it opens the vault when the password is lost.
//!
//! A vault that cannot be unlocked is never replaced: it stays locked for the run, and nothing
//! can be saved until the wrapping key is back.

// Rust guideline compliant 2026-10-09

mod crypt;
mod known;
mod password;
mod recovery;
mod secrets;
mod state;

pub use password::VaultSettings;
pub use secrets::Saved;
pub(crate) use secrets::{get, put, remove, take_old};
pub(crate) use state::open;
pub use state::{Method, Status, status};
