//! The devices themselves.
//!
//! The ones adb lists, acting on one, its log, phones over Wi-Fi, and a device's screen in the
//! panel.

// Rust guideline compliant 2026-10-09

pub(crate) mod adb;
pub(crate) mod grpc;
pub(crate) mod connected;
pub(crate) mod logcat;
pub(crate) mod mirror;
pub(crate) mod screen;
pub(crate) mod wifi;
