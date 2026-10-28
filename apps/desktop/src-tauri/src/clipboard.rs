//! Reads what a code editor put on the system clipboard alongside the copied text. VS Code and
//! other Electron editors store the copy's language under `vscode-editor-data`, and Chromium on
//! Linux packs such entries into one `chromium/x-web-custom-data` entry, which the webview does
//! not show to the page.

/// The `vscode-editor-data` JSON of the clipboard's current copy, or None when the copy came
/// from somewhere else or the clipboard cannot be read (an X11 session, or another platform).
use patchwork_server::Res;

#[cfg(target_os = "linux")]
pub fn editor_data() -> Option<String> {
    use std::io::Read;
    use wl_clipboard_rs::paste::{ClipboardType, MimeType, Seat, get_contents};

    let (mut pipe, _) = get_contents(
        ClipboardType::Regular,
        Seat::Unspecified,
        MimeType::Specific("chromium/x-web-custom-data"),
    )
    .ok()?;
    let mut bytes = Vec::new();
    pipe.read_to_end(&mut bytes).ok()?;
    custom_data_entry(&bytes, "vscode-editor-data")
}

#[cfg(not(target_os = "linux"))]
pub fn editor_data() -> Option<String> {
    None
}

/// The clipboard's image as PNG bytes, or None when it holds none. The webview on Linux gives a
/// paste no file for an image copied on its own, such as a screenshot.
pub fn image() -> Option<Vec<u8>> {
    let image = arboard::Clipboard::new().ok()?.get_image().ok()?;
    let rgba = image::RgbaImage::from_raw(
        u32::try_from(image.width).ok()?,
        u32::try_from(image.height).ok()?,
        image.bytes.into_owned(),
    )?;
    let mut png = std::io::Cursor::new(Vec::new());
    rgba.write_to(&mut png, image::ImageFormat::Png).ok()?;
    Some(png.into_inner())
}

/// The file name the app that copied the image suggests for it, such as a KDE screenshot's.
#[cfg(target_os = "linux")]
pub fn image_name() -> Option<String> {
    use std::io::Read;
    use wl_clipboard_rs::paste::{ClipboardType, MimeType, Seat, get_contents};

    let (mut pipe, _) = get_contents(
        ClipboardType::Regular,
        Seat::Unspecified,
        MimeType::Specific("application/x-kde-suggestedfilename"),
    )
    .ok()?;
    let mut name = String::new();
    pipe.read_to_string(&mut name).ok()?;
    Some(name).filter(|name| !name.trim().is_empty())
}

#[cfg(not(target_os = "linux"))]
pub fn image_name() -> Option<String> {
    None
}

/// The paths of files and folders copied in a file manager, or none when the copy is something
/// else. Webviews give a paste the files' contents at most, never their paths.
pub fn files() -> Vec<String> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.get().file_list())
        .unwrap_or_default()
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

/// Puts `text` on the system clipboard, for a copy the webview refused: its clipboard API wants a
/// focused page and a fresh click, which a click that only brings the window forward is not. The
/// copy is served from a background thread until something else is copied.
#[cfg(target_os = "linux")]
pub fn write_text(text: String) -> Res<()> {
    use wl_clipboard_rs::copy::{MimeType, Options, Source};

    Options::new()
        .copy(Source::Bytes(text.into_bytes().into()), MimeType::Text)
        .map_err(|e| format!("Could not copy: {e}"))
}

#[cfg(not(target_os = "linux"))]
pub fn write_text(_text: String) -> Res<()> {
    Err("Could not copy".into())
}

/// One entry of Chromium's custom clipboard data: a pickle holding its payload size, the entry
/// count, then each type and value as a length-prefixed UTF-16 string padded to 4 bytes.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn custom_data_entry(bytes: &[u8], wanted: &str) -> Option<String> {
    let mut at = 4;
    let u32_at = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
    };
    let string = |at: &mut usize| -> Option<String> {
        let len = u32_at(*at)? as usize;
        let start = *at + 4;
        let units: Vec<u16> = bytes
            .get(start..start + len * 2)?
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        *at = (start + len * 2 + 3) & !3;
        String::from_utf16(&units).ok()
    };
    let count = u32_at(at)?;
    at += 4;
    for _ in 0..count {
        let kind = string(&mut at)?;
        let value = string(&mut at)?;
        if kind == wanted {
            return Some(value);
        }
    }
    None
}
