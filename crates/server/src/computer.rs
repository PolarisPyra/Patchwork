//! Computer use: a screenshot of the screen, and the pointer and keyboard underneath it.
//!
//! This is what an agent needs to drive another application, the way Claude's and Codex's computer
//! use does, and it is off unless Settings → Permissions turns it on.
//!
//! A screenshot comes back as PNG at the screen's own size, never scaled. Input actions are the
//! small set a model can ask for: move, click, type, press a key, scroll.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::path::PathBuf;

use base64::Engine as _;
use enigo::{Axis, Button, Coordinate, Direction, Enigo, Keyboard, Mouse, Settings as EnigoSettings};
use image::RgbaImage;
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

/// A screenshot, as a data URL the webview can show, and the screen's size.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Screenshot {
    /// `data:image/png;base64,…`
    pub data_url: String,
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

fn left() -> String {
    "left".into()
}

/// The primary monitor as a PNG data URL, at its own size.
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
    let encoded = base64::engine::general_purpose::STANDARD.encode(&png);
    Ok(Screenshot {
        data_url: format!("data:image/png;base64,{encoded}"),
        width,
        height,
        screen_width,
        screen_height,
    })
}

/// Writes a screenshot to `dir` as PNG and returns the path, for an agent that reads images
/// with its file tools rather than receiving them inline.
pub(crate) fn screenshot_to_file(dir: &std::path::Path) -> Result<PathBuf, Error> {
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
    let path = dir.join(format!("screen-{}.png", crate::system::now_ms()));
    std::fs::write(&path, png).map_err(|e| format!("Could not write {}: {e}", path.display()))?;
    Ok(path)
}

/// The keys a model names in an action, mapped to the keyboard's own.
fn key(name: &str) -> Result<enigo::Key, Error> {
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
fn modifier(name: &str) -> Result<enigo::Key, Error> {
    match name.trim().to_lowercase().as_str() {
        "ctrl" | "control" => Ok(enigo::Key::Control),
        "shift" => Ok(enigo::Key::Shift),
        "alt" | "option" => Ok(enigo::Key::Alt),
        "meta" | "cmd" | "command" | "super" | "win" => Ok(enigo::Key::Meta),
        _ => Err(format!("Unknown modifier: {name}").into()),
    }
}

fn pointer() -> Result<Enigo, Error> {
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
struct Capture {
    pub image: RgbaImage,
    pub screen_width: u32,
    pub screen_height: u32,
}

/// The primary monitor: the one at the origin, failing that the first.
#[cfg(not(target_os = "linux"))]
fn capture() -> Result<Capture, Error> {
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
fn capture() -> Result<Capture, Error> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        linux::portal()
    } else {
        linux::x11()
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::HashMap;
    use std::time::Duration;

    use image::RgbaImage;
    use x11rb::connection::Connection as _;
    use x11rb::protocol::randr::ConnectionExt as _;
    use x11rb::protocol::xproto::{ConnectionExt as _, ImageFormat, ImageOrder, Screen};
    use zbus::blocking::{Connection, Proxy};
    use zbus::zvariant::{OwnedValue, Value};

    use super::Capture;
    use crate::Error;

    const PORTAL: &str = "org.freedesktop.portal.Desktop";

    /// How long the portal may take to answer. Long enough for the desktop's one-time "allow
    /// screenshots" prompt, which waits on the user.
    const PORTAL_TIMEOUT: Duration = Duration::from_secs(60);

    /// The whole desktop, through `org.freedesktop.portal.Screenshot`. The portal answers with a
    /// signal on a request object, so that is subscribed to before the call, or a quick answer
    /// could arrive before anyone listens.
    pub fn portal() -> Result<Capture, Error> {
        let unreachable = |e: zbus::Error| format!("Could not reach the screenshot portal: {e}");
        let conn = Connection::session().map_err(unreachable)?;
        let sender = conn
            .unique_name()
            .ok_or("The session bus gave no name")?
            .trim_start_matches(':')
            .replace('.', "_");
        let token = format!("patchwork_{}", crate::system::new_id().replace('-', "_"));
        let request = Proxy::new(
            &conn,
            PORTAL,
            format!("/org/freedesktop/portal/desktop/request/{sender}/{token}"),
            "org.freedesktop.portal.Request",
        )
        .map_err(unreachable)?;
        let mut responses = request.receive_signal("Response").map_err(unreachable)?;
        let screenshot = Proxy::new(
            &conn,
            PORTAL,
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Screenshot",
        )
        .map_err(unreachable)?;
        let options = HashMap::from([
            ("handle_token", Value::from(token.as_str())),
            ("interactive", Value::from(false)),
        ]);
        screenshot
            .call_method("Screenshot", &("", options))
            .map_err(|e| format!("The screenshot portal refused: {e}"))?;

        // The wait has a timeout, so it runs on its own thread. One that times out ends when the
        // portal answers, or with the app.
        let (sent, received) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sent.send(responses.next());
        });
        let message = received
            .recv_timeout(PORTAL_TIMEOUT)
            .map_err(|_discarded| "The screenshot portal did not answer")?
            .ok_or("The screenshot portal closed without answering")?;
        let (code, mut results): (u32, HashMap<String, OwnedValue>) = message
            .body()
            .deserialize()
            .map_err(|e| format!("The screenshot portal's answer was unreadable: {e}"))?;
        match code {
            0 => {}
            1 => return Err("The screenshot was not allowed".into()),
            _ => return Err("The screenshot portal could not take one".into()),
        }
        let uri: String = results
            .remove("uri")
            .ok_or("The screenshot portal named no file")?
            .try_into()
            .map_err(|e| format!("The screenshot portal named no file: {e}"))?;
        let path = url::Url::parse(&uri)
            .ok()
            .and_then(|url| url.to_file_path().ok())
            .ok_or_else(|| format!("The screenshot portal named no local file: {uri}"))?;
        let png =
            std::fs::read(&path).map_err(|e| format!("Could not read the screenshot: {e}"))?;
        // The portal saves a new file for each request; it was only ever for this one.
        let _ = std::fs::remove_file(&path);
        let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
            .map_err(|e| format!("Could not read the screenshot: {e}"))?
            .into_rgba8();
        // The portal shows every monitor. XWayland knows where the primary one is, in the
        // coordinates the pointer moves in, so the picture is cut down to it, as elsewhere.
        let layout = x11rb::connect(None).ok().and_then(|(conn, screen_num)| {
            let screen = conn.setup().roots.get(screen_num)?;
            Some(Layout::of(&conn, screen))
        });
        Ok(match layout {
            Some(layout) => layout.crop(&image),
            None => Capture {
                screen_width: image.width(),
                screen_height: image.height(),
                image,
            },
        })
    }

    /// Where the primary monitor is on the X11 screen, and how big the screen is. RandR names the
    /// primary monitor; without RandR, the whole screen is one monitor.
    struct Layout {
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        screen_width: u16,
        screen_height: u16,
    }

    impl Layout {
        fn of(conn: &impl x11rb::connection::Connection, screen: &Screen) -> Self {
            let monitors = conn
                .randr_get_monitors(screen.root, true)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .map(|reply| reply.monitors)
                .unwrap_or_default();
            let (screen_width, screen_height) = (screen.width_in_pixels, screen.height_in_pixels);
            let (x, y, width, height) = monitors
                .iter()
                .find(|m| m.primary)
                .or_else(|| monitors.iter().find(|m| m.x == 0 && m.y == 0))
                .or_else(|| monitors.first())
                .map_or((0, 0, screen_width, screen_height), |m| {
                    (m.x, m.y, m.width, m.height)
                });
            Layout {
                x,
                y,
                width,
                height,
                screen_width,
                screen_height,
            }
        }

        /// The primary monitor's part of a picture of the whole screen, which may be at another
        /// scale than the X11 screen, as a portal's picture is under display scaling.
        fn crop(&self, whole: &RgbaImage) -> Capture {
            let scale_x = f64::from(whole.width()) / f64::from(self.screen_width.max(1));
            let scale_y = f64::from(whole.height()) / f64::from(self.screen_height.max(1));
            let at =
                |v: i16, scale: f64, limit: u32| ((f64::from(v.max(0)) * scale) as u32).min(limit);
            let x = at(self.x, scale_x, whole.width());
            let y = at(self.y, scale_y, whole.height());
            // `crop_imm` keeps the rectangle inside the picture.
            let width = ((f64::from(self.width) * scale_x) as u32).max(1);
            let height = ((f64::from(self.height) * scale_y) as u32).max(1);
            Capture {
                image: image::imageops::crop_imm(whole, x, y, width, height).to_image(),
                screen_width: self.width.into(),
                screen_height: self.height.into(),
            }
        }
    }

    /// The primary monitor, read from the root window.
    pub fn x11() -> Result<Capture, Error> {
        let failed = |e: &dyn std::fmt::Display| format!("Could not capture the screen: {e}");
        let (conn, screen_num) =
            x11rb::connect(None).map_err(|e| format!("Could not reach the display: {e}"))?;
        let setup = conn.setup();
        let screen = setup
            .roots
            .get(screen_num)
            .ok_or("The display has no screen")?;
        let Layout {
            x,
            y,
            width,
            height,
            ..
        } = Layout::of(&conn, screen);
        let reply = conn
            .get_image(ImageFormat::Z_PIXMAP, screen.root, x, y, width, height, !0)
            .map_err(|e| failed(&e))?
            .reply()
            .map_err(|e| failed(&e))?;
        // Only the 32-bit pixels of a 24- or 32-bit display, which is every display in use.
        let bits = setup
            .pixmap_formats
            .iter()
            .find(|f| f.depth == reply.depth)
            .map(|f| f.bits_per_pixel);
        let pixels = usize::from(width) * usize::from(height);
        if bits != Some(32) || reply.data.len() < pixels * 4 {
            return Err(
                format!("The display's {}-bit pixels are not supported", reply.depth).into(),
            );
        }
        let msb = setup.image_byte_order == ImageOrder::MSB_FIRST;
        let rgba = reply.data[..pixels * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[a, b, c, d]| if msb { [b, c, d, 255] } else { [c, b, a, 255] })
            .collect();
        let image = RgbaImage::from_raw(width.into(), height.into(), rgba)
            .ok_or("The screen's pixels did not fill the picture")?;
        Ok(Capture {
            image,
            screen_width: width.into(),
            screen_height: height.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_map_to_keys() {
        key("Enter").unwrap();
        key("esc").unwrap();
        key("a").unwrap();
        key("not-a-key").unwrap_err();
        modifier("ctrl").unwrap();
        modifier("Hyper").unwrap_err();
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
        // The PNG header, so the base64 really is a picture.
        let encoded = shot.data_url.split_once("base64,").expect("base64").1;
        let png = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("base64 decodes");
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    }
}
