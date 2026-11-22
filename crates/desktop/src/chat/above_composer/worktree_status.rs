//! The status above the composer of a task that works in its own worktree: what it changed that
//! the project doesn't have yet, how far behind the project it is, and Merge, Update and Discard.
//! Its files open on their diffs, and its menu copies or opens its folder.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use futures::channel::oneshot;
use gpui::{
    ClickEvent, ClipboardItem, Entity, FontWeight, Pixels, Point, PromptButton, PromptLevel, Role,
    SharedString, Stateful, Window, div, prelude::*, px,
};
use patchwork_core::system::ssh::is_remote;
use patchwork_core::worktree::WorktreeState;
use patchwork_core::{Core, Error};

use crate::canvas::CanvasTab;
use super::goal_bar::strip_action;
use patchwork_ui::controls::menu::{Dismissed, Menu, MenuItem};
use patchwork_ui::theme::{Theme, fs_sm, fs_xs, radius_sm};
use patchwork_ui::{button, icon, icon_button, tooltip};
use crate::workspace::{Workspace, is_working};

/// The tallest the list of files grows before it scrolls.
const FILES_MAX_HEIGHT: f32 = 140.;

/// What the strip does to the task's worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Merge,
    Update,
    Discard,
    SwitchBack,
    CleanUp,
}

impl Step {
    fn busy(self) -> &'static str {
        match self {
            Self::Merge => "Merging…",
            Self::Update => "Updating…",
            Self::Discard => "Discarding…",
            Self::SwitchBack => "Switching back…",
            Self::CleanUp => "Cleaning up…",
        }
    }

    /// Runs the step on task `id`, and returns what to tell the user when it went through.
    /// `waiting` is called when another merge holds a merge up.
    fn run(self, core: &Core, id: &str, waiting: impl FnOnce()) -> Result<Option<String>, Error> {
        match self {
            Self::Merge => core
                .merge_chat(id, waiting)
                .map(|into| Some(format!("Merged into {into}"))),
            Self::Update => core
                .update_worktree(id)
                .map(|from| Some(format!("Brought in {from}"))),
            Self::Discard => core.discard_chat_changes(id).map(|()| None),
            Self::SwitchBack => core.restore_worktree_branch(id).map(|()| None),
            Self::CleanUp => core
                .clean_up_worktree(id)
                .map(|_| Some("Cleaned up".to_owned())),
        }
    }
}

/// The open task's worktree as last read, and what the strip is doing with it.
pub struct WorktreeStrip {
    /// The task, and what it looked like when its worktree was read, so a change reads it again.
    key: (String, usize, bool),
    state: Option<WorktreeState>,
    /// Why the task has no worktree although it was to get one, so it edits the project itself.
    failed: Option<String>,
    busy: Option<&'static str>,
    error: Option<String>,
    /// The last merge conflicted, so the strip offers to have the task resolve it.
    conflicted: bool,
    /// The last step ran out of time, so the strip offers to run it again.
    retry: Option<Step>,
    note: Option<String>,
    open: bool,
    /// The worktree's menu and where it opened.
    menu: Option<(Entity<Menu>, Point<Pixels>)>,
    /// Where the press that closed the menu landed, so it doesn't open the menu again.
    menu_dismissed: Option<Point<Pixels>>,
}

/// The key a task's worktree is read again on: its id, how many messages it has, and whether
/// it is working.
fn key_of(w: &Workspace) -> Option<(String, usize, bool)> {
    let chat = w.chat.as_ref()?;
    chat.project_id.as_ref()?;
    Some((chat.id.clone(), chat.messages.len(), is_working(chat)))
}

impl Workspace {
    /// The folder the open task works in when that is its own worktree.
    pub fn chat_folder(&self) -> Option<String> {
        let id = &self.chat.as_ref()?.id;
        let strip = self.worktree.as_ref().filter(|s| &s.key.0 == id)?;
        strip.state.as_ref().map(|s| s.worktree.root.clone())
    }

    /// Whether the open task's worktree holds anything yet: files changed there, or commits.
    pub(crate) fn chat_worktree_changed(&self) -> bool {
        let id = self.chat.as_ref().map(|t| &t.id);
        self.worktree
            .as_ref()
            .filter(|s| Some(&s.key.0) == id)
            .and_then(|s| s.state.as_ref())
            .is_some_and(|s| !s.changes.is_empty() || s.commits > 0)
    }

    /// Reads the open task's worktree off the main thread when the task changed since it was
    /// last read.
    pub(crate) fn load_worktree(&mut self, cx: &mut Context<Self>) {
        let Some(key) = key_of(self) else {
            self.park_worktree();
            return;
        };
        if self.worktree.as_ref().is_some_and(|s| s.key == key) {
            return;
        }
        let same_task = self.worktree.as_ref().is_some_and(|s| s.key.0 == key.0);
        if let Some(strip) = self.worktree.as_mut().filter(|_| same_task) {
            strip.key = key.clone();
        } else {
            self.park_worktree();
            let strip = match self.parked_worktrees.remove(&key.0) {
                Some(parked) => WorktreeStrip {
                    key: key.clone(),
                    ..parked
                },
                None => WorktreeStrip {
                    key: key.clone(),
                    state: None,
                    failed: None,
                    busy: None,
                    error: None,
                    conflicted: false,
                    retry: None,
                    note: None,
                    open: false,
                    menu: None,
                    menu_dismissed: None,
                },
            };
            self.worktree = Some(strip);
        }
        let core = Arc::clone(&self.core);
        let id = key.0.clone();
        let task =
            cx.background_spawn(
                async move { (core.worktree_state(&id), core.worktree_failure(&id)) },
            );
        cx.spawn(async move |this, cx| {
            let (read, failed) = task.await;
            this.update(cx, |this, cx| {
                let Some(strip) = this.worktree.as_mut().filter(|s| s.key == key) else {
                    return;
                };
                let had = strip.state.as_ref().map(|s| s.worktree.root.clone());
                strip.failed = failed;
                match read {
                    Ok(state) => strip.state = state,
                    Err(e) => {
                        log::warn!(error:% = e; "could not read the task's worktree: {{error}}");
                    }
                }
                // Replies' files were read from the project's folder before the worktree was known.
                if strip.state.as_ref().map(|s| s.worktree.root.clone()) != had {
                    this.reply_files.clear();
                    // A task's first turn makes its worktree, which Source Control then shows.
                    this.read_git(false, cx);
                }
                cx.notify();
            })
        })
        .detach();
    }

    /// Sets the open task's strip aside as another task opens, keeping it while a step runs or
    /// left something to say.
    fn park_worktree(&mut self) {
        let Some(mut strip) = self.worktree.take() else {
            return;
        };
        strip.menu = None;
        strip.menu_dismissed = None;
        if strip.busy.is_some() || strip.error.is_some() || strip.note.is_some() || strip.conflicted
        {
            self.parked_worktrees.insert(strip.key.0.clone(), strip);
        }
    }

    /// Task `id`'s strip, whether its task is open or was set aside.
    fn strip_for(&mut self, id: &str) -> Option<&mut WorktreeStrip> {
        match self.worktree.as_mut() {
            Some(strip) if strip.key.0 == id => Some(strip),
            _ => self.parked_worktrees.get_mut(id),
        }
    }

    /// Reads the open task's worktree again, whatever was read before.
    pub(crate) fn reload_worktree(&mut self, cx: &mut Context<Self>) {
        if let Some(strip) = self.worktree.as_mut() {
            strip.key.1 = usize::MAX;
        }
        self.load_worktree(cx);
    }

    /// Runs `step` on the open task's worktree off the main thread, then reads it again.
    fn worktree_step(&mut self, step: Step, cx: &mut Context<Self>) {
        if let Some(id) = self.worktree.as_ref().map(|s| s.key.0.clone()) {
            self.worktree_step_on(&id, step, cx);
        }
    }

    /// Runs `step` on task `id`'s worktree, while its strip is still around.
    fn worktree_step_on(&mut self, id: &str, step: Step, cx: &mut Context<Self>) {
        let Some(strip) = self.strip_for(id) else {
            return;
        };
        if strip.busy.is_some() {
            return;
        }
        strip.busy = Some(step.busy());
        strip.error = None;
        strip.note = None;
        strip.retry = None;
        let id = strip.key.0.clone();
        let core = Arc::clone(&self.core);
        let (waiting, waited) = oneshot::channel::<()>();
        let task = {
            let id = id.clone();
            cx.background_spawn(async move {
                step.run(&core, &id, move || {
                    let _ = waiting.send(());
                })
            })
        };
        // A merge held up by another says so until it gets its turn.
        let held_id = id.clone();
        cx.spawn(async move |this, cx| {
            if waited.await.is_err() {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                if let Some(strip) = this.strip_for(&held_id)
                    && strip.busy.is_some()
                {
                    strip.busy = Some("Waiting for another merge…");
                    cx.notify();
                }
            });
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let done = task.await;
            this.update(cx, |this, cx| {
                if let Some(strip) = this.strip_for(&id) {
                    strip.busy = None;
                    match done {
                        Ok(note) => {
                            strip.note = note;
                            strip.conflicted = false;
                        }
                        Err(e) => {
                            strip.retry = e.is_timed_out().then_some(step);
                            let e = e.to_string();
                            strip.conflicted = e.starts_with("Merging would conflict")
                                || e.starts_with("Updating would conflict");
                            strip.error = Some(e);
                        }
                    }
                }
                this.reload_worktree(cx);
                this.reload_worktree_marks(cx);
                this.load_git(cx);
            })
        })
        .detach();
        cx.notify();
    }

    fn discard_worktree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(strip) = self.worktree.as_ref() else {
            return;
        };
        let Some(state) = strip.state.as_ref() else {
            return;
        };
        // The answer may come after another task opened; it discards this one's or nothing.
        let id = strip.key.0.clone();
        let detail = discard_detail(state);
        let answer = window.prompt(
            PromptLevel::Warning,
            "Discard this task's changes?",
            Some(&detail),
            &[PromptButton::ok("Discard"), PromptButton::cancel("Keep")],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let _ = this.update(cx, |this, cx| this.worktree_step_on(&id, Step::Discard, cx));
        })
        .detach();
    }

    /// Runs the step that ran out of time again.
    fn retry_worktree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.worktree.as_ref().and_then(|s| s.retry) {
            Some(Step::Discard) => self.discard_worktree(window, cx),
            Some(step) => self.worktree_step(step, cx),
            None => {}
        }
    }

    /// Opens or closes the worktree's menu: copy its path, open a terminal or the file manager
    /// there.
    fn toggle_worktree_menu(
        &mut self,
        e: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(strip) = self.worktree.as_mut() else {
            return;
        };
        let closing_press = matches!(e, ClickEvent::Mouse(click)
            if strip.menu_dismissed.take() == Some(click.down.position));
        if closing_press || strip.menu.take().is_some() {
            cx.notify();
            return;
        }
        let Some(root) = strip.state.as_ref().map(|s| s.worktree.root.clone()) else {
            return;
        };
        let this = cx.entity().downgrade();
        let mut items = vec![{
            let root = root.clone();
            MenuItem::new("Copy path", move |_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(root.clone()));
            })
            .icon("copy")
        }];
        if !is_remote(&root) {
            let folder = root.clone();
            items.push(
                MenuItem::new("Open terminal here", move |window, cx| {
                    let _ = this.update(cx, |w, cx| w.new_terminal_in(root.clone(), window, cx));
                })
                .icon("terminal"),
            );
            items.push(
                MenuItem::new("Open in file manager", move |_, cx| {
                    cx.open_with_system(std::path::Path::new(&folder));
                })
                .icon("folder-open"),
            );
        }
        let menu = Menu::open("Worktree", items, window, cx);
        cx.subscribe_in(&menu, window, |this, _, e: &Dismissed, window, cx| {
            if let Some(strip) = this.worktree.as_mut() {
                strip.menu_dismissed = e.0;
                strip.menu = None;
            }
            window.focus(&this.focus, cx);
            cx.notify();
        })
        .detach();
        strip.menu = Some((menu, e.position()));
        cx.notify();
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

/// What discarding drops: "3 changed files go back to how the project has them, and 2 unmerged
/// commits are dropped."
fn discard_detail(state: &WorktreeState) -> String {
    let files = match state.changes.len() {
        0 => None,
        1 => Some("1 changed file goes back to how the project has it".to_owned()),
        n => Some(format!(
            "{n} changed files go back to how the project has them"
        )),
    };
    let commits = match state.commits {
        0 => None,
        1 => Some("1 unmerged commit is dropped".to_owned()),
        n => Some(format!("{n} unmerged commits are dropped")),
    };
    let parts: Vec<String> = files.into_iter().chain(commits).collect();
    let shared = match state.shared_with.as_slice() {
        [] => String::new(),
        [one] => format!(" \"{one}\" works in this worktree too and loses its changes as well."),
        many => format!(
            " {} other tasks work in this worktree too and lose their changes as well: {}.",
            many.len(),
            many.iter()
                .map(|t| format!("\"{t}\""))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    format!("{}.{shared}", parts.join(", and "))
}

/// "3 files changed · 1 commit · 2 behind".
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
    if state.behind > 0 {
        parts.push(format!("{} behind", state.behind));
    }
    parts.join(" · ")
}

/// The status of a task that was to get a worktree and edits the project's own folder instead.
fn no_worktree(error: &str, t: &Theme) -> Stateful<gpui::Div> {
    let text: SharedString = format!("No worktree, so this task edits the project: {error}").into();
    div()
        .id("worktree")
        .role(Role::Alert)
        .aria_label(text.clone())
        .flex()
        .min_w_0()
        .items_center()
        .gap(px(6.))
        .text_color(t.danger)
        .tooltip(tooltip(text.clone()))
        .child(icon("warning", 13.).flex_none())
        .child(div().min_w_0().truncate().child(text))
}

/// The open task's worktree, when it works in one and there is something to say about it: its
/// status for the composer's controls, before Format, and its changed files when that status is
/// opened, which go above the composer.
pub fn worktree_strip(
    w: &Workspace,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> (Option<Stateful<gpui::Div>>, Option<Stateful<gpui::Div>>) {
    let working = w.chat.as_ref().is_some_and(is_working);
    let Some(strip) = w.worktree.as_ref() else {
        return (None, None);
    };
    if strip.state.is_none()
        && let Some(failed) = &strip.failed
    {
        return (Some(no_worktree(failed, t)), None);
    }
    let Some(state) = strip.state.clone() else {
        return (None, None);
    };
    let pending = !state.changes.is_empty() || state.commits > 0;
    let off_branch = state.off_branch.clone();
    let landed_on = state.landed_on.clone();
    let idle = strip.busy.is_none() && !working;
    let (open, conflicted, retry) = (strip.open, strip.conflicted, strip.retry.is_some());
    let menu = strip.menu.clone();
    let message = strip
        .error
        .clone()
        .map(|e| (e, true))
        .or_else(|| {
            off_branch.clone().filter(|_| idle).map(|on| {
                let branch = &state.worktree.branch;
                (format!("On {on} instead of {branch}"), true)
            })
        })
        .or_else(|| strip.note.clone().map(|n| (n, false)))
        .or_else(|| {
            landed_on
                .clone()
                .filter(|_| idle)
                .map(|on| (format!("{on} already has these changes"), false))
        });
    let retry = idle && retry && strip.error.is_some();
    // Nothing to merge or bring in, nothing running and nothing to report: the line stays blank.
    if !pending && state.behind == 0 && strip.busy.is_none() && message.is_none() && !conflicted {
        return (None, None);
    }
    let (muted, fg) = (t.text_muted, t.text);
    // Opening it shows the changed files and the Discard and Merge buttons.
    let can_open = pending;
    let branch: SharedString = state.worktree.branch.clone().into();
    // While only a note or an error shows, it still says the task works in its own worktree.
    let label: SharedString = strip
        .busy
        .map(str::to_owned)
        .or_else(|| (pending || state.behind > 0).then(|| summary(&state)))
        .unwrap_or_else(|| "Worktree".to_owned())
        .into();
    let toggle = button("worktree-toggle", label.clone())
        .when(can_open, |b| b.aria_expanded(open))
        .flex_none()
        .gap(px(6.))
        .text_color(muted)
        .hover(move |s| s.text_color(fg))
        .tooltip(tooltip(format!("Working in a worktree on {branch}")))
        .child(icon("folders", 13.).flex_none().text_color(t.accent_text))
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
        }));
    let more = icon_button("worktree-menu", "Worktree actions", "dots-three", 14., cx)
        .flex_none()
        .aria_expanded(menu.is_some())
        .tooltip(tooltip("Worktree actions"))
        .on_click(cx.listener(Workspace::toggle_worktree_menu));
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
        .min_w_0()
        .items_center()
        .gap(px(8.))
        .child(toggle)
        .child(more)
        .when(idle && conflicted, |d| {
            d.child(
                strip_action("worktree-resolve", "Ask to resolve", t)
                    .tooltip(tooltip("Have the task bring in the project's changes"))
                    .on_click(cx.listener(|w, _, _, cx| w.resolve_worktree(cx))),
            )
        })
        .when(idle && off_branch.is_some(), |d| {
            d.child(
                strip_action("worktree-switch-back", "Switch back", t)
                    .tooltip(tooltip(format!(
                        "Check out {} again",
                        state.worktree.branch
                    )))
                    .on_click(cx.listener(|w, _, _, cx| w.worktree_step(Step::SwitchBack, cx))),
            )
        })
        .when(idle && landed_on.is_some(), |d| {
            d.child(
                strip_action("worktree-clean-up", "Clean up", t)
                    .tooltip(tooltip(
                        "Reset the worktree to the branch that has its changes",
                    ))
                    .on_click(cx.listener(|w, _, _, cx| w.worktree_step(Step::CleanUp, cx))),
            )
        })
        .when(idle && state.behind > 0 && off_branch.is_none(), |d| {
            d.child(
                strip_action("worktree-update", "Update", t)
                    .tooltip(tooltip("Bring in the project's new commits"))
                    .on_click(cx.listener(|w, _, _, cx| w.worktree_step(Step::Update, cx))),
            )
        })
        .children(message)
        .when(retry, |d| {
            d.child(
                strip_action("worktree-retry", "Retry", t)
                    .on_click(cx.listener(|w, _, window, cx| w.retry_worktree(window, cx))),
            )
        })
        .children(menu.map(|(menu, at)| Menu::view(&menu, at)));
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
    let actions = (idle && off_branch.is_none()).then(|| {
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
                    .on_click(cx.listener(|w, _, _, cx| w.worktree_step(Step::Merge, cx))),
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
