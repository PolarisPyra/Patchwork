//! Source Control: the commit box, the changed files, and the
//! commit graph, each pane closing on its header and the divider between them dragged.

// Rust guideline compliant 2026-10-09

mod state;
mod actions;
mod rows;
mod commit;
mod render;

pub use state::ChangesView;
