//! Where a chat without a project works, and text saved where the user picked.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::path::PathBuf;

use crate::Server;

impl Server {
    /// Scratch working directory for a chat that has no project.
    pub fn chat_dir(&self, thread_id: &str) -> PathBuf {
        self.chats_dir.join(thread_id)
    }

    /// Writes `text` to `path`, a file the user picked in a save dialog.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be written.
    pub fn save_text_file(&self, path: &str, text: &str) -> Result<(), Error> {
        std::fs::write(path, text).map_err(|e| (e.to_string()).into())
    }
}
