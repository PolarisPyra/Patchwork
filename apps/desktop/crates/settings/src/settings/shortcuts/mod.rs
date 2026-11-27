//! Settings → Shortcuts: every command a key can run, by page, with its keys. Picking a command
//! shows its keys and what else shares them. A command's keys are changed by recording the next
//! keys pressed; the changes are kept in the app state under `keybindings`, and the window's
//! keymap is built from them with [`bind_keys`].

// Rust guideline compliant 2026-10-09

mod bindings;
mod chord;
mod inspector;
mod list;
mod live;

pub use bindings::{bind_keys, command_action, key_label, page_commands, searchable};
pub use list::{ShortcutsState, kbd, render};
