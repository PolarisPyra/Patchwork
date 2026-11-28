//! Capturing the screen on Linux: through the desktop portal, or from X11.

// Rust guideline compliant 2026-10-09

use crate::Error;
use image::RgbaImage;

use std::collections::HashMap;
use std::time::Duration;

use x11rb::connection::Connection as _;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::xproto::{ConnectionExt as _, ImageFormat, ImageOrder, Screen};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedValue, Value};

use super::actions::Capture;

pub(super) const PORTAL: &str = "org.freedesktop.portal.Desktop";

/// How long the portal may take to answer. Long enough for the desktop's one-time "allow
/// screenshots" prompt, which waits on the user.
pub(super) const PORTAL_TIMEOUT: Duration = Duration::from_secs(60);

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
    let png = std::fs::read(&path).map_err(|e| format!("Could not read the screenshot: {e}"))?;
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
pub(super) struct Layout {
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
        let at = |v: i16, scale: f64, limit: u32| ((f64::from(v.max(0)) * scale) as u32).min(limit);
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
        return Err(format!("The display's {}-bit pixels are not supported", reply.depth).into());
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
