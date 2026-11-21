//! Where a task works, said before it starts and once it has: the composer's switch between a
//! worktree of its own and the project's folder for a task whose first turn hasn't run, and the
//! line that tells, once per project, that its tasks work in worktrees.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{Role, SharedString, Stateful, div, prelude::*, px};
use patchwork_core::system::ssh::is_remote;
use patchwork_core::threads::Role as MessageRole;

use crate::chat::goal::strip_action;
use crate::ui::theme::{Theme, fs_sm, radius_sm};
use crate::ui::{button, icon, icon_button, tooltip};
use crate::workspace::Workspace;

/// What was picked for where a task that hasn't started works.
pub struct WorktreePick {
    /// The task's id, or `new:` and the project's id for the task the composer's next message
    /// starts.
    key: String,
    /// Whether it gets a worktree of its own; None while the pick is read.
    on: Option<bool>,
    /// Whether the user picked it, so the task the next message starts keeps it.
    picked: bool,
}

impl Workspace {
    /// The task whose worktree can still be picked: its key, its id once it exists, and its
    /// project's id. That is the open task before its first turn, or the task the composer's
    /// next message starts, in a git repository on this computer.
    fn pick_target(&self) -> Option<(String, Option<String>, String)> {
        let project = self.project()?;
        if is_remote(&project.path) || self.branch.is_none() || self.bot_shown.is_some() {
            return None;
        }
        match &self.thread {
            None => Some((format!("new:{}", project.id), None, project.id.clone())),
            Some(thread) => {
                let unstarted = thread.parent.is_none()
                    && thread.session_id.is_none()
                    && !thread
                        .messages
                        .iter()
                        .any(|m| m.role == MessageRole::Assistant);
                (unstarted && self.thread_folder().is_none()).then(|| {
                    let id = thread.id.clone();
                    (id.clone(), Some(id), project.id.clone())
                })
            }
        }
    }

    /// Reads what was picked for the task whose worktree can still be picked, off the main
    /// thread, when that task changed. A task nothing was picked for works in the project's
    /// folder until a worktree is picked.
    pub(crate) fn load_worktree_pick(&mut self, cx: &mut Context<Self>) {
        let Some((key, thread, _)) = self.pick_target() else {
            return;
        };
        if self.worktree_pick.as_ref().is_some_and(|p| p.key == key) {
            return;
        }
        let Some(thread) = thread else {
            // Picked, so the task the next message starts keeps it over the project's setting.
            self.worktree_pick = Some(WorktreePick {
                key,
                on: Some(false),
                picked: true,
            });
            return;
        };
        self.worktree_pick = Some(WorktreePick {
            key: key.clone(),
            on: None,
            picked: false,
        });
        let core = Arc::clone(&self.core);
        let read = cx.background_spawn(async move {
            core.task_worktree_pick(&thread).unwrap_or_else(|| {
                if let Err(e) = core.set_task_gets_worktree(&thread, false) {
                    log::warn!(error:% = e; "could not pick where the task works: {{error}}");
                }
                false
            })
        });
        cx.spawn(async move |this, cx| {
            let on = read.await;
            this.update(cx, |this, cx| {
                if let Some(pick) = this.worktree_pick.as_mut().filter(|p| p.key == key) {
                    pick.on = Some(on);
                    cx.notify();
                }
            })
        })
        .detach();
    }

    /// Flips where the task whose worktree can still be picked works.
    fn flip_worktree_pick(&mut self, cx: &mut Context<Self>) {
        let Some((key, thread, _)) = self.pick_target() else {
            return;
        };
        let Some(pick) = self.worktree_pick.as_mut().filter(|p| p.key == key) else {
            return;
        };
        let Some(on) = pick.on.map(|on| !on) else {
            return;
        };
        pick.on = Some(on);
        pick.picked = true;
        if let Some(id) = thread {
            let core = Arc::clone(&self.core);
            crate::ui::off_thread(
                cx,
                move || core.set_task_gets_worktree(&id, on),
                |_, done, _| {
                    if let Err(e) = done {
                        log::warn!(error:% = e; "could not pick where the task works: {{error}}");
                    }
                },
            );
        }
        cx.notify();
    }

    /// Gives task `thread_id`, just started in `project_id` by the composer's message, what was
    /// picked for it there.
    pub(crate) fn apply_worktree_pick(&mut self, thread_id: &str, project_id: &str) {
        let key = format!("new:{project_id}");
        let Some(pick) = self.worktree_pick.take() else {
            return;
        };
        if pick.key != key || !pick.picked {
            return;
        }
        if let Some(on) = pick.on
            && let Err(e) = self.core.set_task_gets_worktree(thread_id, on)
        {
            log::warn!(error:% = e; "could not pick where the task works: {{error}}");
        }
    }

    /// Reads whether the user was told where the open task's project's tasks work, off the main
    /// thread, once per project.
    pub(crate) fn load_worktree_told(&mut self, cx: &mut Context<Self>) {
        let Some(project) = self
            .thread_folder()
            .and(self.project())
            .map(|p| p.id.clone())
        else {
            return;
        };
        if self.worktree_told.contains_key(&project) {
            return;
        }
        self.worktree_told.insert(project.clone(), None);
        let core = Arc::clone(&self.core);
        let id = project.clone();
        let read = cx.background_spawn(async move { core.worktree_told(&id) });
        cx.spawn(async move |this, cx| {
            let told = read.await;
            this.update(cx, |this, cx| {
                this.worktree_told.insert(project, Some(told));
                cx.notify();
            })
        })
        .detach();
    }

    /// Puts the told line away for good in project `project_id`, and with `off`, has its new
    /// tasks edit its folder.
    fn dismiss_worktree_told(&mut self, project_id: String, off: bool, cx: &mut Context<Self>) {
        self.worktree_told.insert(project_id.clone(), Some(true));
        self.change(
            "put away where the project's tasks work",
            move |core| {
                core.set_worktree_told(&project_id)?;
                if off {
                    core.set_edits_in_place(&project_id, true)?;
                }
                Ok(())
            },
            cx,
        );
        cx.notify();
    }
}

/// The composer's switch between a worktree of its own and the project's folder, for the task
/// whose worktree can still be picked. Hidden while worktrees are turned off in Settings.
pub fn worktree_pick_chip(
    w: &Workspace,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Option<Stateful<gpui::Div>> {
    if w.settings.worktrees.disabled {
        return None;
    }
    let (key, _, _) = w.pick_target()?;
    let on = w.worktree_pick.as_ref().filter(|p| p.key == key)?.on?;
    let (label, glyph) = if on {
        ("Worktree", "folders")
    } else {
        ("Project folder", "folder")
    };
    let (muted, fg) = (t.text_muted, t.text);
    Some(
        button("worktree-pick", format!("Works in: {label}"))
            .aria_toggled(on.into())
            .flex_none()
            .gap(px(6.))
            .text_color(muted)
            .hover(move |s| s.text_color(fg))
            .tooltip(tooltip(if on {
                "Edit the project folder instead"
            } else {
                "Work in a worktree"
            }))
            .child(icon(glyph, 13.).flex_none())
            .child(div().whitespace_nowrap().child(label))
            .on_click(cx.listener(|w, _, _, cx| w.flip_worktree_pick(cx))),
    )
}

/// The line that says, the first time a project's task changes something in its worktree, where
/// it works, with what to do about it.
pub fn worktree_told_line(
    w: &Workspace,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Option<Stateful<gpui::Div>> {
    let folder = w.thread_folder()?;
    let project = w.project()?;
    if !w.thread_worktree_changed()
        || w.worktree_told.get(&project.id).copied().flatten() != Some(false)
    {
        return None;
    }
    let text: SharedString =
        format!("This task works in its own copy of the project: {folder}").into();
    let (id_open, id_off, id_done) = (project.id.clone(), project.id.clone(), project.id.clone());
    let local = !is_remote(&folder);
    Some(
        div()
            .id("worktree-told")
            .role(Role::Status)
            .aria_label(text.clone())
            .mb(px(6.))
            .px(px(10.))
            .py(px(6.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(radius_sm()))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_panel)
            .text_size(px(fs_sm()))
            .child(icon("folders", 13.).flex_none().text_color(t.accent_text))
            .child(
                div()
                    .id("worktree-told-text")
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .tooltip(tooltip(text.clone()))
                    .child(text),
            )
            .when(local, |d| {
                d.child(
                    strip_action("worktree-told-open", "Open folder", t).on_click(cx.listener(
                        move |w, _, _, cx| {
                            cx.open_with_system(std::path::Path::new(&folder));
                            w.dismiss_worktree_told(id_open.clone(), false, cx);
                        },
                    )),
                )
            })
            .child(
                strip_action("worktree-told-off", "Edit project folder next time", t)
                    .tooltip(tooltip("New tasks in this project edit its folder"))
                    .on_click(cx.listener(move |w, _, _, cx| {
                        w.dismiss_worktree_told(id_off.clone(), true, cx);
                    })),
            )
            .child(
                icon_button("worktree-told-close", "Dismiss", "x", 12., cx)
                    .flex_none()
                    .on_click(cx.listener(move |w, _, _, cx| {
                        w.dismiss_worktree_told(id_done.clone(), false, cx);
                    })),
            ),
    )
}
