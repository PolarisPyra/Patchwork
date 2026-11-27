//! What an animated background's file holds: a sky, dust and sprites drawn in text characters.

// Rust guideline compliant 2026-10-09

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::theme_file::{Color, Scheme};

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
