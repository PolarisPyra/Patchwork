//! Patchwork's Android client: a GPUI app that connects to a running Patchwork.

// Rust guideline compliant 2026-10-09

#[cfg(target_os = "android")]
mod app;
mod nav;

pub use nav::Nav;

#[cfg(target_os = "android")]
pub use app::android_main;
