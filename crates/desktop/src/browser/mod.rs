//! A web page drawn by the installed Chrome or Chromium, running headless, inside the window:
//! the artifacts on the Artifacts page, and pages the person browses.
//!
//! It draws what `patchwork_core::browser` streams and sends the pointer and keys back. With no
//! browser installed it asks for one.
//!
//! | File | Holds |
//! |---|---|
//! | `view.rs` | The view's state: the session, the newest picture, and the input sent |
//! | `render.rs` | The view drawn: the address bar, the page, the install prompt |

// Rust guideline compliant 2026-10-09

mod render;
mod view;

pub use view::{BrowserView, PageErrorsChanged};
