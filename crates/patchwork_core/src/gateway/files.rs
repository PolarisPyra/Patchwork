//! What the core knows of the files chat apps are sent: the largest one, what kind each is, and
//! the screenshot an agent sends.

// Rust guideline compliant 2026-10-09

use crate::{Core, Error};

/// The largest file Patchwork sends to a chat app, in bytes. Discord takes 10 MiB from a bot and
/// Telegram 10 MB as a photo; staying under both lets every connector take it whole.
pub const FILE_LIMIT: usize = 8 * 1024 * 1024;

/// What a chat app is told when it asks for a screenshot while they are off.
pub const SCREENSHOTS_OFF: &str = "Screenshots are off. Turn on \u{201c}Let the agent see the screen\u{201d} in Settings → Permissions on the computer";

/// The media type of a file Patchwork sends, read from its first bytes: one of the pictures chat
/// apps show inline, text when it is UTF-8 without a NUL, and plain bytes otherwise.
///
/// The bytes decide rather than the name, so a picture saved under another extension still shows
/// as one, and a file named `.png` that is not one is not offered to a chat app as a picture.
#[must_use]
pub fn mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok() {
        "text/plain; charset=utf-8"
    } else {
        "application/octet-stream"
    }
}

/// Whether `bytes` are a picture chat apps show inline.
#[must_use]
pub fn is_picture(bytes: &[u8]) -> bool {
    mime(bytes).starts_with("image/")
}

impl Core {
    /// A screenshot of the screen for a chat app, as PNG, while Settings → Permissions lets the
    /// agent see the screen. Computer use is a setting of its own and need not be on.
    ///
    /// # Errors
    ///
    /// Returns an error when seeing the screen is off, or the screen cannot be captured.
    pub fn agent_screenshot(&self) -> Result<crate::computer::Screenshot, Error> {
        if !self.store.settings().see_screen {
            return Err(SCREENSHOTS_OFF.into());
        }
        crate::computer::look()
    }
}
