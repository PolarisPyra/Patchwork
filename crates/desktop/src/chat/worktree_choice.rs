//! Where a project's tasks work: the composer's switch between a worktree for each new task and
//! the project's folder, and the line that tells, once per project, that its tasks work in
//! worktrees.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{Role, SharedString, Stateful, div, prelude::*, px};
use patchwork_core::system::ssh::is_remote;
use patchwork_core::projects::Project;

use crate::chat::goal::strip_action;
use crate::ui::theme::{Theme, fs_sm, radius_sm};
use crate::ui::{button, icon, icon_button, tooltip};
use crate::workspace::Workspace;

impl Workspace {
    /// The project whose tasks the composer's switch picks a worktree or the folder for: the
    /// open task's, or the one the next message starts a task in. None when it has none, and
    /// why it can't have worktrees when it is no git repository on this computer.
    fn pick_project(&self) -> Result<&Project, &'static str> {
        let project = self
            .project()
            .ok_or("Pick a project to work in a worktree")?;
        if is_remote(&project.path) {
            return Err("Projects over SSH edit their folder");
        }
        if self.branch.is_none() {
            return Err("Only a git repository can have worktrees");
        }
        Ok(project)
    }

    /// Flips whether the project's new tasks work in a worktree each or edit its folder, as its
    /// switch in Settings → Worktrees does.
    fn flip_worktree_pick(&mut self, cx: &mut Context<Self>) {
        let Ok(project) = self.pick_project().map(|p| p.id.clone()) else {
            return;
        };
        let in_place =
            !self.settings.worktrees.disabled && !self.settings.edit_in_place.contains(&project);
        self.settings.set_edit_in_place(&project, in_place);
        self.change(
            "change where the project's tasks work",
            move |core| core.set_edits_in_place(&project, in_place),
            cx,
        );
        cx.notify();
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

/// The composer's switch between a worktree for each new task and the project's folder, the
/// same setting as the project's switch in Settings → Worktrees. It always shows, and says why
/// when the project can't have worktrees.
pub fn worktree_pick_chip(
    w: &Workspace,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Stateful<gpui::Div> {
    let picked = w.pick_project();
    let on = picked.is_ok_and(|project| {
        !w.settings.worktrees.disabled && !w.settings.edit_in_place.contains(&project.id)
    });
    let (label, glyph) = if on {
        ("Worktree", "folders")
    } else {
        ("Project folder", "folder")
    };
    let (muted, fg) = (t.text_muted, t.text);
    let chip = button("worktree-pick", format!("New tasks work in: {label}"))
        .aria_toggled(on.into())
        .flex_none()
        .gap(px(6.))
        .text_color(muted)
        .child(icon(glyph, 13.).flex_none())
        .child(div().whitespace_nowrap().child(label));
    match picked {
        Err(why) => chip
            .aria_label(format!("New tasks work in: {label}. {why}"))
            .opacity(0.6)
            .cursor_default()
            .tooltip(tooltip(why)),
        Ok(_) => chip
            .hover(move |s| s.text_color(fg))
            .tooltip(tooltip(if on {
                "New tasks edit the project folder instead"
            } else {
                "New tasks work in a worktree each"
            }))
            .on_click(cx.listener(|w, _, _, cx| w.flip_worktree_pick(cx))),
    }
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
