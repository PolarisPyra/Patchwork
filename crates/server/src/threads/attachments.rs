//! Files a draft attaches, saved where the agent can read them, and text saved where the user picked.

use std::path::PathBuf;

use crate::system::new_id;
use crate::threads::Attachment;
use crate::Server;

impl Server {
    /// Scratch working directory for a chat that has no project.
    pub fn chat_dir(&self, thread_id: &str) -> PathBuf {
        self.chats_dir.join(thread_id)
    }

    /// Saves an image pasted from the clipboard (base64 `data`) as a file, and returns it as an
    /// attachment like a dropped image.
    pub fn save_pasted_image(
        &self,
        mime_type: &str,
        data: &str,
        name: Option<&str>,
    ) -> Result<Attachment, String> {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| format!("Pasted image is not valid base64: {e}"))?;
        self.save_pasted_image_bytes(mime_type, &bytes, name)
    }

    /// Saves a pasted image's raw bytes as a file, and returns it as an attachment named `name`,
    /// the name the copy came with, or "Pasted image" when it has none.
    pub fn save_pasted_image_bytes(
        &self,
        mime_type: &str,
        bytes: &[u8],
        name: Option<&str>,
    ) -> Result<Attachment, String> {
        let ext = match mime_type {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            _ => return Err(format!("Can't attach a {mime_type} image")),
        };
        std::fs::create_dir_all(&self.pasted_dir).map_err(|e| e.to_string())?;
        let path = self.pasted_dir.join(format!("{}.{ext}", new_id()));
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        let mut attachment = Attachment::from_path(&path.to_string_lossy())?;
        attachment.name = name
            .and_then(|name| std::path::Path::new(name.trim()).file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("Pasted image.{ext}"));
        Ok(attachment)
    }

    /// Saves text pasted from the clipboard as a file, and returns it as an attachment the agent
    /// opens itself, so a long paste does not fill the composer.
    pub fn save_pasted_text(&self, text: &str) -> Result<Attachment, String> {
        std::fs::create_dir_all(&self.pasted_dir).map_err(|e| e.to_string())?;
        let path = self.pasted_dir.join(format!("{}.txt", new_id()));
        std::fs::write(&path, text).map_err(|e| e.to_string())?;
        let mut attachment = Attachment::from_path(&path.to_string_lossy())?;
        attachment.name = "Pasted text.txt".to_string();
        Ok(attachment)
    }

    /// Writes `text` to `path`, a file the user picked in a save dialog.
    pub fn save_text_file(&self, path: &str, text: &str) -> Result<(), String> {
        std::fs::write(path, text).map_err(|e| e.to_string())
    }
}
