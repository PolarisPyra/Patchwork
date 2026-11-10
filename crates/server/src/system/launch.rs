//! Starting other programs on the user's desktop. On Wayland a window a program opens is only
//! raised if the request carries an activation token from the compositor, so they pass one on when
//! there is one.

// Rust guideline compliant 2026-10-09

use std::process::Command;

/// Passes `token` to a program the app starts. The desktop reads it from `XDG_ACTIVATION_TOKEN`,
/// and does not raise the window the program opens without it.
pub fn pass_token(cmd: &mut Command, token: Option<&str>) {
    if let Some(token) = token {
        cmd.env("XDG_ACTIVATION_TOKEN", token);
    }
}
