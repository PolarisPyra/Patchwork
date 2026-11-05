//! Chat backgrounds: the one every chat shows, kept in Settings, and a chat's own.

use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{Res, Server};

/// What shows behind a chat's messages.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "backgrounds.ts")]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChatBackground {
    /// The plain window color.
    #[default]
    None,
    /// One of the backgrounds the app comes with, by its id in the web app's list.
    Preset { id: String },
    /// A picture the user picked, copied into Patchwork's own folder.
    Image { path: String },
}

impl Server {
    /// Gives a chat a background of its own, or None to show the one every chat shares.
    pub fn set_thread_background(&self, id: &str, background: Option<ChatBackground>) -> Res<()> {
        self.store
            .update_thread(id, |t| t.background = background.clone())
    }

    /// Copies a picture into Patchwork's folder, so the background stays when the original is
    /// moved or deleted. Returns the copy's path.
    pub fn import_background_image(&self, path: &str) -> Res<String> {
        let source = Path::new(path);
        let extension = source
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .filter(|e| ["png", "jpg", "jpeg", "webp", "gif"].contains(&e.as_str()))
            .ok_or_else(|| format!("{path} is not a PNG, JPEG, WebP or GIF picture"))?;
        std::fs::create_dir_all(&self.backgrounds_dir).map_err(|e| e.to_string())?;
        let copy = self
            .backgrounds_dir
            .join(format!("{}.{extension}", uuid::Uuid::new_v4()));
        std::fs::copy(source, &copy).map_err(|e| format!("couldn't copy {path}: {e}"))?;
        Ok(copy.to_string_lossy().into_owned())
    }
}
