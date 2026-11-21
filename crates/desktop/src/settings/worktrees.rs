//! Settings → Worktrees: whether tasks get worktrees at all and in which projects, whether a fork
//! gets one of its own, what cleaning up keeps, and the button that deletes stale task worktrees
//! now.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{AnyElement, Div, Role, SharedString, Stateful, div, prelude::*, px};
use patchwork_core::settings::KeepWorktrees;

use crate::settings::SettingsDialog;
use crate::settings::controls::{choices, section_heading, select_box, switch, switch_row, token_row};
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
        crate::ui::off_thread(
            cx,
            move || core.delete_stale_worktrees(open.as_deref()),
            move |d, removed, cx| {
                d.worktrees.running = false;
                d.worktrees.done = Some(removed);
                cx.notify();
            },
        );
        cx.notify();
    }
}

pub fn render(d: &mut SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let worktrees = d.settings.worktrees.clone();
    let on = !worktrees.disabled;
    let use_row = switch_row(
        "worktrees-on",
        "Give tasks their own worktree",
        None,
        on,
        |d, on, cx| d.update(|s| s.worktrees.disabled = !on, cx),
        cx,
    );
    let fork_row = switch_row(
        "worktrees-fork",
        "Fork into a new worktree",
        None,
        worktrees.fork_into_new,
        |d, on, cx| d.update(|s| s.worktrees.fork_into_new = on, cx),
        cx,
    );
    let deleted = switch_row(
        "worktrees-deleted",
        "Delete with deleted tasks",
        None,
        worktrees.cleanup.with_deleted_tasks,
        |d, on, cx| d.update(|s| s.worktrees.cleanup.with_deleted_tasks = on, cx),
        cx,
    );
    let projects = (on).then(|| project_rows(d, &t, cx));
    let stale = stale_row(&t, d, cx);
    let keep = keep_row(worktrees.cleanup.keep, &t, d, cx);
    div()
        .flex()
        .flex_col()
        .child(section_heading("Tasks", true, &t))
        .child(use_row)
        .children(projects)
        .child(section_heading("Forks", false, &t))
        .child(fork_row)
        .child(section_heading("Cleanup", false, &t))
        .child(deleted)
        .child(keep)
        .child(stale)
        .into_any_element()
}

/// A switch for each project on this computer: whether its new tasks get a worktree each or edit
/// its folder. Tasks that already started keep where they work.
fn project_rows(d: &SettingsDialog, t: &Theme, cx: &mut Context<SettingsDialog>) -> Div {
    let rows: Vec<_> = d
        .settings
        .projects
        .iter()
        .filter(|p| !patchwork_core::system::ssh::is_remote(&p.path))
        .map(|p| {
            let on = !d.settings.edit_in_place.contains(&p.id);
            let id = p.id.clone();
            token_row(p.name.clone(), None, t).child(
                switch(format!("worktrees-project-{}", p.id), p.name.clone(), on, t).on_click(
                    cx.listener(move |d, _, _, cx| {
                        let id = id.clone();
                        d.update(
                            move |s| {
                                s.edit_in_place.retain(|p| p != &id);
                                if on {
                                    s.edit_in_place.push(id.clone());
                                }
                            },
                            cx,
                        );
                    }),
                ),
            )
        })
        .collect();
    div()
        .flex()
        .flex_col()
        .when(!rows.is_empty(), |d| {
            d.child(section_heading("Projects", false, t))
        })
        .children(rows)
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
    let options = KEEPS.iter().map(|&(k, label)| (k, label.into())).collect();
    let set = |d: &mut SettingsDialog, k, cx: &mut Context<SettingsDialog>| {
        d.update(|s| s.worktrees.cleanup.keep = k, cx);
    };
    token_row(name.clone(), None, t).child(
        select_box(key.clone(), name.clone(), current, open, t)
            .on_click(choices(key, name, options, keep, set, cx)),
    )
}
