//! Native Android platform support for GPUI.
//!
//! GPUI ships no Android backend, so Patchwork has its own. It connects GPUI to a
//! `NativeActivity` through `android-activity`. The app's `android_main` hands its
//! [`AndroidApp`](android_activity::AndroidApp) to [`init`] and passes an [`AndroidPlatform`] to
//! `Application::with_platform`. Rendering goes through `gpui_wgpu` on Vulkan, and text through
//! `cosmic-text`.

// Rust guideline compliant 2026-10-09

#[cfg(target_os = "android")]
mod android;

#[cfg(target_os = "android")]
pub use android::{AndroidPlatform, current_platform, init};

#[cfg(target_os = "android")]
pub use android_activity;
