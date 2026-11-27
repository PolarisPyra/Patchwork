//! What a color scheme or code theme file holds: colors, fonts, and whether its text reads.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::backdrop::BackdropChoice;
use crate::entry::Entry;

/// A color, written `#rrggbb` in a theme file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Color(pub u32);

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!("#{:06x}", self.0))
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        let digits = text.strip_prefix('#').unwrap_or(&text);
        u32::from_str_radix(digits, 16)
            .ok()
            .filter(|_| digits.len() == 6)
            .map(Color)
            .ok_or_else(|| serde::de::Error::custom(format!("{text} is not a #rrggbb color")))
    }
}

/// Whether a theme is light or dark, which picks the colors it does not name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    Light,
    #[default]
    Dark,
}

/// The colors a theme names. Every other color of the app is mixed from them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchors {
    /// The window's background.
    pub surface: Color,
    /// The main text.
    pub text: Color,
    /// The faint text.
    pub muted: Color,
    /// Buttons, links and what is selected.
    pub accent: Color,
}

/// A color scheme as its file holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeFile {
    pub name: String,
    #[serde(default)]
    pub scheme: Scheme,
    /// The Orchestrator background picked along with it: an animated background's id,
    /// `butterfly` or `none`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backdrop: Option<String>,
    pub colors: Anchors,
    /// The colors it names itself, in place of the ones mixed from `colors`.
    #[serde(default)]
    pub overrides: Overrides,
    #[serde(default, skip_serializing_if = "Fonts::is_empty")]
    pub fonts: Fonts,
    /// How round corners are: the radius of a button's, in pixels. None is the app's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
}

impl ThemeFile {
    /// What is hard to read in it: each text whose contrast on the surface is too low.
    #[must_use]
    pub fn notes(&self) -> Vec<String> {
        notes(self)
    }

    /// The Orchestrator background it names.
    #[must_use]
    pub fn backdrop_choice(&self) -> Option<BackdropChoice> {
        Some(match self.backdrop.as_deref()? {
            "none" => BackdropChoice::None,
            "butterfly" => BackdropChoice::Butterfly,
            id => BackdropChoice::Scene { id: id.to_string() },
        })
    }
}

/// The font families a color scheme sets. One it leaves out is the app's own.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Fonts {
    /// The font of the app's own text.
    pub ui: Option<String>,
    /// The font of code and terminals.
    pub code: Option<String>,
}

impl Fonts {
    fn is_empty(&self) -> bool {
        self.ui.is_none() && self.code.is_none()
    }
}

/// The colors a color scheme may name itself. One it leaves out is mixed from its four.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Overrides {
    /// The sidebar and the panels.
    pub bg_panel: Option<Color>,
    /// Where chats and code are read.
    pub bg_editor: Option<Color>,
    pub bg_input: Option<Color>,
    pub bg_hover: Option<Color>,
    pub bg_selected: Option<Color>,
    /// What is selected in a pane that is not in use.
    pub bg_selected_muted: Option<Color>,
    /// Popups and dialogs.
    pub bg_elevated: Option<Color>,
    pub border: Option<Color>,
    pub border_strong: Option<Color>,
    /// The faintest text: placeholders and counts.
    pub text_subtle: Option<Color>,
    pub text_on_accent: Option<Color>,
    pub accent_hover: Option<Color>,
    /// Links.
    pub accent_text: Option<Color>,
    pub success: Option<Color>,
    pub danger: Option<Color>,
    pub warning: Option<Color>,
    /// The user's message bubble.
    pub bubble: Option<Color>,
    /// The colors of highlighted code, unless a code theme is picked.
    pub syntax: Option<Syntax>,
}

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

/// The least contrast the main text needs on the surface, and the least the faint text needs.
const TEXT: f32 = 4.5;
const MUTED: f32 = 3.;

/// How bright a color looks, from 0 for black to 1 for white, as WCAG 2 measures it.
fn luminance(color: Color) -> f32 {
    let channel = |shift: u32| {
        let c = f32::from(u8::try_from((color.0 >> shift) & 0xff).unwrap_or(u8::MAX)) / 255.;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
}

/// The contrast of two colors, from 1 for the same color to 21 for black on white.
fn ratio(a: Color, b: Color) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// What is hard to read in a color scheme: each text whose contrast on the surface is too low.
pub(crate) fn notes(file: &ThemeFile) -> Vec<String> {
    let colors = file.colors;
    [
        ("Text", colors.text, TEXT),
        ("Faint text", colors.muted, MUTED),
    ]
    .into_iter()
    .filter_map(|(what, color, least)| {
        let contrast = ratio(color, colors.surface);
        (contrast < least).then(|| {
            format!("{what} is hard to read: its contrast is {contrast:.1}, under {least}")
        })
    })
    .collect()
}
