//! Keys as the app names them turned into the `DevTools` protocol's key events: the DOM key and
//! code, the Windows key code pages read from `keyCode`, the text typed, and the editing command
//! a shortcut stands for, since a headless page has no menu to run it.

// Rust guideline compliant 2026-10-09

use serde_json::{Value, json};

use super::types::Modifiers;

/// Named keys: the app's name, the DOM key, the DOM code, the Windows key code, and the text it
/// types.
const NAMED: &[(&str, &str, &str, u32, &str)] = &[
    ("enter", "Enter", "Enter", 0x0D, "\r"),
    ("backspace", "Backspace", "Backspace", 0x08, ""),
    ("tab", "Tab", "Tab", 0x09, "\t"),
    ("escape", "Escape", "Escape", 0x1B, ""),
    ("space", " ", "Space", 0x20, " "),
    ("left", "ArrowLeft", "ArrowLeft", 0x25, ""),
    ("up", "ArrowUp", "ArrowUp", 0x26, ""),
    ("right", "ArrowRight", "ArrowRight", 0x27, ""),
    ("down", "ArrowDown", "ArrowDown", 0x28, ""),
    ("home", "Home", "Home", 0x24, ""),
    ("end", "End", "End", 0x23, ""),
    ("pageup", "PageUp", "PageUp", 0x21, ""),
    ("pagedown", "PageDown", "PageDown", 0x22, ""),
    ("insert", "Insert", "Insert", 0x2D, ""),
    ("delete", "Delete", "Delete", 0x2E, ""),
];

/// The `DevTools` protocol's bits for the modifier keys.
#[must_use]
pub(super) fn modifier_bits(modifiers: Modifiers) -> u32 {
    u32::from(modifiers.alt)
        | u32::from(modifiers.control) << 1
        | u32::from(modifiers.meta) << 2
        | u32::from(modifiers.shift) << 3
}

/// The `Input.dispatchKeyEvent` parameters for one key press: down, then up.
#[must_use]
pub(super) fn key_events(key: &str, text: Option<&str>, modifiers: Modifiers) -> Vec<Value> {
    let (dom_key, code, key_code, named_text) = describe(key);
    let shortcut = modifiers.control || modifiers.meta;
    let text = if shortcut {
        ""
    } else {
        text.filter(|t| !t.is_empty()).unwrap_or(named_text)
    };
    let bits = modifier_bits(modifiers);
    let mut down = json!({
        "type": if text.is_empty() { "rawKeyDown" } else { "keyDown" },
        "key": if text.is_empty() || named_text == text { dom_key.clone() } else { text.to_owned() },
        "code": code,
        "windowsVirtualKeyCode": key_code,
        "modifiers": bits,
    });
    if !text.is_empty() {
        down["text"] = json!(text);
        down["unmodifiedText"] = json!(text);
    }
    if let Some(command) = shortcut.then(|| command(key, modifiers.shift)).flatten() {
        down["commands"] = json!([command]);
    }
    let up = json!({
        "type": "keyUp",
        "key": dom_key,
        "code": code,
        "windowsVirtualKeyCode": key_code,
        "modifiers": bits,
    });
    vec![down, up]
}

/// A key's DOM key, DOM code, Windows key code and the text it types alone.
fn describe(key: &str) -> (String, String, u32, &'static str) {
    if let Some(&(_, dom, code, key_code, text)) = NAMED.iter().find(|n| n.0 == key) {
        return (dom.into(), code.into(), key_code, text);
    }
    if let Some(n) = key.strip_prefix('f').and_then(|n| n.parse::<u32>().ok())
        && (1..=24).contains(&n)
    {
        return (format!("F{n}"), format!("F{n}"), 0x6F + n, "");
    }
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphabetic() => {
            let upper = c.to_ascii_uppercase();
            (key.into(), format!("Key{upper}"), u32::from(upper), "")
        }
        (Some(c), None) if c.is_ascii_digit() => {
            (key.into(), format!("Digit{c}"), u32::from(c), "")
        }
        _ => (key.into(), String::new(), 0, ""),
    }
}

/// The editing command a Control or Command shortcut runs.
fn command(key: &str, shift: bool) -> Option<&'static str> {
    Some(match (key, shift) {
        ("a", false) => "selectAll",
        ("c", false) => "copy",
        ("x", false) => "cut",
        ("v", false) => "paste",
        ("z", false) => "undo",
        ("z", true) | ("y", false) => "redo",
        _ => return None,
    })
}
