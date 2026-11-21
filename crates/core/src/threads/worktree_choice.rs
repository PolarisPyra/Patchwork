//! Whether a task gets a worktree of its own: what was picked for it before its first turn, else
//! its project's setting, and whether the user was told where a project's tasks work.

// Rust guideline compliant 2026-10-09

use crate::{Core, Error};

/// The `app_state` key holding whether task `thread_id` gets a worktree, "1" or "0", when that
/// was picked for it.
fn choice_key(thread_id: &str) -> String {
    format!("worktree-choice:{thread_id}")
}

/// The `app_state` key set once the user was told where project `project_id`'s tasks work.
fn told_key(project_id: &str) -> String {
    format!("worktree-told:{project_id}")
}

impl Core {
    /// Whether task `thread_id`, in project `project_id`, gets a worktree of its own on its first
    /// turn: what was picked for it, else the project's setting.
    #[must_use]
    pub fn task_gets_worktree(&self, thread_id: &str, project_id: &str) -> bool {
        match self.store.state(&choice_key(thread_id)).as_deref() {
            Some("1") => true,
            Some(_) => false,
            None => !self.edits_in_place(project_id),
        }
    }

    /// What was picked for task `thread_id`: whether it gets a worktree of its own, or None when
    /// nothing was.
    #[must_use]
    pub fn task_worktree_pick(&self, thread_id: &str) -> Option<bool> {
        self.store
            .state(&choice_key(thread_id))
            .map(|choice| choice == "1")
    }

    /// Picks whether task `thread_id` gets a worktree of its own on its first turn, whatever its
    /// project's setting. A task that already started keeps where it works.
    ///
    /// # Errors
    ///
    /// Returns an error when the pick cannot be saved.
    pub fn set_task_gets_worktree(&self, thread_id: &str, on: bool) -> Result<(), Error> {
        self.store
            .set_state(&choice_key(thread_id), if on { "1" } else { "0" })
    }

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
