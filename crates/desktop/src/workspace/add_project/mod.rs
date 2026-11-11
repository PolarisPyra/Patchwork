//! Add a project: picks a project folder inside the app, laid out like search. The field holds the
//! folder being shown: type after it to filter its folders, or type or paste another path and press
//! Enter to go there. When `~/.ssh/config` names hosts, a list of machines comes first; only this
//! computer can be browsed today, so the hosts are listed but disabled.

// Rust guideline compliant 2026-10-09

mod state;
mod view;

pub use state::{Closed, AddProject};
