//! The Artifacts page: what agents made for the person to look at, drawn live by the installed
//! browser, with every version kept.
//!
//! It draws what `patchwork_core::artifacts` reads and asks it to restore a version.
//!
//! | File | Holds |
//! |---|---|
//! | `state.rs` | The artifacts as last read, the gallery's filter and pictures, and the one opened |
//! | `gallery.rs` | The gallery: the tiles that make new ones, the filters, cards and rows |
//! | `manage.rs` | Pinning, renaming and deleting one, and its menu |
//! | `render.rs` | The page drawn: the opened one's preview, source and versions |
//! | `thumbs.rs` | The versions' pictures, shared with the chats' cards |

// Rust guideline compliant 2026-10-09

mod gallery;
mod manage;
mod render;
mod state;
mod thumbs;

pub(crate) use render::kind_icon;
pub use state::{ArtifactsEvent, ArtifactsPage};
pub use thumbs::Thumbs;
