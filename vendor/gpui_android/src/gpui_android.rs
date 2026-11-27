//! Native Android platform support for GPUI.
//!
//! Patchwork wrote this crate; it is not part of the Zed sources the rest of `vendor/` is copied
//! from. It connects GPUI to a `NativeActivity` through `android-activity`. The app's
//! `android_main` hands its [`AndroidApp`](android_activity::AndroidApp) to [`init`], and
//! `gpui_platform::current_platform` then returns [`AndroidPlatform`]. Rendering goes through
//! `gpui_wgpu` on Vulkan or GLES, and text through `cosmic-text`.

// Rust guideline compliant 2026-10-09

#[cfg(target_os = "android")]
mod android;

#[cfg(target_os = "android")]
pub use android::{AndroidPlatform, current_platform, init};

#[cfg(target_os = "android")]
pub use android_activity;
