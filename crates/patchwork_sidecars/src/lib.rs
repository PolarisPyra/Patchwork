//! The machines beside Patchwork's core, each driven from its own module: Android devices and
//! emulators ([`android`]), a browser ([`browser`]), the integrated terminal's shells
//! ([`terminal`]), and the computer's screen, pointer and keyboard ([`computer`]).
//!
//! None of them knows the core. What one needs from the app, it asks through a trait the core
//! implements, such as [`android::Host`].

// Rust guideline compliant 2026-10-09

pub mod android;
pub mod browser;
pub mod computer;
pub mod terminal;

use patchwork_platform as system;
pub use patchwork_platform::Error;
