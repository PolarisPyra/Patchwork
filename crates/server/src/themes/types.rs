//! What a color scheme's file and an animated background's file hold, and what is drawn.

// Rust guideline compliant 2026-10-09

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::code::Syntax;
use crate::settings::TextSize;

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

/// An animated background as its file holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneFile {
    pub name: String,
    /// Whether the page over it is drawn in light or dark text's colors.
    #[serde(default)]
    pub scheme: Scheme,
    #[serde(flatten)]
    pub scene: Scene,
}

/// A picture drawn in text characters: a sky, dust over it, and sprites that move.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Scene {
    /// The color behind everything: one, or two fading from the top to the bottom. Empty uses
    /// the theme's surface.
    pub sky: Vec<Color>,
    pub dust: Option<Dust>,
    /// The color of the shooting stars that cross now and then. None has none.
    pub shooting_stars: Option<Color>,
    pub sprites: Vec<Sprite>,
}

/// Specks scattered over the whole page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Dust {
    /// The characters a speck is one of.
    pub glyphs: String,
    pub color: Color,
    /// The share of cells that hold a speck, from 0 to 0.2.
    pub density: f32,
    /// Whether specks brighten and dim.
    pub twinkle: bool,
    /// How fast the specks move across and down, in pixels a second.
    pub drift: [f32; 2],
}

impl Default for Dust {
    fn default() -> Self {
        Self {
            glyphs: ".".into(),
            color: Color(0x0080_90b0),
            density: 0.01,
            twinkle: true,
            drift: [0., 0.],
        }
    }
}

/// A small picture: rows of characters, each drawn in the color its character is given.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Sprite {
    /// Its rows, top first. A space is empty.
    pub art: Vec<String>,
    /// The color of each character. A character without one is drawn in the theme's faint text.
    pub colors: BTreeMap<char, Color>,
    /// Where its middle is, as shares of the page's width and height.
    pub at: [f32; 2],
    pub size: CellSize,
    /// Each character is drawn on a square of its color, as a tile.
    pub filled: bool,
    /// It has a soft light under it.
    pub glow: bool,
    pub motion: Motion,
}

/// How large a sprite's characters are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CellSize {
    /// Small enough to shade a shape.
    Small,
    /// The size of code.
    #[default]
    Large,
}

/// How a sprite moves.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Motion {
    #[default]
    Still,
    /// Up and down by `height` pixels, once every `period` seconds.
    Bob { height: f32, period: f32 },
    /// Across and down at `speed` pixels a second, coming back in at the other side.
    Drift { speed: [f32; 2] },
    /// Around the page in a slow loop, at about `speed` pixels a second.
    Wander { speed: f32 },
}

/// The most sprites a scene has, and the most rows and characters a row a sprite has.
const MAX_SPRITES: usize = 24;
const MAX_ROWS: usize = 80;
const MAX_COLS: usize = 160;

impl SceneFile {
    /// Checks the scene is small enough to draw every frame.
    ///
    /// # Errors
    ///
    /// Returns an error naming the limit the scene is over.
    pub fn check(&self) -> Result<(), String> {
        let scene = &self.scene;
        if scene.sprites.len() > MAX_SPRITES {
            return Err(format!("a scene has at most {MAX_SPRITES} sprites"));
        }
        let fits = |sprite: &Sprite| {
            sprite.art.len() <= MAX_ROWS
                && sprite.art.iter().all(|row| row.chars().count() <= MAX_COLS)
        };
        if !scene.sprites.iter().all(fits) {
            return Err(format!(
                "a sprite has at most {MAX_ROWS} rows of {MAX_COLS} characters"
            ));
        }
        Ok(())
    }
}

/// A file that can be picked: one Patchwork comes with, or one in its folder.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry<T> {
    /// Its file's name without `.json`.
    pub id: String,
    /// It is a file a person or an agent added, which can be deleted. One Patchwork comes with
    /// is written to the folder anew when it is missing.
    pub custom: bool,
    /// What is wrong with it that it can still be used with, such as text that is hard to read.
    pub notes: Vec<String>,
    pub file: T,
}

/// A file in one of the folders that cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable {
    /// Its folder and name, as `themes/blossom.json`.
    pub file: String,
    pub error: String,
}

/// A color scheme that can be picked.
pub type Theme = Entry<ThemeFile>;

/// An animated background that can be picked.
pub type SavedScene = Entry<SceneFile>;

/// What the Orchestrator page shows behind it, as Settings keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BackdropChoice {
    None,
    #[default]
    #[serde(alias = "theme")]
    Butterfly,
    /// A picture in Patchwork's backgrounds folder.
    Image {
        file: String,
        #[serde(default)]
        dim: Dim,
        #[serde(default)]
        fit: Fit,
        #[serde(default)]
        blur: bool,
    },
    /// The animated background with this id.
    Scene {
        #[serde(alias = "theme")]
        id: String,
    },
}

impl BackdropChoice {
    /// The picture with this file name, shown as a picture is until it is set otherwise.
    #[must_use]
    pub fn picture(file: String) -> Self {
        Self::Image {
            file,
            dim: Dim::default(),
            fit: Fit::default(),
            blur: false,
        }
    }

    /// The file name of the picture it shows.
    #[must_use]
    pub fn picture_file(&self) -> Option<&str> {
        match self {
            Self::Image { file, .. } => Some(file),
            _ => None,
        }
    }
}

/// How far a picture is dimmed toward the window's color, so the page reads over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dim {
    None,
    Light,
    #[default]
    Medium,
    Strong,
}

impl Dim {
    /// The share of the window's color laid over the picture.
    #[must_use]
    pub fn share(self) -> f32 {
        match self {
            Self::None => 0.,
            Self::Light => 0.25,
            Self::Medium => 0.45,
            Self::Strong => 0.65,
        }
    }
}

/// How a picture fills the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    /// It covers the page, cut at the sides it is too long on.
    #[default]
    Cover,
    /// All of it shows, with the window's color beside it.
    Contain,
    /// It is stretched to the page's shape.
    Stretch,
}

/// The backdrop to draw.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Shown {
    None,
    #[default]
    Butterfly,
    /// The picture at `path`, with `dim` of the window's color over it.
    Image {
        path: PathBuf,
        dim: f32,
        fit: Fit,
    },
    Scene(Scene),
}

/// The colors a window is drawn in.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Palette {
    Light,
    #[default]
    Dark,
    /// Dark with true-black backgrounds.
    Oled,
    /// A color scheme's file.
    File(Box<ThemeFile>),
}

impl Palette {
    /// The palette Patchwork comes with under this id.
    pub(super) fn built_in(id: &str) -> Option<Self> {
        match id {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            "oled" => Some(Self::Oled),
            _ => None,
        }
    }
}

/// Which palette the app is drawn in.
#[derive(Debug, Clone, PartialEq)]
pub enum Paint {
    /// One palette, whatever the system's appearance.
    Fixed(Palette),
    /// One palette while the system is light and another while it is dark.
    System { light: Palette, dark: Palette },
}

impl Default for Paint {
    fn default() -> Self {
        Self::Fixed(Palette::default())
    }
}

/// What the appearance settings come to.
///
/// It holds the colors to draw the app in, the Orchestrator's backdrop, and how chats are set.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Applied {
    pub paint: Paint,
    /// The picked code theme's colors in a light window. None uses the palette's own.
    pub code_light: Option<Syntax>,
    /// The picked code theme's colors in a dark window. None uses the palette's own.
    pub code_dark: Option<Syntax>,
    pub backdrop: Shown,
    /// Whether the backdrop is made for light or dark text over it. None follows the app.
    pub backdrop_scheme: Option<Scheme>,
    /// The font family chats are set in. None is the app's own.
    pub chat_font: Option<String>,
    pub text_size: TextSize,
    /// Nothing moves on its own: animations show their resting state.
    pub reduce_motion: bool,
}
