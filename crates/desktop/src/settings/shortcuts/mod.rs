//! Settings → Shortcuts: every command a key can run, by page, with its key. Clicking one records
//! the next key pressed as its new one; the changes are kept in the app state under `keybindings`,
//! and the window's keymap is built from them with [`bind_keys`].

// Rust guideline compliant 2026-10-09

mod keys;
mod render;

pub use keys::{bind_keys, key_label};
pub use render::{ShortcutsState, kbd, render};
