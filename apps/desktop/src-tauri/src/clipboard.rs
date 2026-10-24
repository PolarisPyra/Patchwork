//! Reads what a code editor put on the system clipboard alongside the copied text. VS Code and
//! other Electron editors store the copy's language under `vscode-editor-data`, and Chromium on
//! Linux packs such entries into one `chromium/x-web-custom-data` entry, which the webview does
//! not show to the page.

/// The `vscode-editor-data` JSON of the clipboard's current copy, or None when the copy came
/// from somewhere else or the clipboard cannot be read (an X11 session, or another platform).
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
