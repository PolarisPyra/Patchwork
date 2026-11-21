//! Where an artifact's files are kept: a folder in its chat's project, beside the worktrees in
//! `.patchwork`, or in the app's data folder for a chat with no project on this machine.
//!
//! The folder holds each version's source, such as `3.html` or `3.csv`, and, when the page
//! drawn from it differs, that page as `3.page.html`. The database stays the record; these
//! files are written from it, so a missing one is written again.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};

use super::render::page;
use super::types::Artifact;
use crate::system::ssh;
use crate::threads::worktree_home::{PROJECT_DIR, keep_out_of_git};
use crate::{Core, Error};

/// The folder in the project's `.patchwork` that holds artifacts. A development build keeps its
/// own, as it does for worktrees.
const FOLDER: &str = if cfg!(debug_assertions) {
    "artifacts-dev"
} else {
    "artifacts"
};

impl Core {
    /// The folder the artifact `artifact`'s files are kept in. It may not exist yet.
    pub(super) fn artifact_folder(&self, artifact: &Artifact) -> PathBuf {
        let short = artifact.id.get(..8).unwrap_or(&artifact.id);
        // A key from before keys were checked may hold a path; it stays inside the folder. A
        // backslash or a colon is a path only on Windows, so elsewhere such a key keeps its folder.
        let key = artifact.key.replace('/', "-");
        let key = if cfg!(windows) {
            key.replace(['\\', ':'], "-")
        } else {
            key
        };
        let name = format!("{key}-{short}");
        match self.local_project(&artifact.thread_id) {
            Some(project) => Path::new(&project)
                .join(PROJECT_DIR)
                .join(FOLDER)
                .join(name),
            None => crate::system::dirs().data.join("artifacts").join(name),
        }
    }

    /// Writes version `n` of `artifact`, whose source is `content`, into its folder, and
    /// returns the file the browser draws it from.
    ///
    /// # Errors
    ///
    /// Returns an error when the folder or a file cannot be written.
    pub(super) fn write_artifact_files(
        &self,
        artifact: &Artifact,
        n: u32,
        content: &str,
    ) -> Result<PathBuf, Error> {
        let folder = self.artifact_folder(artifact);
        if !folder.is_dir() {
            std::fs::create_dir_all(&folder)
                .map_err(|e| format!("Cannot create {}: {e}", folder.display()))?;
            if let Some(project) = self.local_project(&artifact.thread_id) {
                keep_out_of_git(&project, &format!("{PROJECT_DIR}/{FOLDER}"));
            }
        }
        let ext = artifact.kind.extension(artifact.language.as_deref());
        let source = folder.join(format!("{n}.{ext}"));
        write_if_changed(&source, content)?;
        let drawn = page(artifact.kind, content, &artifact.title);
        if ext == "html" && drawn == content {
            return Ok(source);
        }
        let file = folder.join(format!("{n}.page.html"));
        write_if_changed(&file, &drawn)?;
        Ok(file)
    }

    /// The local folder of chat `thread_id`'s project, when it has one on this machine.
    fn local_project(&self, thread_id: &str) -> Option<String> {
        let project_id = self.store.thread(thread_id)?.project_id?;
        let path = self.project_root(&project_id).ok()?;
        (!ssh::is_remote(&path) && Path::new(&path).is_dir()).then_some(path)
    }
}

/// Writes `text` to `file` unless it already holds it, so an open page is not reloaded for
/// nothing and the file's time stays when it was made.
fn write_if_changed(file: &Path, text: &str) -> Result<(), Error> {
    if std::fs::read(file).is_ok_and(|old| old == text.as_bytes()) {
        return Ok(());
    }
    std::fs::write(file, text).map_err(|e| format!("Cannot write {}: {e}", file.display()))?;
    Ok(())
}
