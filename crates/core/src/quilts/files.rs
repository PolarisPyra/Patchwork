//! Where a quilt's files are kept: a folder in its chat's project, beside the worktrees in
//! `.patchwork`, or in the app's data folder for a chat with no project on this machine.
//!
//! The folder holds each version's source, such as `3.html` or `3.csv`, and, when the page
//! drawn from it differs, that page as `3.page.html`. The database stays the record; these
//! files are written from it, so a missing one is written again.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};

use super::render::page;
use super::types::Quilt;
use crate::system::ssh;
use crate::worktree::{PROJECT_DIR, keep_out_of_git};
use crate::{Core, Error};

/// The folder in the project's `.patchwork` that holds quilts. A development build keeps its
/// own, as it does for worktrees.
const FOLDER: &str = if cfg!(debug_assertions) {
    "quilts-dev"
} else {
    "quilts"
};

/// What `FOLDER` was called before quilts were. A chat with no project kept its quilts in the
/// data folder's `artifacts`.
const OLD_FOLDER: &str = if cfg!(debug_assertions) {
    "artifacts-dev"
} else {
    "artifacts"
};

impl Core {
    /// The folder the quilt `quilt`'s files are kept in. It may not exist yet. The folder of every
    /// quilt there, from before quilts were called that, is first moved to its new name.
    pub(super) fn quilt_folder(&self, quilt: &Quilt) -> PathBuf {
        let short = quilt.id.get(..8).unwrap_or(&quilt.id);
        // A key from before keys were checked may hold a path; it stays inside the folder. A
        // backslash or a colon is a path only on Windows, so elsewhere such a key keeps its folder.
        let key = quilt.key.replace('/', "-");
        let key = if cfg!(windows) {
            key.replace(['\\', ':'], "-")
        } else {
            key
        };
        let name = format!("{key}-{short}");
        let all = if let Some(project) = self.local_project(&quilt.chat_id) {
            let home = Path::new(&project).join(PROJECT_DIR);
            move_old(&home.join(OLD_FOLDER), &home.join(FOLDER))
        } else {
            let data = &crate::system::dirs().data;
            move_old(&data.join("artifacts"), &data.join("quilts"))
        };
        all.join(name)
    }

    /// Writes version `n` of `quilt`, whose source is `content`, into its folder, and
    /// returns the file the browser draws it from.
    ///
    /// # Errors
    ///
    /// Returns an error when the folder or a file cannot be written.
    pub(super) fn write_quilt_files(
        &self,
        quilt: &Quilt,
        n: u32,
        content: &str,
    ) -> Result<PathBuf, Error> {
        let folder = self.quilt_folder(quilt);
        if !folder.is_dir() {
            std::fs::create_dir_all(&folder)
                .map_err(|e| format!("Cannot create {}: {e}", folder.display()))?;
            if let Some(project) = self.local_project(&quilt.chat_id) {
                keep_out_of_git(&project, &format!("{PROJECT_DIR}/{FOLDER}"));
            }
        }
        let ext = quilt.kind.extension(quilt.language.as_deref());
        let source = folder.join(format!("{n}.{ext}"));
        write_if_changed(&source, content)?;
        let drawn = page(quilt.kind, content, &quilt.title);
        if ext == "html" && drawn == content {
            return Ok(source);
        }
        let file = folder.join(format!("{n}.page.html"));
        write_if_changed(&file, &drawn)?;
        Ok(file)
    }

    /// The local folder of chat `chat_id`'s project, when it has one on this machine.
    fn local_project(&self, chat_id: &str) -> Option<String> {
        let project_id = self.store.chat(chat_id)?.project_id?;
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

/// Moves the folder `old`, named from before quilts were called that, to `new` when only the
/// old one is there, and returns `new`.
fn move_old(old: &Path, new: &Path) -> PathBuf {
    if !new.exists()
        && old.is_dir()
        && let Err(e) = std::fs::rename(old, new)
    {
        log::warn!(error:% = e; "could not move the quilts' folder to its new name: {{error}}");
    }
    new.to_path_buf()
}
