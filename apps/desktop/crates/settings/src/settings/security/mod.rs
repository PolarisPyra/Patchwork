//! The Vault pane: what protects the saved secrets, the vault's password (set, changed,
//! removed and entered here) and its recovery key, the secrets that are saved, the copies
//! earlier versions left, and what the chat apps had Patchwork do.

// Rust guideline compliant 2026-10-09

mod locks;
mod rows;
mod vault;

use locks::{password_field, reveal_button};
use vault::{ASK, Change, FIELD_WIDTH};
pub use locks::render;
pub use vault::SecurityState;
