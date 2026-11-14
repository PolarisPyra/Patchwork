//! Where updating stands, as Settings and the update card show it.

// Rust guideline compliant 2026-10-09

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

/// What `Server::restart_to_update` found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartResult {
    /// Agents are replying in this many tasks; restarting would stop them.
    Busy { running: usize },
    /// Nothing is installed and waiting.
    NothingReady,
    /// The app may restart now.
    Restart,
}
