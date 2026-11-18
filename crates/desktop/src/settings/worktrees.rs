//! Settings → Worktrees: whether a fork gets a worktree of its own, what cleaning up keeps, and
//! the buttons that clean task worktrees up now.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{AnyElement, ClickEvent, Div, Role, SharedString, Stateful, div, prelude::*, px};
use patchwork_core::settings::KeepWorktrees;
use patchwork_core::threads::WorktreeSweep;

use crate::settings::SettingsDialog;
use crate::settings::controls::{choice, section_heading, select_box, switch, token_row};
use crate::ui::theme::{Theme, fs_sm, theme};
use crate::ui::{BtnKind, btn};

/// What a project's worktree's folder is kept for, and their names.
const KEEPS: [(KeepWorktrees, &str); 3] = [
    (KeepWorktrees::Uncommitted, "Uncommitted changes"),
    (KeepWorktrees::Tracked, "Tracked changes"),
    (KeepWorktrees::AnyFiles, "Any local files"),
];

/// The cleanups the pane runs, and their rows' names.
const SWEEPS: [(WorktreeSweep, &str); 4] = [
    (WorktreeSweep::Merged, "Delete merged"),
    (WorktreeSweep::Inactive, "Delete inactive"),
    (WorktreeSweep::Archived, "Delete archived"),
    (WorktreeSweep::Unchanged, "Delete unchanged"),
];

/// The pane's own state: the cleanup running, and what the last one did.
#[derive(Default)]
pub struct WorktreesState {
    running: Option<WorktreeSweep>,
    /// The last cleanup, and how many worktrees it removed.
    done: Option<(WorktreeSweep, usize)>,
}

impl SettingsDialog {
    /// Removes the task worktrees `sweep` picks, off the main thread.
    fn clean_up_worktrees(&mut self, sweep: WorktreeSweep, cx: &mut Context<Self>) {
        if self.worktrees.running.is_some() {
            return;
        }
        self.worktrees.running = Some(sweep);
        self.worktrees.done = None;
        let core = Arc::clone(&self.core);
        let task = cx.background_spawn(async move { core.clean_up_worktrees(sweep) });
        cx.spawn(async move |this, cx| {
            let removed = task.await;
            let _ = this.update(cx, |d, cx| {
                d.worktrees.running = None;
                d.worktrees.done = Some((sweep, removed));
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

pub fn render(d: &mut SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let worktrees = d.settings.worktrees.clone();
    let fork_new = worktrees.fork_into_new;
    let fork_row = token_row("Fork into a new worktree", None, &t).child(
        switch("worktrees-fork", "Fork into a new worktree", fork_new, &t).on_click(
            cx.listener(move |d, _, _, cx| d.update(|s| s.worktrees.fork_into_new = !fork_new, cx)),
        ),
    );
    let with_tasks = worktrees.cleanup.with_deleted_tasks;
    let deleted = token_row("Delete with deleted tasks", None, &t).child(
        switch(
            "worktrees-deleted",
            "Delete with deleted tasks",
            with_tasks,
            &t,
        )
        .on_click(cx.listener(move |d, _, _, cx| {
            d.update(|s| s.worktrees.cleanup.with_deleted_tasks = !with_tasks, cx);
        })),
    );
    let sweeps: Vec<_> = SWEEPS
        .iter()
        .map(|&(sweep, title)| sweep_row(sweep, title, &t, d, cx))
        .collect();
    let keep = keep_row(worktrees.cleanup.keep, &t, d, cx);
    div()
        .flex()
        .flex_col()
        .child(section_heading("Forks", true, &t))
        .child(fork_row)
        .child(section_heading("Cleanup", false, &t))
        .child(deleted)
        .child(keep)
        .children(sweeps)
        .into_any_element()
}

/// A cleanup's row: its button, and how many worktrees it removed last.
fn sweep_row(
    sweep: WorktreeSweep,
    title: &'static str,
    t: &Theme,
    d: &SettingsDialog,
    cx: &mut Context<SettingsDialog>,
) -> Stateful<Div> {
    let state = &d.worktrees;
    let running = state.running == Some(sweep);
    let busy = state.running.is_some();
    let done = state
        .done
        .filter(|(s, _)| *s == sweep)
        .map(|(_, removed)| match removed {
            0 => "None to delete".to_owned(),
            1 => "Deleted 1".to_owned(),
            n => format!("Deleted {n}"),
        });
    token_row(title, None, t)
        .children(done.map(|text| {
            div()
                .id(SharedString::from(format!("worktrees-done-{title}")))
                .role(Role::Status)
                .text_size(px(fs_sm()))
                .text_color(t.text_muted)
                .child(text)
        }))
        .child(
            btn(
                SharedString::from(format!("worktrees-sweep-{title}")),
                title,
                BtnKind::Plain,
                busy,
                t,
            )
            .child(if running { "Deleting…" } else { "Delete" })
            .when(!busy, |b| {
                b.on_click(cx.listener(move |d, _, _, cx| d.clean_up_worktrees(sweep, cx)))
            }),
        )
}

/// The row whose dropdown picks what keeps a worktree's folder.
fn keep_row(
    keep: KeepWorktrees,
    t: &Theme,
    d: &SettingsDialog,
    cx: &mut Context<SettingsDialog>,
) -> Stateful<Div> {
    let key = SharedString::from("worktrees-keep");
    let name = SharedString::from("Keep when");
    let current = KEEPS
        .iter()
        .find(|(k, _)| *k == keep)
        .map_or("", |(_, label)| label);
    let open = d.menu_open(&key);
    let (menu_key, menu_name) = (key.clone(), name.clone());
    token_row(name.clone(), None, t).child(select_box(key, name, current, open, t).on_click(
        cx.listener(move |d, e: &ClickEvent, window, cx| {
            let weak = cx.entity().downgrade();
            let items = KEEPS
                .iter()
                .map(|&(k, label)| {
                    choice(weak.clone(), label, k == keep, move |d, cx| {
                        d.menu = None;
                        d.update(|s| s.worktrees.cleanup.keep = k, cx);
                    })
                })
                .collect();
            d.toggle_menu(
                menu_key.clone(),
                menu_name.clone(),
                items,
                e.position(),
                window,
                cx,
            );
        }),
    ))
}
