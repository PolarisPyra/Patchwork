//! Source Control: the commit box, the changed files, and the
//! commit graph, each pane closing on its header and the divider between them dragged.

#![expect(
    clippy::unused_self,
    reason = "a view acts through its `Context<Self>`, so some of its methods never read the view itself"
)]

// Rust guideline compliant 2026-10-09

mod git_status;
mod git_commands;
mod file_groups;
mod commit_box;
mod commit_graph;
mod commit_details;
mod file_list;
mod ref_pickers;
mod panes;
mod header_menu;
mod host;

pub use git_status::ChangesView;
pub use host::{SourceControlEvent, SourceControlHost};
