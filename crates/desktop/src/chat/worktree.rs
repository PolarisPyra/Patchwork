//! The status above the composer of a task that works in its own worktree: what it changed that
//! the project doesn't have yet, and Merge and Discard. Its files open on their diffs.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{
    FontWeight, PromptButton, PromptLevel, Role, SharedString, Stateful, Window, div, prelude::*,
    px,
};
use patchwork_core::threads::WorktreeState;

use crate::canvas::CanvasTab;
use crate::chat::goal::strip_action;
use crate::ui::theme::{Theme, fs_sm, fs_xs, radius_sm};
use crate::ui::{button, icon, tooltip};
use crate::workspace::{Workspace, is_working};

/// The tallest the list of files grows before it scrolls.
const FILES_MAX_HEIGHT: f32 = 140.;

/// The open task's worktree as last read, and what the strip is doing with it.
pub struct WorktreeStrip {
    /// The task, and what it looked like when its worktree was read, so a change reads it again.
    key: (String, usize, bool),
    state: Option<WorktreeState>,
    busy: Option<&'static str>,
    error: Option<String>,
    /// The last merge conflicted, so the strip offers to have the task resolve it.
    conflicted: bool,
    note: Option<String>,
    open: bool,
}

/// The key a task's worktree is read again on: its id, how many messages it has, and whether
/// it is working.
fn key_of(w: &Workspace) -> Option<(String, usize, bool)> {
    let thread = w.thread.as_ref()?;
    thread.project_id.as_ref()?;
    Some((thread.id.clone(), thread.messages.len(), is_working(thread)))
}

impl Workspace {
    /// The folder the open task works in when that is its own worktree.
    pub fn thread_folder(&self) -> Option<String> {
        let id = &self.thread.as_ref()?.id;
        let strip = self.worktree.as_ref().filter(|s| &s.key.0 == id)?;
        strip.state.as_ref().map(|s| s.worktree.root.clone())
    }

    /// Reads the open task's worktree off the main thread when the task changed since it was
    /// last read.
    pub(crate) fn load_worktree(&mut self, cx: &mut Context<Self>) {
        let Some(key) = key_of(self) else {
            self.worktree = None;
            return;
        };
        if self.worktree.as_ref().is_some_and(|s| s.key == key) {
            return;
        }
        let same_task = self.worktree.as_ref().is_some_and(|s| s.key.0 == key.0);
        match self.worktree.as_mut().filter(|_| same_task) {
            Some(strip) => strip.key = key.clone(),
            None => {
                self.worktree = Some(WorktreeStrip {
                    key: key.clone(),
                    state: None,
                    busy: None,
                    error: None,
                    conflicted: false,
                    note: None,
                    open: false,
                });
            }
        }
        let core = Arc::clone(&self.core);
        let id = key.0.clone();
        let task = cx.background_spawn(async move { core.worktree_state(&id) });
        cx.spawn(async move |this, cx| {
            let read = task.await;
            this.update(cx, |this, cx| {
                let Some(strip) = this.worktree.as_mut().filter(|s| s.key == key) else {
                    return;
                };
                let had = strip.state.as_ref().map(|s| s.worktree.root.clone());
                match read {
                    Ok(state) => strip.state = state,
                    Err(e) => {
                        log::warn!(error:% = e; "could not read the task's worktree: {{error}}");
                    }
                }
                // Replies' files were read from the project's folder before the worktree was known.
                if strip.state.as_ref().map(|s| s.worktree.root.clone()) != had {
                    this.reply_files.clear();
                }
                cx.notify();
            })
        })
        .detach();
    }

    /// Reads the open task's worktree again, whatever was read before.
    fn reload_worktree(&mut self, cx: &mut Context<Self>) {
        if let Some(strip) = self.worktree.as_mut() {
            strip.key.1 = usize::MAX;
        }
        self.load_worktree(cx);
    }

    /// Runs `step` on the open task's worktree off the main thread, then reads it again.
    fn worktree_step(
        &mut self,
        busy: &'static str,
        step: impl FnOnce(&patchwork_core::Core, &str) -> Result<Option<String>, patchwork_core::Error>
        + Send
        + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(strip) = self.worktree.as_mut() else {
            return;
        };
        if strip.busy.is_some() {
            return;
        }
        strip.busy = Some(busy);
        strip.error = None;
        strip.note = None;
        let id = strip.key.0.clone();
        let core = Arc::clone(&self.core);
        let task = {
            let id = id.clone();
            cx.background_spawn(async move { step(&core, &id) })
        };
        cx.spawn(async move |this, cx| {
            let done = task.await;
            this.update(cx, |this, cx| {
                if let Some(strip) = this.worktree.as_mut().filter(|s| s.key.0 == id) {
                    strip.busy = None;
                    match done {
                        Ok(note) => {
                            strip.note = note;
                            strip.conflicted = false;
                        }
                        Err(e) => {
                            let e = e.to_string();
                            strip.conflicted = e.starts_with("Merging would conflict");
                            strip.error = Some(e);
                        }
                    }
                }
                this.reload_worktree(cx);
                this.load_git(cx);
            })
        })
        .detach();
        cx.notify();
    }

    fn merge_worktree(&mut self, cx: &mut Context<Self>) {
        self.worktree_step(
            "Merging…",
            |core, id| {
                core.merge_thread(id)
                    .map(|into| Some(format!("Merged into {into}")))
            },
            cx,
        );
    }

    fn discard_worktree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let answer = window.prompt(
            PromptLevel::Warning,
            "Discard this task's changes?",
            Some("Its files go back to how the project has them, and its commits that aren't merged are dropped."),
            &[PromptButton::ok("Discard"), PromptButton::cancel("Keep")],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.worktree_step(
                    "Discarding…",
                    |core, id| core.discard_thread_changes(id).map(|()| None),
                    cx,
                );
            });
        })
        .detach();
    }

    /// Asks the task to bring in the project's branch and resolve what conflicts.
    fn resolve_worktree(&mut self, cx: &mut Context<Self>) {
        let into = self
            .branch
            .clone()
            .unwrap_or_else(|| "the project's branch".into());
        if let Some(strip) = self.worktree.as_mut() {
            strip.conflicted = false;
            strip.error = None;
        }
        self.send(
            format!("Merge {into} into this branch, resolve the conflicts, and commit the result."),
            Vec::new(),
            None,
            None,
            cx,
        );
    }
}

/// "3 files changed · 1 commit".
fn summary(state: &WorktreeState) -> String {
    let mut parts = Vec::new();
    match state.changes.len() {
        0 => {}
        1 => parts.push("1 file changed".to_owned()),
        n => parts.push(format!("{n} files changed")),
    }
    match state.commits {
        0 => {}
        1 => parts.push("1 commit".to_owned()),
        n => parts.push(format!("{n} commits")),
    }
    parts.join(" · ")
}

/// The open task's worktree, when it works in one and there is something to say about it: its
/// status for the line above the composer, beside the token use, and its changed files when that
/// status is opened, which go above that line.
pub fn worktree_strip(
    w: &Workspace,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> (Option<Stateful<gpui::Div>>, Option<Stateful<gpui::Div>>) {
    let working = w.thread.as_ref().is_some_and(is_working);
    let Some((strip, state)) = w
        .worktree
        .as_ref()
        .and_then(|strip| Some((strip, strip.state.clone()?)))
    else {
        return (None, None);
    };
    let pending = !state.changes.is_empty() || state.commits > 0;
    let (open, conflicted) = (strip.open, strip.conflicted);
    let message = strip
        .error
        .clone()
        .map(|e| (e, true))
        .or_else(|| strip.note.clone().map(|n| (n, false)));
    // Nothing to merge, nothing running and nothing to report: the line stays blank.
    if !pending && strip.busy.is_none() && message.is_none() && !conflicted {
        return (None, None);
    }
    let idle = strip.busy.is_none() && !working;
    let (muted, fg) = (t.text_muted, t.text);
    // Opening it shows the changed files and the Discard and Merge buttons.
    let can_open = pending;
    let branch: SharedString = state.worktree.branch.clone().into();
    let label: Option<SharedString> = strip
        .busy
        .map(str::to_owned)
        .or_else(|| pending.then(|| summary(&state)))
        .map(Into::into);
    let toggle = label.map(|label| {
        button("worktree-toggle", label.clone())
            .when(can_open, |b| b.aria_expanded(open))
            .flex_none()
            .gap(px(6.))
            .text_color(muted)
            .hover(move |s| s.text_color(fg))
            .tooltip(tooltip(format!("Working in its own worktree on {branch}")))
            .child(
                icon("git-branch", 13.)
                    .flex_none()
                    .text_color(t.accent_text),
            )
            .child(
                div()
                    .id("worktree-state")
                    .role(Role::Status)
                    .aria_label(label.clone())
                    .flex_none()
                    .whitespace_nowrap()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.text)
                    .child(label),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(t.text_subtle)
                    .child(branch.clone()),
            )
            .when(can_open, |b| {
                b.child(icon(if open { "caret-down" } else { "caret-right" }, 12.).flex_none())
            })
            .on_click(cx.listener(|w, _, _, cx| {
                if let Some(strip) = w.worktree.as_mut() {
                    strip.open = !strip.open;
                }
                cx.notify();
            }))
    });
    let message = message.map(|(text, failed)| {
        let shown: SharedString = text.into();
        div()
            .id("worktree-message")
            .role(if failed { Role::Alert } else { Role::Status })
            .aria_label(shown.clone())
            .min_w_0()
            .truncate()
            .text_color(if failed { t.danger } else { t.text_muted })
            .tooltip(tooltip(shown.clone()))
            .child(shown)
    });
    let status = div()
        .id("worktree")
        .role(Role::Region)
        .aria_label("Worktree")
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .children(toggle)
        .when(idle && conflicted, |d| {
            d.child(
                strip_action("worktree-resolve", "Ask to resolve", t)
                    .tooltip(tooltip("Have the task bring in the project's changes"))
                    .on_click(cx.listener(|w, _, _, cx| w.resolve_worktree(cx))),
            )
        })
        .children(message);
    let root = state.worktree.root.clone();
    let hover = t.bg_hover;
    let list = (!state.changes.is_empty()).then(|| {
        div()
            .id("worktree-files")
            .role(Role::List)
            .aria_label("Changed files")
            .max_h(px(FILES_MAX_HEIGHT))
            .overflow_y_scroll()
            .py(px(4.))
            .children(state.changes.iter().enumerate().map(|(i, change)| {
                let path = change.path.clone();
                let root = root.clone();
                let counts = format!("+{} −{}", change.additions, change.deletions);
                let name: SharedString = change.path.clone().into();
                button(("worktree-file", i), format!("{} {counts}", change.path))
                    .role(Role::ListItem)
                    .w_full()
                    .px(px(10.))
                    .py(px(3.))
                    .gap(px(8.))
                    .hover(move |s| s.bg(hover))
                    .child(icon("git-diff", 12.).flex_none().text_color(t.text_subtle))
                    .child(div().min_w_0().flex_1().truncate().child(name))
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(fs_xs()))
                            .text_color(t.text_subtle)
                            .child(counts),
                    )
                    .on_click(cx.listener(move |w, _, window, cx| {
                        w.open_canvas_at(&path, root.clone(), None, CanvasTab::Diff, window, cx);
                    }))
            }))
    });
    let actions = idle.then(|| {
        div()
            .flex()
            .justify_end()
            .gap(px(8.))
            .px(px(10.))
            .py(px(6.))
            .when(list.is_some(), |d| d.border_t_1().border_color(t.border))
            .child(
                strip_action("worktree-discard", "Discard", t)
                    .tooltip(tooltip("Throw away the task's changes"))
                    .on_click(cx.listener(|w, _, window, cx| w.discard_worktree(window, cx))),
            )
            .child(
                strip_action("worktree-merge", "Merge", t)
                    .tooltip(tooltip("Bring the task's changes into the project"))
                    .on_click(cx.listener(|w, _, _, cx| w.merge_worktree(cx))),
            )
    });
    let panel = (open && can_open && (list.is_some() || actions.is_some())).then(|| {
        div()
            .id("worktree-panel")
            .role(Role::Region)
            .aria_label("Worktree changes")
            .mb(px(6.))
            .rounded(px(radius_sm()))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_panel)
            .text_size(px(fs_sm()))
            .shadow_sm()
            .children(list)
            .children(actions)
    });
    (Some(status), panel)
}
