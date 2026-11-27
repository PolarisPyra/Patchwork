//! The Android SDK and what is built with it.
//!
//! Finding the SDK and running its tools, the project's framework and build variants, the virtual
//! devices and the system images they run.

// Rust guideline compliant 2026-10-09

pub(crate) mod avd;
pub(crate) mod detect;
pub(crate) mod gradle;
pub(crate) mod images;
pub(crate) mod locate;
pub(crate) mod tool;
