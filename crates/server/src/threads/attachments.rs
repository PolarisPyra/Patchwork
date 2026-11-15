//! Where a chat without a project works, what a message attaches, and text saved where the user
//! picked.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::fmt::Write as _;
use std::path::PathBuf;

use image::ImageFormat;
use sha2::{Digest, Sha256};

use super::Attachment;
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

    /// The files and folders at `paths` as attachments, without the ones that are not there.
    /// Reads the disk.
    #[must_use]
    pub fn attach_paths(&self, paths: &[String]) -> Vec<Attachment> {
        paths
            .iter()
            .filter_map(|path| Attachment::from_path(path).ok())
            .collect()
    }

    /// Saves a picture that was pasted, as its pixels, and attaches the file. A picture in a
    /// format agents do not take is saved as a PNG.
    ///
    /// # Errors
    ///
    /// Returns an error when the bytes are not a picture, or the file cannot be written.
    pub fn attach_image(&self, bytes: &[u8]) -> Result<Attachment, Error> {
        let not_picture = |_discarded| "The clipboard does not hold a picture Patchwork can read";
        let format = image::guess_format(bytes).map_err(not_picture)?;
        let mut converted = Vec::new();
        let (bytes, extension) = match format {
            ImageFormat::Png => (bytes, "png"),
            ImageFormat::Jpeg => (bytes, "jpg"),
            ImageFormat::WebP => (bytes, "webp"),
            ImageFormat::Gif => (bytes, "gif"),
            _ => {
                image::load_from_memory(bytes)
                    .and_then(|picture| {
                        picture
                            .write_to(&mut std::io::Cursor::new(&mut converted), ImageFormat::Png)
                    })
                    .map_err(not_picture)?;
                (converted.as_slice(), "png")
            }
        };
        self.save_pasted(bytes, extension)
    }

    /// Saves pasted text too long for the message field as a file, and attaches it.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be written.
    pub fn attach_text(&self, text: &str) -> Result<Attachment, Error> {
        self.save_pasted(text.as_bytes(), "txt")
    }

    /// Writes pasted `bytes` to the pasted folder and attaches the file.
    fn save_pasted(&self, bytes: &[u8], extension: &str) -> Result<Attachment, Error> {
        // Named by what it holds, so pasting the same thing twice keeps one file.
        let hash = Sha256::digest(bytes)[..8]
            .iter()
            .fold(String::new(), |mut hash, b| {
                let _ = write!(hash, "{b:02x}");
                hash
            });
        std::fs::create_dir_all(&self.pasted_dir).map_err(|e| e.to_string())?;
        let path = self.pasted_dir.join(format!("pasted-{hash}.{extension}"));
        if !path.is_file() {
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        }
        Attachment::from_path(&path.to_string_lossy())
    }
}

/// The most characters a paste puts in the message field; a longer one is attached as a file.
const LONG_PASTE: usize = 5_000;

/// Whether pasted `text` is long enough to attach as a file instead of filling the field.
#[must_use]
pub fn is_long_paste(text: &str) -> bool {
    text.chars().count() > LONG_PASTE
}
