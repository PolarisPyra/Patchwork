//! Settings → Worktrees: whether tasks get worktrees at all, whether a fork gets one of its own,
//! what cleaning up keeps, and the button that deletes stale task worktrees now.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{AnyElement, ClickEvent, Div, Role, SharedString, Stateful, div, prelude::*, px};
use patchwork_core::settings::KeepWorktrees;

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

/// The pane's own state: the task open behind Settings, whose worktree is never stale, whether
/// stale worktrees are being deleted, and how many the last run deleted.
#[derive(Default)]
pub struct WorktreesState {
    open: Option<String>,
    running: bool,
    done: Option<usize>,
}

impl SettingsDialog {
    /// Keeps the worktree of `thread`, the task open behind Settings, when stale ones are deleted.
    pub fn set_open_thread(&mut self, thread: Option<String>) {
        self.worktrees.open = thread;
    }

    /// Deletes the stale task worktrees, off the main thread.
    fn delete_stale_worktrees(&mut self, cx: &mut Context<Self>) {
        if self.worktrees.running {
            return;
        }
        self.worktrees.running = true;
        self.worktrees.done = None;
        let core = Arc::clone(&self.core);
        let open = self.worktrees.open.clone();
        let task = cx.background_spawn(async move { core.delete_stale_worktrees(open.as_deref()) });
        cx.spawn(async move |this, cx| {
            let removed = task.await;
            let _ = this.update(cx, |d, cx| {
                d.worktrees.running = false;
                d.worktrees.done = Some(removed);
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
    let on = !worktrees.disabled;
    let use_row = token_row("Give tasks their own worktree", None, &t).child(
        switch("worktrees-on", "Give tasks their own worktree", on, &t)
            .on_click(cx.listener(move |d, _, _, cx| d.update(|s| s.worktrees.disabled = on, cx))),
    );
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
    let stale = stale_row(&t, d, cx);
    let keep = keep_row(worktrees.cleanup.keep, &t, d, cx);
    div()
        .flex()
        .flex_col()
        .child(section_heading("Tasks", true, &t))
        .child(use_row)
        .child(section_heading("Forks", false, &t))
        .child(fork_row)
        .child(section_heading("Cleanup", false, &t))
        .child(deleted)
        .child(keep)
        .child(stale)
        .into_any_element()
}

/// The row whose button deletes stale worktrees, with how many it deleted last.
fn stale_row(t: &Theme, d: &SettingsDialog, cx: &mut Context<SettingsDialog>) -> Stateful<Div> {
    let title = "Delete stale worktrees";
    let running = d.worktrees.running;
    let done = d.worktrees.done.map(|removed| match removed {
        0 => "None to delete".to_owned(),
        1 => "Deleted 1".to_owned(),
        n => format!("Deleted {n}"),
    });
    token_row(title, None, t)
        .children(done.map(|text| {
            div()
                .id("worktrees-stale-done")
                .role(Role::Status)
                .text_size(px(fs_sm()))
                .text_color(t.text_muted)
                .child(text)
        }))
        .child(
            btn("worktrees-stale", title, BtnKind::Plain, running, t)
                .child(if running { "Deleting…" } else { "Delete" })
                .when(!running, |b| {
                    b.on_click(cx.listener(|d, _, _, cx| d.delete_stale_worktrees(cx)))
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
