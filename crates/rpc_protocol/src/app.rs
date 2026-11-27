//! What the app tells clients about itself: version, settings, agents and projects.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::chats::AccessMode;

/// The answer to `server.info`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub version: String,
    /// The operating system as Node names it: `linux`, `darwin` or `win32`.
    pub platform: String,
    pub home_dir: Option<PathBuf>,
    pub data_dir: PathBuf,
}

/// A project, as `projects.list` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    /// Its place in the sidebar, from 0.
    pub position: usize,
    /// Always 0: the app does not keep when a project was added.
    pub created_at: u64,
    /// Whether its folder is there, or it is on another machine.
    pub exists: bool,
}

/// The settings `settings.get` gives a client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// The agent new chats start on, by its id.
    pub default_agent: String,
    pub default_access_mode: AccessMode,
    pub show_thoughts: bool,
    pub terminal_shell: Option<String>,
    pub terminal_font_size: u32,
}

/// An agent, as `agents.list` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    /// Its id, such as `claude` or `codex`.
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub version: Option<String>,
    pub signed_in: Option<bool>,
    pub path: Option<String>,
}
