//! What the Orchestrator page shows behind it: as Settings keeps it, and as it is drawn.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::scene::Scene;
use crate::theme_file::Scheme;

/// What the Orchestrator page shows behind it, as Settings keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BackdropChoice {
    None,
    #[default]
    Butterfly,
    /// A picture in Patchwork's backgrounds folder.
    Image {
        file: String,
    },
    /// One of the backgrounds the app comes with, as a chat's `ChatBackground::Preset`.
    Preset {
        id: String,
    },
    /// The animated background with this id.
    Scene {
        id: String,
    },
}

impl BackdropChoice {
    /// The file name of the picture it shows.
    #[must_use]
    pub fn picture_file(&self) -> Option<&str> {
        match self {
            Self::Image { file, .. } => Some(file),
            _ => None,
        }
    }
}

/// The backdrop to draw.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Shown {
    None,
    #[default]
    Butterfly,
    /// The picture at `path`, which covers the page.
    Image {
        path: PathBuf,
    },
    /// The background the app comes with that has this id.
    Preset(String),
    Scene(Scene),
}

/// A backdrop, and the kind of text that reads over it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Backdrop {
    pub shown: Shown,
    /// Whether it is made for light or dark text over it. None follows the app.
    pub scheme: Option<Scheme>,
}
