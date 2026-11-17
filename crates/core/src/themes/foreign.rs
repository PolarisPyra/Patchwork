//! Color schemes read from the theme files other editors use: a base16 scheme, the JSON color
//! theme many editors share, and a Zed theme family.

// Rust guideline compliant 2026-10-09

use serde_json::Value;

use super::code::{CodeThemeFile, Syntax};
use super::types::{Anchors, Color, Fonts, Overrides, Scheme, ThemeFile};

/// The color scheme of a base16 scheme, whose code theme is `code` and whose file is `text`.
pub(super) fn from_base16(text: &str, code: &CodeThemeFile) -> Option<ThemeFile> {
    let color = |key: &str| {
        let line = text
            .lines()
            .find(|line| line.trim_start().starts_with(key))?;
        let value = line.split_once(':')?.1.split(" #").next()?;
        hex(value.trim().trim_matches(['"', '\'']), None)
    };
    Some(ThemeFile {
        name: code.name.clone(),
        scheme: code.scheme,
        backdrop: None,
        colors: Anchors {
            surface: color("base00")?,
            text: color("base05")?,
            muted: color("base04")?,
            accent: color("base0D")?,
        },
        overrides: Overrides {
            bg_panel: color("base01"),
            bg_selected: color("base02"),
            danger: color("base08"),
            warning: color("base0A"),
            success: color("base0B"),
            syntax: Some(code.colors),
            ..Overrides::default()
        },
        fonts: Fonts::default(),
        radius: None,
    })
}

/// JSON that has comments and trailing commas in it, as editors' theme files do.
pub(super) fn relaxed(text: &str) -> Option<Value> {
    let mut plain = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        if quoted {
            plain.push(c);
            match c {
                '\\' => plain.extend(chars.next()),
                '"' => quoted = false,
                _ => {}
            }
            continue;
        }
        match (c, chars.peek()) {
            ('"', _) => {
                quoted = true;
                plain.push(c);
            }
            ('/', Some('/')) => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
                plain.push('\n');
            }
            ('/', Some('*')) => {
                chars.next();
                let mut last = ' ';
                for c in chars.by_ref() {
                    if last == '*' && c == '/' {
                        break;
                    }
                    last = c;
                }
            }
            (']' | '}', _) => {
                // A comma before the closing bracket is the trailing one.
                let kept = plain.trim_end().len();
                if plain[..kept].ends_with(',') {
                    plain.truncate(kept - 1);
                }
                plain.push(c);
            }
            _ => plain.push(c),
        }
    }
    serde_json::from_str(&plain).ok()
}

/// A color written `#rgb`, `#rrggbb` or `#rrggbbaa`. One that is see-through is laid over
/// `under`, or loses its transparency when there is nothing under it.
fn hex(text: &str, under: Option<Color>) -> Option<Color> {
    let digits = text.trim().trim_start_matches('#');
    if !digits.is_ascii() {
        return None;
    }
    let wide = match digits.len() {
        3 | 4 => digits.chars().flat_map(|c| [c, c]).collect(),
        6 | 8 => digits.to_string(),
        _ => return None,
    };
    let color = u32::from_str_radix(&wide[..6], 16).ok()?;
    let alpha = match wide.get(6..8) {
        Some(alpha) => u32::from_str_radix(alpha, 16).ok()?,
        None => 255,
    };
    let Some(Color(under)) = under.filter(|_| alpha < 255) else {
        return Some(Color(color));
    };
    let mixed = [16, 8, 0].map(|shift| {
        let (over, under) = ((color >> shift) & 0xff, (under >> shift) & 0xff);
        ((over * alpha + under * (255 - alpha)) / 255) << shift
    });
    Some(Color(mixed[0] | mixed[1] | mixed[2]))
}

/// Whether a color is a dark one.
fn is_dark(Color(color): Color) -> bool {
    let [r, g, b] = [color >> 16, color >> 8, color].map(|c| c & 0xff);
    r * 299 + g * 587 + b * 114 < 128_000
}

/// The color halfway between two.
fn halfway(Color(a): Color, Color(b): Color) -> Color {
    let mixed =
        [16, 8, 0].map(|shift| u32::midpoint((a >> shift) & 0xff, (b >> shift) & 0xff) << shift);
    Color(mixed[0] | mixed[1] | mixed[2])
}

/// The first of `keys` that `colors` gives a color.
fn first(colors: &Value, keys: &[&str], under: Option<Color>) -> Option<Color> {
    keys.iter()
        .find_map(|key| hex(colors.get(key)?.as_str()?, under))
}

/// The color scheme of an editor's JSON color theme: a `colors` table keyed by the part of the
/// window, and `tokenColors` for code. None when `json` is not one.
pub(super) fn from_color_theme(json: &Value, fallback_name: &str) -> Option<ThemeFile> {
    let colors = json.get("colors")?;
    let surface = first(colors, &["editor.background"], None)?;
    let over = Some(surface);
    let pick = |keys: &[&str]| first(colors, keys, over);
    let text = pick(&["editor.foreground", "foreground"])?;
    let muted = pick(&["descriptionForeground", "editorLineNumber.foreground"])
        .unwrap_or_else(|| halfway(text, surface));
    let accent = pick(&["button.background", "focusBorder", "textLink.foreground"]).unwrap_or(text);
    let scheme = match json.get("type").and_then(Value::as_str) {
        Some("light" | "hc-light") => Scheme::Light,
        Some(_) => Scheme::Dark,
        None if is_dark(surface) => Scheme::Dark,
        None => Scheme::Light,
    };
    let name = json.get("name").and_then(Value::as_str);
    Some(ThemeFile {
        name: name.unwrap_or(fallback_name).to_string(),
        scheme,
        backdrop: None,
        colors: Anchors {
            surface,
            text,
            muted,
            accent,
        },
        overrides: Overrides {
            bg_panel: pick(&["sideBar.background"]),
            bg_input: pick(&["input.background"]),
            bg_hover: pick(&["list.hoverBackground"]),
            bg_selected: pick(&["list.activeSelectionBackground"]),
            bg_selected_muted: pick(&["list.inactiveSelectionBackground"]),
            bg_elevated: pick(&["editorWidget.background", "menu.background"]),
            border: pick(&["panel.border", "editorGroup.border", "sideBar.border"]),
            text_on_accent: pick(&["button.foreground"]),
            accent_hover: pick(&["button.hoverBackground"]),
            accent_text: pick(&["textLink.foreground"]),
            success: pick(&["terminal.ansiGreen"]),
            danger: pick(&["errorForeground", "editorError.foreground"]),
            warning: pick(&["editorWarning.foreground"]),
            syntax: json
                .get("tokenColors")
                .and_then(Value::as_array)
                .map(|tokens| token_syntax(tokens, text, muted, over)),
            ..Overrides::default()
        },
        fonts: Fonts::default(),
        radius: None,
    })
}

/// The colors of code that a `tokenColors` list gives, with `text` where it names none.
fn token_syntax(tokens: &[Value], text: Color, muted: Color, under: Option<Color>) -> Syntax {
    // The color of the rule that names `wanted` or the closest scope it is part of.
    let scoped = |wanted: &str| {
        let mut best: Option<(usize, Color)> = None;
        for token in tokens {
            let Some(color) = token
                .pointer("/settings/foreground")
                .and_then(Value::as_str)
                .and_then(|color| hex(color, under))
            else {
                continue;
            };
            let scopes: Vec<&str> = match token.get("scope") {
                Some(Value::String(scopes)) => scopes.split(',').collect(),
                Some(Value::Array(scopes)) => scopes.iter().filter_map(Value::as_str).collect(),
                _ => continue,
            };
            for scope in scopes.into_iter().map(str::trim) {
                let part = wanted == scope
                    || wanted
                        .strip_prefix(scope)
                        .is_some_and(|rest| rest.starts_with('.'));
                if part && best.is_none_or(|(len, _)| scope.len() > len) {
                    best = Some((scope.len(), color));
                }
            }
        }
        best.map(|(_, color)| color)
    };
    let find = |wanted: &[&str], or: Color| wanted.iter().find_map(|w| scoped(w)).unwrap_or(or);
    Syntax {
        keyword: find(&["keyword.control", "storage.type"], text),
        string: find(&["string.quoted"], text),
        property: find(
            &["variable.other.property", "support.type.property-name"],
            text,
        ),
        ty: find(&["entity.name.type", "support.type", "support.class"], text),
        number: find(&["constant.numeric", "constant.language"], text),
        comment: find(&["comment.line"], muted),
        function: find(&["entity.name.function", "support.function"], text),
        tag: find(&["entity.name.tag"], text),
    }
}

/// The color schemes of a Zed theme family: each of its `themes`, with its `style`. Empty when
/// `json` is not one.
pub(super) fn from_zed_family(json: &Value) -> Vec<ThemeFile> {
    let themes = json.get("themes").and_then(Value::as_array);
    themes
        .into_iter()
        .flatten()
        .filter_map(|theme| {
            let style = theme.get("style")?;
            let surface = first(style, &["editor.background", "background"], None)?;
            let over = Some(surface);
            let pick = |keys: &[&str]| first(style, keys, over);
            let text = pick(&["text", "editor.foreground"])?;
            let muted = pick(&["text.muted"]).unwrap_or_else(|| halfway(text, surface));
            let code = |key: &str, or: Color| {
                let color = style.pointer(&format!("/syntax/{key}/color"));
                color
                    .and_then(Value::as_str)
                    .and_then(|color| hex(color, over))
                    .unwrap_or(or)
            };
            let scheme = match theme.get("appearance").and_then(Value::as_str) {
                Some("light") => Scheme::Light,
                Some(_) => Scheme::Dark,
                None if is_dark(surface) => Scheme::Dark,
                None => Scheme::Light,
            };
            Some(ThemeFile {
                name: theme.get("name")?.as_str()?.to_string(),
                scheme,
                backdrop: None,
                colors: Anchors {
                    surface,
                    text,
                    muted,
                    accent: pick(&["text.accent", "border.focused"]).unwrap_or(text),
                },
                overrides: Overrides {
                    bg_panel: pick(&["panel.background", "surface.background"]),
                    bg_hover: pick(&["element.hover", "ghost_element.hover"]),
                    bg_selected: pick(&["element.selected", "ghost_element.selected"]),
                    bg_elevated: pick(&["elevated_surface.background"]),
                    border: pick(&["border"]),
                    border_strong: pick(&["border.selected"]),
                    text_subtle: pick(&["text.placeholder"]),
                    success: pick(&["success"]),
                    danger: pick(&["error"]),
                    warning: pick(&["warning"]),
                    syntax: style.get("syntax").map(|_| Syntax {
                        keyword: code("keyword", text),
                        string: code("string", text),
                        property: code("property", text),
                        ty: code("type", text),
                        number: code("number", text),
                        comment: code("comment", muted),
                        function: code("function", text),
                        tag: code("tag", text),
                    }),
                    ..Overrides::default()
                },
                fonts: Fonts::default(),
                radius: None,
            })
        })
        .collect()
}
