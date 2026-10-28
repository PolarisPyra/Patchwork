//! Computer use: a screenshot of the screen, and the pointer and keyboard underneath it. This is
//! what an agent needs to drive another application, the way Claude's and Codex's computer use
//! does, and it is off unless Settings → Permissions turns it on.
//!
//! A screenshot comes back as PNG, scaled down so a model does not spend its context on pixels
//! it cannot read. Input actions are the small set a model can ask for: move, click, type, press
//! a key, scroll.

use ts_rs::TS;
use std::path::PathBuf;

use base64::Engine as _;
use enigo::{Axis, Button, Coordinate, Direction, Enigo, Keyboard, Mouse, Settings as EnigoSettings};
use image::imageops::FilterType;
use serde::{Deserialize, Serialize};

/// Whether Settings → Permissions has turned computer use on. Kept here so the tool list and the
/// approval rules can ask without a settings lookup.
static ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Follows the setting; called whenever the settings are loaded or saved.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

/// Whether computer use is on.
pub fn enabled() -> bool {
    ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Widest a screenshot is sent. A 4K screen scaled to this is still legible for a model, and
/// costs a fraction of the tokens.
pub const MAX_WIDTH: u32 = 1440;

/// A screenshot, as a data URL the webview can show, and the screen's size.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Screenshot {
    /// `data:image/png;base64,…`
    pub data_url: String,
    /// The size of the picture, after scaling.
    pub width: u32,
    pub height: u32,
    /// The size of the screen it came from.
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

fn left() -> String {
    "left".into()
}

/// The primary monitor as a PNG data URL, scaled to `MAX_WIDTH`.
pub fn screenshot() -> Result<Screenshot, String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("Could not list the screens: {e}"))?;
    // The primary monitor is the one at the origin; failing that, the first one.
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
    let (width, height) = (image.width(), image.height());
    let scaled = if width > MAX_WIDTH {
        let height = (height as u64 * MAX_WIDTH as u64 / width as u64) as u32;
        // Triangle is a third of Lanczos3's time on a 4K screen and is legible at this scale;
        // a screenshot is taken mid-turn, so its cost is felt.
        image::DynamicImage::ImageRgba8(image).resize(MAX_WIDTH, height, FilterType::Triangle)
    } else {
        image::DynamicImage::ImageRgba8(image)
    };
    let mut png = Vec::new();
    scaled
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| format!("Could not encode the screenshot: {e}"))?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&png);
    Ok(Screenshot {
        data_url: format!("data:image/png;base64,{encoded}"),
        width: scaled.width(),
        height: scaled.height(),
        screen_width,
        screen_height,
    })
}

/// Writes a screenshot to `dir` as PNG and returns the path, for an agent that reads images
/// with its file tools rather than receiving them inline.
pub fn screenshot_to_file(dir: &std::path::Path) -> Result<PathBuf, String> {
    let shot = screenshot()?;
    let encoded = shot
        .data_url
        .split_once("base64,")
        .map(|(_, rest)| rest)
        .ok_or("The screenshot had no data")?;
    let png = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| format!("Could not decode the screenshot: {e}"))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("Could not make {}: {e}", dir.display()))?;
    let path = dir.join(format!("screen-{}.png", crate::types::now_ms()));
    std::fs::write(&path, png).map_err(|e| format!("Could not write {}: {e}", path.display()))?;
    Ok(path)
}

/// The keys a model names in an action, mapped to the keyboard's own.
fn key(name: &str) -> Result<enigo::Key, String> {
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
                _ => return Err(format!("Unknown key: {name}")),
            }
        }
    };
    Ok(key)
}

/// A modifier a model names, mapped to the keyboard's own.
fn modifier(name: &str) -> Result<enigo::Key, String> {
    match name.trim().to_lowercase().as_str() {
        "ctrl" | "control" => Ok(enigo::Key::Control),
        "shift" => Ok(enigo::Key::Shift),
        "alt" | "option" => Ok(enigo::Key::Alt),
        "meta" | "cmd" | "command" | "super" | "win" => Ok(enigo::Key::Meta),
        _ => Err(format!("Unknown modifier: {name}")),
    }
}

fn pointer() -> Result<Enigo, String> {
    Enigo::new(&EnigoSettings::default())
        .map_err(|e| format!("Could not take control of the pointer: {e}"))
}

/// Performs one action on the pointer or keyboard.
pub fn perform(action: &Action) -> Result<(), String> {
    let mut enigo = pointer()?;
    match action {
        Action::Move { x, y } => enigo
            .move_mouse(*x, *y, Coordinate::Abs)
            .map_err(|e| format!("Could not move the pointer: {e}")),
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
                other => return Err(format!("Unknown mouse button: {other}")),
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
        Action::Type { text } => enigo.text(text).map_err(|e| format!("Could not type: {e}")),
        Action::Key {
            key: name,
            modifiers,
        } => {
            let held: Vec<enigo::Key> = modifiers
                .iter()
                .map(|name| modifier(name))
                .collect::<Result<_, _>>()?;
            for m in &held {
                enigo
                    .key(*m, Direction::Press)
                    .map_err(|e| format!("Could not hold a modifier: {e}"))?;
            }
            let result = enigo
                .key(key(name)?, Direction::Click)
                .map_err(|e| format!("Could not press {name}: {e}"));
            for m in held.iter().rev() {
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
                .map_err(|e| format!("Could not scroll: {e}"))
        }
    }
}

/// A one-line description of an action, for the chat's tool rows and the log.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_map_to_keys() {
        assert!(key("Enter").is_ok());
        assert!(key("esc").is_ok());
        assert!(key("a").is_ok());
        assert!(key("not-a-key").is_err());
        assert!(modifier("ctrl").is_ok());
        assert!(modifier("Hyper").is_err());
    }

    #[test]
    fn actions_describe_themselves() {
        assert_eq!(
            describe(&Action::Click {
                x: 10,
                y: 20,
                button: "left".into(),
                double: true
            }),
            "Double-click left at 10, 20"
        );
        assert_eq!(
            describe(&Action::Key {
                key: "s".into(),
                modifiers: vec!["ctrl".into()]
            }),
            "Press ctrl+s"
        );
        let long = "x".repeat(60);
        assert!(describe(&Action::Type { text: long }).ends_with("…\""));
    }

    #[test]
    fn an_action_reads_from_json_the_way_the_model_sends_it() {
        let click: Action =
            serde_json::from_str(r#"{"kind":"click","x":1,"y":2}"#).expect("a click");
        assert_eq!(
            click,
            Action::Click {
                x: 1,
                y: 2,
                button: "left".into(),
                double: false
            }
        );
        let key: Action =
            serde_json::from_str(r#"{"kind":"key","key":"t","modifiers":["ctrl","shift"]}"#)
                .expect("a key");
        assert_eq!(
            key,
            Action::Key {
                key: "t".into(),
                modifiers: vec!["ctrl".into(), "shift".into()]
            }
        );
    }

    /// Ignored by default: it needs a display, and it moves nothing on a machine without one.
    /// Run it with `cargo test -p patchwork-server -- --ignored screenshot_captures_the_screen`.
    #[test]
    #[ignore = "needs a display"]
    fn screenshot_captures_the_screen() {
        let started = std::time::Instant::now();
        let shot = screenshot().expect("a screenshot");
        eprintln!(
            "capture took {:?} ({}x{} from {}x{})",
            started.elapsed(),
            shot.width,
            shot.height,
            shot.screen_width,
            shot.screen_height
        );
        assert!(shot.data_url.starts_with("data:image/png;base64,"));
        assert!(shot.width > 0 && shot.height > 0);
        assert!(shot.screen_width >= shot.width, "scaled down, never up");
        // The PNG header, so the base64 really is a picture.
        let encoded = shot.data_url.split_once("base64,").expect("base64").1;
        let png = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("base64 decodes");
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    }
}
