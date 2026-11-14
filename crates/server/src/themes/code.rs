//! The code themes: the colors code is highlighted in, picked apart from the color scheme.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::types::{Color, Entry, Scheme};

/// The colors of highlighted code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Syntax {
    pub keyword: Color,
    pub string: Color,
    pub property: Color,
    /// The names of types.
    pub ty: Color,
    pub number: Color,
    pub comment: Color,
    pub function: Color,
    pub tag: Color,
}

/// A code theme as its file holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeThemeFile {
    pub name: String,
    /// The kind of window its colors read on.
    #[serde(default)]
    pub scheme: Scheme,
    pub colors: Syntax,
}

/// A code theme that can be picked for light windows or for dark ones.
pub type CodeTheme = Entry<CodeThemeFile>;

/// The code theme of a base16 scheme: its YAML file's sixteen colors, of which code uses eight.
///
/// The window is dark when the scheme's background, `base00`, is. None when `text` is not a
/// base16 scheme.
pub(super) fn from_base16(text: &str) -> Option<CodeThemeFile> {
    let mut fields: HashMap<&str, &str> = HashMap::new();
    for line in text.lines() {
        let line = line.split(" #").next().unwrap_or(line);
        if let Some((key, value)) = line.split_once(':') {
            let value = value.trim().trim_matches(['"', '\'']);
            fields.insert(key.trim(), value);
        }
    }
    let color = |key: &str| {
        let digits = fields.get(key)?.trim_start_matches('#');
        let value = u32::from_str_radix(digits, 16).ok()?;
        (digits.len() == 6).then_some(Color(value))
    };
    let surface = color("base00")?.0;
    let [r, g, b] = [surface >> 16, surface >> 8, surface].map(|c| c & 0xff);
    let dark = r * 299 + g * 587 + b * 114 < 128_000;
    let name = ["name", "scheme"]
        .iter()
        .find_map(|key| fields.get(key).filter(|name| !name.is_empty()))
        .map_or_else(|| "Base16".to_string(), |name| (*name).to_string());
    Some(CodeThemeFile {
        name,
        scheme: if dark { Scheme::Dark } else { Scheme::Light },
        colors: Syntax {
            keyword: color("base0E")?,
            string: color("base0B")?,
            property: color("base0C")?,
            ty: color("base0A")?,
            number: color("base09")?,
            comment: color("base03")?,
            function: color("base0D")?,
            tag: color("base08")?,
        },
    })
}
