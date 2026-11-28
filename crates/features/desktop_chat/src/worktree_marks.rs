//! What each task's worktree needs from the user, which the sidebar marks on the task's row.

// Rust guideline compliant 2026-11-27

use crate::prelude::*;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use gpui::Context;
use patchwork_client::AppState;
use patchwork_platform::now_ms;

/// The window's worktree marks methods, for any [`ChatWindow`].
pub trait WorktreeMarksExt: ChatWindow {
    /// Reads what each task's worktree needs from the user off the main thread, when a task
    /// changed since they were last read.
    fn load_worktree_marks(&mut self, cx: &mut Context<Self>) {
        let mut hasher = DefaultHasher::new();
        for th in AppState::get(cx).chats.iter() {
            (&th.id, th.messages.len(), th.is_busy()).hash(&mut hasher);
        }
        // A running turn changes files without adding messages, so its marks are read again every
        // few seconds while it updates.
        if AppState::get(cx).chats.iter().any(|th| th.is_busy()) {
            (now_ms() / 3000).hash(&mut hasher);
        }
        let key = hasher.finish();
        if self.worktree_marks_key == Some(key) {
            return;
        }
        self.worktree_marks_key = Some(key);
        let core = Arc::clone(&self.core);
        patchwork_design_tasks::off_thread(
            cx,
            move || core.worktree_marks(),
            move |this, marks, cx| {
                if this.worktree_marks != marks {
                    this.worktree_marks = marks;
                    cx.notify();
                }
            },
        );
    }

    /// Reads the worktree marks again, whatever was read before.
    fn reload_worktree_marks(&mut self, cx: &mut Context<Self>) {
        self.worktree_marks_key = None;
        self.load_worktree_marks(cx);
    }
}

impl<W: ChatWindow> WorktreeMarksExt for W {}
