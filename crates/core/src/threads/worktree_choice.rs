//! Whether the user was told where a project's tasks work.

// Rust guideline compliant 2026-10-09

use crate::{Core, Error};

/// The `app_state` key set once the user was told where project `project_id`'s tasks work.
fn told_key(project_id: &str) -> String {
    format!("worktree-told:{project_id}")
}

impl Core {
    /// Whether the user was told that project `project_id`'s tasks work in worktrees.
    #[must_use]
    pub fn worktree_told(&self, project_id: &str) -> bool {
        self.store.state(&told_key(project_id)).is_some()
    }

    /// Remembers that the user was told where project `project_id`'s tasks work.
    ///
    /// # Errors
    ///
    /// Returns an error when it cannot be saved.
    pub fn set_worktree_told(&self, project_id: &str) -> Result<(), Error> {
        self.store.set_state(&told_key(project_id), "1")
    }
}
