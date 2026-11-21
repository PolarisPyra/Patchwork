//! Reading artifacts and their versions, saving a new version, bringing an old one back,
//! pinning, renaming and deleting one, and the page a version is drawn from.

// Rust guideline compliant 2026-10-09

use super::types::{Artifact, ArtifactVersion};
use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::system::now_ms;
use crate::{Core, Error};

impl Core {
    /// Every artifact, the most recently changed first.
    #[must_use]
    pub fn artifacts(&self) -> Vec<Artifact> {
        self.store.artifacts()
    }

    /// Every version of the artifact `id`, the oldest first.
    #[must_use]
    pub fn artifact_versions(&self, id: &str) -> Vec<ArtifactVersion> {
        self.store.artifact_versions(id)
    }

    /// Saves version `n` of the artifact `id` again as its newest, keeping the versions after it.
    /// Returns the new version's number.
    ///
    /// # Errors
    ///
    /// Returns an error when the artifact or the version is gone, or the database write fails.
    pub fn restore_artifact(&self, id: &str, n: u32) -> Result<u32, Error> {
        let artifact = self.store.artifact(id).ok_or("That artifact is gone")?;
        let version = self
            .artifact_versions(id)
            .into_iter()
            .find(|v| v.n == n)
            .ok_or("That version is gone")?;
        self.save_artifact(artifact, &version.content)
    }

    /// Pins the artifact `id`, which keeps it first in the gallery, or unpins it.
    ///
    /// # Errors
    ///
    /// Returns an error when the artifact is gone or the database write fails.
    pub fn pin_artifact(&self, id: &str, pinned: bool) -> Result<(), Error> {
        let artifact = self.store.artifact(id).ok_or("That artifact is gone")?;
        self.store.set_artifact_pinned(id, pinned)?;
        self.emit(AgentEventEnvelope {
            thread_id: artifact.thread_id,
            event: AgentEvent::ArtifactsChanged,
        });
        Ok(())
    }

    /// Gives the artifact `id` the title `title`. Its agent keeps calling it by its key.
    ///
    /// # Errors
    ///
    /// Returns an error when the title is empty, the artifact is gone, or the database write
    /// fails.
    pub fn rename_artifact(&self, id: &str, title: &str) -> Result<(), Error> {
        let title = title.trim();
        if title.is_empty() {
            return Err("An artifact needs a title".into());
        }
        let artifact = self.store.artifact(id).ok_or("That artifact is gone")?;
        self.store.rename_artifact(id, title)?;
        self.emit(AgentEventEnvelope {
            thread_id: artifact.thread_id,
            event: AgentEvent::ArtifactsChanged,
        });
        Ok(())
    }

    /// Deletes the artifact `id`, every version of it, its folder of files, and its pictures in
    /// the cache.
    ///
    /// # Errors
    ///
    /// Returns an error when the artifact is gone or the database write fails.
    pub fn delete_artifact(&self, id: &str) -> Result<(), Error> {
        let artifact = self.store.artifact(id).ok_or("That artifact is gone")?;
        self.store.delete_artifact(id)?;
        let _ = std::fs::remove_dir_all(self.artifact_folder(&artifact));
        let thumbs = crate::system::dirs().cache.join("artifacts").join("thumbs");
        for n in 1..=artifact.versions {
            let _ = std::fs::remove_file(thumbs.join(format!("{id}-{n}.png")));
        }
        self.emit(AgentEventEnvelope {
            thread_id: artifact.thread_id,
            event: AgentEvent::ArtifactsChanged,
        });
        Ok(())
    }

    /// Writes version `n` of the artifact `id` into its folder as a page the browser can open,
    /// and returns its address. The files are written from the database when they are missing or
    /// differ, so the folder can be cleared.
    ///
    /// # Errors
    ///
    /// Returns an error when the artifact or the version is gone, or the file cannot be written.
    pub fn artifact_page(&self, id: &str, n: u32) -> Result<String, Error> {
        let artifact = self.store.artifact(id).ok_or("That artifact is gone")?;
        let version = self
            .artifact_versions(id)
            .into_iter()
            .find(|v| v.n == n)
            .ok_or("That version is gone")?;
        let file = self.write_artifact_files(&artifact, n, &version.content)?;
        Ok(format!("file://{}", file.display()).replace(' ', "%20"))
    }

    /// Saves `content` as the next version of `artifact`, as it now stands, and tells the window.
    pub(super) fn save_artifact(
        &self,
        mut artifact: Artifact,
        content: &str,
    ) -> Result<u32, Error> {
        artifact.updated_at = now_ms();
        let n = self.store.save_artifact_version(&artifact, content)?;
        if let Err(e) = self.write_artifact_files(&artifact, n, content) {
            log::warn!(error:% = e; "could not write an artifact's files: {{error}}");
        }
        self.emit(AgentEventEnvelope {
            thread_id: artifact.thread_id,
            event: AgentEvent::ArtifactsChanged,
        });
        Ok(n)
    }
}
