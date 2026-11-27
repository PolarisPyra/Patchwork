//! Patchwork's dependencies, built once into a shared library for debug builds.
//!
//! `cargo build --features dynamic` links the app against this library instead of copying every
//! dependency into the binary, so a rebuild after an edit relinks only Patchwork's own crates.
//! The idea is Bevy's `dynamic_linking`. Release builds never use it.

// Rust guideline compliant 2026-10-09

pub use anyhow;
pub use argon2;
pub use base64;
pub use chacha20poly1305;
pub use chrono;
pub use dirs;
pub use enigo;
pub use futures;
pub use globset;
pub use gpui;
pub use gpui_platform;
pub use ignore;
pub use image;
pub use log;
pub use minisign_verify;
pub use notify;
pub use portable_pty;
pub use pulldown_cmark;
pub use regex;
pub use resvg;
pub use serde;
pub use serde_json;
pub use sha2;
pub use thiserror;
pub use toml;
pub use tungstenite;
pub use unicode_segmentation;
pub use ureq;
pub use url;
pub use uuid;
pub use vt100;
pub use zeroize;
pub use zip;
pub use zune_jpeg;
#[cfg(target_os = "linux")]
pub use x11rb;
#[cfg(target_os = "linux")]
pub use zbus;
