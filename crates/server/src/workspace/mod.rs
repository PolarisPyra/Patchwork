//! Filesystem, git, and process helpers. All functions are blocking.
//!
//! Split by concern: `fs` for project folders, `git` for repository state, `media` for how a
//! reply's files are shown, and `process` for signals and the editor. Callers use these re-exports.

mod types;

pub use types::{FileChange, ProjectChanged};

mod fs;
mod git;
mod media;
mod process;
#[cfg(test)]
mod tests;

pub use fs::{create_project_dir, make_project, missing_files, read_file, resolve_inside};
pub use git::{accept_file, git_branch, git_changes, git_file_diff, list_files, revert_file};
pub use media::{FileMedia, PreviewKind, file_media};
pub use process::{SIGKILL, SIGTERM, open_in_editor, signal_process_group};
