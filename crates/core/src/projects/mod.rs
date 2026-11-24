//! Projects: the folders that group their chats, the files in them, and what changes there.
//!
//! `types` holds the project and its folders, `manage` adds, opens and removes them, `files`
//! reads and makes project folders, `media` says how a reply's files are shown, and `watch`
//! follows changes to the open project. All file helpers are blocking.

// Rust guideline compliant 2026-10-09

mod files;
mod manage;
mod media;
mod pdf;
mod types;
pub mod watch;

#[doc(inline)]
pub use files::{missing_files, read_file, resolve_inside};
pub(crate) use files::make_project;
#[doc(inline)]
pub use manage::folder_found;
#[doc(inline)]
pub use media::{FileMedia, PreviewKind, file_media};
pub use pdf::pdf_pages;
pub(crate) use types::ProjectWatch;
#[doc(inline)]
pub use types::{Folder, Project, ProjectChanged};
