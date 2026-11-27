//! A theme, scene or code theme file that can be picked, and one that cannot be read.

// Rust guideline compliant 2026-10-09

use crate::scene::SceneFile;
use crate::theme_file::ThemeFile;

/// A file that can be picked: one Patchwork comes with, or one in its folder.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry<T> {
    /// Its file's name without `.json`.
    pub id: String,
    /// It is a file a person or an agent added, which can be deleted. One Patchwork comes with
    /// is written to the folder anew when it is missing.
    pub custom: bool,
    /// It is one Patchwork comes with whose file in the folder was changed, so it can be reset.
    pub edited: bool,
    /// What is wrong with it that it can still be used with, such as text that is hard to read.
    pub notes: Vec<String>,
    pub file: T,
}

/// A file in one of the folders that cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable {
    /// Its folder and name, as `themes/blossom.json`.
    pub file: String,
    pub error: String,
}

/// A color scheme that can be picked.
pub type Theme = Entry<ThemeFile>;

/// An animated background that can be picked.
pub type SavedScene = Entry<SceneFile>;
