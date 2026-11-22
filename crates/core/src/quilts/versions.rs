//! Reading quilts and their versions, saving a new version, bringing an old one back,
//! pinning, renaming and deleting one, and the page a version is drawn from.

// Rust guideline compliant 2026-10-09

use super::thumbs::thumbnail_name;
use super::types::{Quilt, QuiltVersion};
use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::system::now_ms;
use crate::{Core, Error};

impl Core {
    /// Every quilt, the most recently changed first.
    #[must_use]
    pub fn quilts(&self) -> Vec<Quilt> {
        self.store.quilts()
    }

    /// Every version of the quilt `id`, the oldest first.
    #[must_use]
    pub fn quilt_versions(&self, id: &str) -> Vec<QuiltVersion> {
        self.store.quilt_versions(id)
    }

    /// Saves version `n` of the quilt `id` again as its newest, keeping the versions after it.
    /// Returns the new version's number.
    ///
    /// # Errors
    ///
    /// Returns an error when the quilt or the version is gone, or the database write fails.
    pub fn restore_quilt(&self, id: &str, n: u32) -> Result<u32, Error> {
        let quilt = self.store.quilt(id).ok_or("That quilt is gone")?;
        let version = self
            .quilt_versions(id)
            .into_iter()
            .find(|v| v.n == n)
            .ok_or("That version is gone")?;
        self.save_quilt(quilt, &version.content)
    }

    /// Pins the quilt `id`, which keeps it first in the gallery, or unpins it.
    ///
    /// # Errors
    ///
    /// Returns an error when the quilt is gone or the database write fails.
    pub fn pin_quilt(&self, id: &str, pinned: bool) -> Result<(), Error> {
        let quilt = self.store.quilt(id).ok_or("That quilt is gone")?;
        self.store.set_quilt_pinned(id, pinned)?;
        self.emit(AgentEventEnvelope {
            thread_id: quilt.thread_id,
            event: AgentEvent::QuiltsChanged,
        });
        Ok(())
    }

    /// Gives the quilt `id` the title `title`. Its agent keeps calling it by its key.
    ///
    /// # Errors
    ///
    /// Returns an error when the title is empty, the quilt is gone, or the database write
    /// fails.
    pub fn rename_quilt(&self, id: &str, title: &str) -> Result<(), Error> {
        let title = title.trim();
        if title.is_empty() {
            return Err("A quilt needs a title".into());
        }
        let quilt = self.store.quilt(id).ok_or("That quilt is gone")?;
        self.store.rename_quilt(id, title)?;
        self.emit(AgentEventEnvelope {
            thread_id: quilt.thread_id,
            event: AgentEvent::QuiltsChanged,
        });
        Ok(())
    }

    /// Deletes the quilt `id`, every version of it, its folder of files, and its pictures in
    /// the cache.
    ///
    /// # Errors
    ///
    /// Returns an error when the quilt is gone or the database write fails.
    pub fn delete_quilt(&self, id: &str) -> Result<(), Error> {
        let quilt = self.store.quilt(id).ok_or("That quilt is gone")?;
        self.store.delete_quilt(id)?;
        let _ = std::fs::remove_dir_all(self.quilt_folder(&quilt));
        let thumbs = crate::system::dirs().cache.join("quilts").join("thumbs");
        for n in 1..=quilt.versions {
            // Pictures taken before they followed the theme have neither name.
            let _ = std::fs::remove_file(thumbs.join(format!("{id}-{n}.png")));
            for dark in [true, false] {
                let _ = std::fs::remove_file(thumbs.join(thumbnail_name(id, n, dark)));
            }
        }
        self.emit(AgentEventEnvelope {
            thread_id: quilt.thread_id,
            event: AgentEvent::QuiltsChanged,
        });
        Ok(())
    }

    /// Writes version `n` of the quilt `id` into its folder as a page the browser can open,
    /// and returns its address. The files are written from the database when they are missing or
    /// differ, so the folder can be cleared.
    ///
    /// # Errors
    ///
    /// Returns an error when the quilt or the version is gone, or the file cannot be written.
    pub fn quilt_page(&self, id: &str, n: u32) -> Result<String, Error> {
        let quilt = self.store.quilt(id).ok_or("That quilt is gone")?;
        let version = self
            .quilt_versions(id)
            .into_iter()
            .find(|v| v.n == n)
            .ok_or("That version is gone")?;
        let file = self.write_quilt_files(&quilt, n, &version.content)?;
        Ok(format!("file://{}", file.display()).replace(' ', "%20"))
    }

    /// Saves `content` as the next version of `quilt`, as it now stands, and tells the window.
    pub(super) fn save_quilt(&self, mut quilt: Quilt, content: &str) -> Result<u32, Error> {
        quilt.updated_at = now_ms();
        let n = self.store.save_quilt_version(&quilt, content)?;
        if let Err(e) = self.write_quilt_files(&quilt, n, content) {
            log::warn!(error:% = e; "could not write a quilt's files: {{error}}");
        }
        self.emit(AgentEventEnvelope {
            thread_id: quilt.thread_id,
            event: AgentEvent::QuiltsChanged,
        });
        Ok(n)
    }
}
