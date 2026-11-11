//! Computer use: whether it is on, the actions an agent can ask for, and taking a screenshot and moving the pointer and keys for them.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::path::PathBuf;
use enigo::{Axis, Button, Coordinate, Direction, Enigo, Keyboard, Mouse, Settings as EnigoSettings};
use image::RgbaImage;
use serde::Deserialize;

/// Whether Settings → Permissions has turned computer use on. Kept here so the tool list and the
/// approval rules can ask without a settings lookup.
pub(super) static ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Follows the setting; called whenever the settings are loaded or saved.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

/// Whether computer use is on.
pub fn enabled() -> bool {
    ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Refuses while computer use is off, so a stray call cannot look at the screen or act on it.
///
/// # Errors
///
/// Returns an error when computer use is turned off in Settings.
pub fn ensure_enabled() -> Result<(), Error> {
    if enabled() {
        Ok(())
    } else {
        Err("Computer use is off in Settings → Permissions".into())
    }
}

/// A screenshot, as PNG bytes, and the screen's size.
#[derive(Debug, Clone)]
pub struct Screenshot {
    /// The picture, encoded as PNG.
    pub png: Vec<u8>,
    /// The size of the picture, in the screen's own pixels.
    pub width: u32,
    pub height: u32,
    /// The size of the screen in the coordinates the pointer moves in. The same as the picture's,
    /// except on a scaled Wayland desktop, where the portal's picture has the device pixels.
    pub screen_width: u32,
    pub screen_height: u32,
}

/// What to do with the pointer or keyboard.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Action {
    Move {
        x: i32,
        y: i32,
    },
    Click {
        x: i32,
        y: i32,
        /// `left`, `middle`, or `right`.
        #[serde(default = "left")]
        button: String,
        #[serde(default)]
        double: bool,
    },
    /// Type text, as the keyboard would.
    Type {
        text: String,
    },
    /// Press a key, with modifiers held.
    Key {
        key: String,
        #[serde(default)]
        modifiers: Vec<String>,
    },
    /// Scroll by notches; positive is down, or right when `horizontal`.
    Scroll {
        amount: i32,
        #[serde(default)]
        horizontal: bool,
    },
}

pub(super) fn left() -> String {
    "left".into()
}

/// The primary monitor as a PNG, at its own size.
///
/// # Errors
///
/// Returns an error when the screen cannot be listed or captured, or the picture cannot be encoded
/// as PNG.
pub fn screenshot() -> Result<Screenshot, Error> {
    let Capture {
        image,
        screen_width,
        screen_height,
    } = capture()?;
    let (width, height) = (image.width(), image.height());
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| format!("Could not encode the screenshot: {e}"))?;
    Ok(Screenshot {
        png,
        width,
        height,
        screen_width,
        screen_height,
    })
}

/// Writes a screenshot to `dir` as PNG and returns the path, for an agent that reads images
/// with its file tools rather than receiving them inline.
pub(crate) fn screenshot_to_file(dir: &std::path::Path) -> Result<PathBuf, Error> {
    let png = screenshot()?.png;
    std::fs::create_dir_all(dir).map_err(|e| format!("Could not make {}: {e}", dir.display()))?;
    let path = dir.join(format!("screen-{}.png", crate::system::now_ms()));
    std::fs::write(&path, png).map_err(|e| format!("Could not write {}: {e}", path.display()))?;
    Ok(path)
}

/// The keys a model names in an action, mapped to the keyboard's own.
pub(super) fn key(name: &str) -> Result<enigo::Key, Error> {
    let key = match name.trim().to_lowercase().as_str() {
        "enter" | "return" => enigo::Key::Return,
        "tab" => enigo::Key::Tab,
        "escape" | "esc" => enigo::Key::Escape,
        "space" => enigo::Key::Space,
        "backspace" => enigo::Key::Backspace,
        "delete" | "del" => enigo::Key::Delete,
        "up" | "arrowup" => enigo::Key::UpArrow,
        "down" | "arrowdown" => enigo::Key::DownArrow,
        "left" | "arrowleft" => enigo::Key::LeftArrow,
        "right" | "arrowright" => enigo::Key::RightArrow,
        "home" => enigo::Key::Home,
        "end" => enigo::Key::End,
        "pageup" | "page_up" => enigo::Key::PageUp,
        "pagedown" | "page_down" => enigo::Key::PageDown,
        other => {
            let mut chars = other.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => enigo::Key::Unicode(c),
                _ => return Err(format!("Unknown key: {name}").into()),
            }
        }
    };
    Ok(key)
}

/// A modifier a model names, mapped to the keyboard's own.
pub(super) fn modifier(name: &str) -> Result<enigo::Key, Error> {
    match name.trim().to_lowercase().as_str() {
        "ctrl" | "control" => Ok(enigo::Key::Control),
        "shift" => Ok(enigo::Key::Shift),
        "alt" | "option" => Ok(enigo::Key::Alt),
        "meta" | "cmd" | "command" | "super" | "win" => Ok(enigo::Key::Meta),
        _ => Err(format!("Unknown modifier: {name}").into()),
    }
}

pub(super) fn pointer() -> Result<Enigo, Error> {
    Enigo::new(&EnigoSettings::default())
        .map_err(|e| format!("Could not take control of the pointer: {e}").into())
}

/// Performs one action on the pointer or keyboard.
pub(crate) fn perform(action: &Action) -> Result<(), Error> {
    let mut enigo = pointer()?;
    match action {
        Action::Move { x, y } => enigo
            .move_mouse(*x, *y, Coordinate::Abs)
            .map_err(|e| format!("Could not move the pointer: {e}").into()),
        Action::Click {
            x,
            y,
            button,
            double,
        } => {
            let button = match button.trim().to_lowercase().as_str() {
                "left" => Button::Left,
                "middle" => Button::Middle,
                "right" => Button::Right,
                other => return Err(format!("Unknown mouse button: {other}").into()),
            };
            enigo
                .move_mouse(*x, *y, Coordinate::Abs)
                .map_err(|e| format!("Could not move the pointer: {e}"))?;
            enigo
                .button(button, Direction::Click)
                .map_err(|e| format!("Could not click: {e}"))?;
            if *double {
                enigo
                    .button(button, Direction::Click)
                    .map_err(|e| format!("Could not click: {e}"))?;
            }
            Ok(())
        }
        Action::Type { text } => enigo
            .text(text)
            .map_err(|e| format!("Could not type: {e}").into()),
        Action::Key {
            key: name,
            modifiers,
        } => {
            let held: Vec<enigo::Key> = modifiers
                .iter()
                .map(|name| modifier(name))
                .collect::<Result<_, _>>()?;
            // Resolved before any modifier is held, so an unknown key name presses nothing.
            let target = key(name)?;
            let mut pressed = Vec::new();
            let mut result = Ok(());
            for m in &held {
                result = enigo
                    .key(*m, Direction::Press)
                    .map_err(|e| format!("Could not hold a modifier: {e}").into());
                if result.is_err() {
                    break;
                }
                pressed.push(*m);
            }
            if result.is_ok() {
                result = enigo
                    .key(target, Direction::Click)
                    .map_err(|e| format!("Could not press {name}: {e}").into());
            }
            // Whatever went down comes back up, however the press went.
            for m in pressed.iter().rev() {
                let _ = enigo.key(*m, Direction::Release);
            }
            result
        }
        Action::Scroll { amount, horizontal } => {
            let axis = if *horizontal {
                Axis::Horizontal
            } else {
                Axis::Vertical
            };
            enigo
                .scroll(*amount, axis)
                .map_err(|e| format!("Could not scroll: {e}").into())
        }
    }
}

/// A one-line description of an action, for the chat's tool rows and the log.
#[must_use]
pub fn describe(action: &Action) -> String {
    match action {
        Action::Move { x, y } => format!("Move the pointer to {x}, {y}"),
        Action::Click {
            x,
            y,
            button,
            double,
        } => {
            let times = if *double { "Double-click" } else { "Click" };
            format!("{times} {button} at {x}, {y}")
        }
        Action::Type { text } => {
            let mut shown: String = text.chars().take(40).collect();
            if text.chars().count() > 40 {
                shown.push('…');
            }
            format!("Type \"{shown}\"")
        }
        Action::Key { key, modifiers } => {
            let mut parts = modifiers.clone();
            parts.push(key.clone());
            format!("Press {}", parts.join("+"))
        }
        Action::Scroll { amount, horizontal } => {
            let axis = if *horizontal { "across" } else { "down" };
            format!("Scroll {amount} {axis}")
        }
    }
}

// Captures the primary screen as RGBA pixels, for computer use's screenshots.
//
// On Windows and macOS this is `xcap`. On Linux it is done here with what the app already builds
// for other reasons: `zbus` (notifications) for the desktop portal on Wayland, and `x11rb` (the
// clipboard and `enigo`) for X11. `xcap` would also build PipeWire's bindings for screen
// recording, which Patchwork never uses, and those need newer PipeWire headers than the oldest
// Ubuntu the AppImage is built on.

/// A screen's pixels, and its size in the coordinates the pointer moves in.
pub(super) struct Capture {
    pub image: RgbaImage,
    pub screen_width: u32,
    pub screen_height: u32,
}

/// The primary monitor: the one at the origin, failing that the first.
#[cfg(not(target_os = "linux"))]
pub(super) fn capture() -> Result<Capture, Error> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("Could not list the screens: {e}"))?;
    let monitor = monitors
        .iter()
        .find(|monitor| monitor.x().is_ok_and(|x| x == 0) && monitor.y().is_ok_and(|y| y == 0))
        .or_else(|| monitors.first())
        .ok_or("No screen to capture")?;
    let screen_width = monitor.width().map_err(|e| format!("{e}"))?;
    let screen_height = monitor.height().map_err(|e| format!("{e}"))?;
    let image = monitor
        .capture_image()
        .map_err(|e| format!("Could not capture the screen: {e}"))?;
    Ok(Capture {
        image,
        screen_width,
        screen_height,
    })
}

/// The desktop portal on Wayland, where an app may not read the screen itself, and X11 otherwise.
#[cfg(target_os = "linux")]
pub(super) fn capture() -> Result<Capture, Error> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        super::linux::portal()
    } else {
        super::linux::x11()
    }
}
