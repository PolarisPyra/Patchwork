//! What shows behind a chat's messages.

// Rust guideline compliant 2026-11-27

use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize};

/// What shows behind a chat's messages.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChatBackground {
    /// The plain window color.
    #[default]
    None,
    /// One of the backgrounds the app comes with, by its id in the web app's list.
    Preset { id: String },
    /// A picture in Patchwork's backgrounds folder, by its file name there.
    Image {
        #[serde(deserialize_with = "file_name")]
        file: String,
    },
    /// The butterfly the Orchestrator page comes with.
    Butterfly,
    /// The animated background with this id.
    Scene { id: String },
}

/// Keeps only the file name of a picture, so a background saved with its whole path, before
/// pictures were kept by name, still finds it.
fn file_name<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    Ok(Path::new(&value)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&value)
        .to_owned())
}
