//! The Quilts page: what agents made for the person to look at, drawn live by the installed
//! browser, with every version kept.
//!
//! It draws what `patchwork_client::local::core::quilts` reads and asks it to restore a version.
//!
//! | File | Holds |
//! |---|---|
//! | `host.rs` | What the page asks of the window: the view a quilt's page is drawn in |
//! | `catalog.rs` | The quilts as last read, the gallery's filter and pictures, and the one opened |
//! | `gallery.rs` | The gallery: the tiles that make new ones, the filters, cards and rows |
//! | `manage.rs` | Pinning, renaming and deleting one, and its menu |
//! | `patch.rs` | Patch, the mascot by the title, and what sets it acting |
//! | `layout.rs` | The page drawn: the opened one's preview, source and versions |
//! | `thumbs.rs` | The versions' pictures, shared with the chats' cards |

// Rust guideline compliant 2026-10-09

#![expect(
    clippy::unused_self,
    reason = "a view acts through its `Context<Self>`, so some of its methods never read the view itself"
)]

mod gallery;
mod manage;
mod patch;
mod layout;
mod catalog;
mod host;
mod thumbs;

pub use layout::kind_icon;
pub use catalog::{QuiltsEvent, QuiltsPage};
pub use host::{PickedElement, QuiltPreview, QuiltsHost};
pub use thumbs::Thumbs;
