//! The Android panel beside the chat: the project's virtual and connected devices, starting and
//! stopping emulators, creating a device, and running the app on the one picked.
//!
//! It draws what `patchwork_server::android` reads and asks it to act. A running emulator picked
//! shows its screen in the panel, in a phone's frame.

// Rust guideline compliant 2026-10-09

mod create;
mod render;
mod rows;
mod screen;
mod state;

pub use state::AndroidPanel;
