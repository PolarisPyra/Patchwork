//! What the appearance settings come to: the palette, code colors, backdrop, text size and spacing.

// Rust guideline compliant 2026-10-09

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::backdrop::Backdrop;
use crate::theme_file::Syntax;
use crate::theme_file::{Color, Scheme, ThemeFile};

/// How large the app's text is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TextSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl TextSize {
    /// What the sizes of the app's text are multiplied by.
    #[must_use]
    pub fn scale(self) -> f32 {
        match self {
            Self::Small => 0.92,
            Self::Medium => 1.,
            Self::Large => 1.15,
        }
    }
}

/// How much room rows and their padding take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Density {
    Compact,
    #[default]
    Comfortable,
    Roomy,
}

impl Density {
    /// What a row's height and padding are multiplied by.
    #[must_use]
    pub fn scale(self) -> f32 {
        match self {
            Self::Compact => 0.85,
            Self::Comfortable => 1.,
            Self::Roomy => 1.2,
        }
    }
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
    #[must_use]
    pub fn built_in(id: &str) -> Option<Self> {
        match id {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            "oled" => Some(Self::Oled),
            _ => None,
        }
    }
}

impl Palette {
    /// Whether its windows are light or dark.
    #[must_use]
    pub fn scheme(&self) -> Scheme {
        match self {
            Self::Light => Scheme::Light,
            Self::Dark | Self::Oled => Scheme::Dark,
            Self::File(file) => file.scheme,
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
    pub backdrop: Backdrop,
    /// The backdrops of a light window and of a dark one, when `paint` follows the system and
    /// each has its own. `backdrop` is then the one of the window that is showing.
    pub system_backdrops: Option<Box<[Backdrop; 2]>>,
    /// The color buttons, links and what is selected are drawn in, in place of the palette's.
    pub accent: Option<Color>,
    /// The font family of the app's text, code aside, in place of the palette's.
    pub ui_font: Option<String>,
    pub text_size: TextSize,
    pub density: Density,
    /// How long until the clock or the sun changes what this comes to. None when neither does.
    pub switch_in: Option<Duration>,
    /// Nothing moves on its own: animations show their resting state.
    pub reduce_motion: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_palettes_and_their_schemes() {
        assert_eq!(Palette::built_in("oled"), Some(Palette::Oled));
        assert_eq!(Palette::built_in("blossom"), None);
        assert_eq!(Palette::Light.scheme(), Scheme::Light);
        assert_eq!(Palette::Oled.scheme(), Scheme::Dark);
    }

    #[test]
    fn text_size_and_density_keep_their_saved_names() {
        assert_eq!(
            serde_json::to_string(&TextSize::Large).unwrap(),
            "\"large\""
        );
        assert_eq!(serde_json::to_string(&Density::Roomy).unwrap(), "\"roomy\"");
        assert!((TextSize::Small.scale() - 0.92).abs() < f32::EPSILON);
    }
}
