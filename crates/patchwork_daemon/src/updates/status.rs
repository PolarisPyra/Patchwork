//! Where updating stands, as Settings and the update card show it.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

/// How Patchwork keeps itself up to date.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateSettings {
    /// Minutes between checks for a new release: 10 to 60, in steps of 10.
    pub check_every_minutes: u32,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            check_every_minutes: 10,
        }
    }
}

/// Where updating stands.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum UpdateState {
    /// Nothing was checked yet.
    #[default]
    Idle,
    Checking,
    UpToDate,
    /// A newer release this copy cannot install itself.
    Available {
        version: String,
    },
    Downloading {
        version: String,
        downloaded: u64,
        /// The download's size, when the server says it.
        total: Option<u64>,
    },
    /// Installed; it runs once Patchwork restarts.
    Ready {
        version: String,
    },
    Failed {
        message: String,
    },
}

/// What the Updates pane shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateStatus {
    /// Whether this copy can install updates itself.
    pub can_install: bool,
    pub state: UpdateState,
    /// The notes of the newer release the last check found, from its section of CHANGELOG.md.
    pub notes: Option<ReleaseNotes>,
}

/// A release's notes as the app shows them: its highlights, then its changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseNotes {
    /// Markdown.
    pub text: String,
    /// The release's page, when it has more changes than `text` lists.
    pub more: Option<String>,
}
