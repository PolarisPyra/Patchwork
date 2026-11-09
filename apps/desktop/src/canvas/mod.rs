//! The file panel right of the chat (`features/canvas/Canvas.tsx` and
//! `workspace/components/CanvasArea.tsx`): a file's code, its diff, or a reply's plan, with the
//! file's name, its actions, and its staged state.

mod diff;

use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    AnyElement, ClipboardItem, Context, Entity, FocusHandle, FontWeight, Hsla, IntoElement,
    KeyDownEvent, ListHorizontalSizingBehavior, ObjectFit, PromptLevel, Render, Role, ScrollHandle,
    SharedString, StyledText, UniformListScrollHandle, WeakEntity, Window, div, img, prelude::*,
    px, relative, uniform_list,
};
use patchwork_server::threads::{Message, MessageStatus, StepStatus, TurnTrees};
use patchwork_server::workspace::{FileMedia, HunkAction, PreviewKind};

pub use diff::{Review, review_message};

use crate::chat::text_area::{Submitted, TextArea};
use crate::highlight::{MAX_HIGHLIGHT_LINES, Run, describe_file, highlight_lines, split_lines};
use crate::theme::{FONT_MONO, FS_LG, FS_MD, FS_SM, RADIUS, Theme, theme};
use crate::ui::{BtnKind, badge, base_name, btn, dir_name, icon, icon_button, spinner, tooltip};
use crate::workspace::Workspace;

/// The panel's narrowest width, and the share of the row it may take at most.
pub const CANVAS_MIN: f32 = 340.;
pub const CANVAS_MAX_FRACTION: f32 = 0.7;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CanvasTab {
    Plan,
    Code,
    Diff,
    Image,
    Video,
}

/// A load's result: the value, or the error it hit.
type Loaded<T> = Option<Result<T, String>>;

pub struct Canvas {
    workspace: WeakEntity<Workspace>,
    pub project_id: String,
    root: String,
    pub path: Option<String>,
    /// The reply it was opened from, as it was then; the open chat's copy wins while it has one.
    message: Option<Message>,
    tab: CanvasTab,
    file: Loaded<String>,
    /// The file's lines as shown, tabs widened to two spaces, and their colours.
    lines: Vec<SharedString>,
    runs: Vec<Vec<Run>>,
    /// The palette the runs were coloured for.
    runs_theme: Option<Hsla>,
    diff: Loaded<String>,
    staged: Vec<bool>,
    media: Option<FileMedia>,
    load_seq: usize,
    busy: bool,
    action_error: Option<String>,
    copied: bool,
    copied_seq: usize,
    was_streaming: bool,
    body: ScrollHandle,
    code_scroll: UniformListScrollHandle,
    /// The rows of the diff that start a change, by their place among the body's children.
    hunk_children: Vec<usize>,
    draft: Option<diff::Draft>,
    editing: Option<usize>,
    note_input: Entity<TextArea>,
    focus: FocusHandle,
}

impl Canvas {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        workspace: WeakEntity<Workspace>,
        project_id: String,
        root: String,
        path: Option<String>,
        message: Option<Message>,
        tab: CanvasTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let t = *theme(cx);
        let note_input = cx.new(|cx| {
            let mut area = TextArea::new("", "Note", cx);
            area.text_color = t.text;
            area.placeholder_color = t.text_subtle;
            area.selection_color = t.accent.opacity(0.3);
            area.enter_newline = true;
            area
        });
        cx.subscribe_in(&note_input, window, |this, _, _: &Submitted, window, cx| {
            this.save_note(window, cx)
        })
        .detach();
        let was_streaming = message
            .as_ref()
            .is_some_and(|m| m.status == MessageStatus::Streaming);
        let mut this = Self {
            workspace,
            project_id,
            root,
            path,
            message,
            tab,
            file: None,
            lines: Vec::new(),
            runs: Vec::new(),
            runs_theme: None,
            diff: None,
            staged: Vec::new(),
            media: None,
            load_seq: 0,
            busy: false,
            action_error: None,
            copied: false,
            copied_seq: 0,
            was_streaming,
            body: ScrollHandle::new(),
            code_scroll: UniformListScrollHandle::new(),
            hunk_children: Vec::new(),
            draft: None,
            editing: None,
            note_input,
            focus: cx.focus_handle(),
        };
        this.load(cx);
        this
    }

    /// The reply as it is now.
    fn live_message(&self, cx: &Context<Self>) -> Option<Message> {
        let id = &self.message.as_ref()?.id;
        let live = self.workspace.upgrade().and_then(|w| {
            w.read(cx)
                .thread
                .as_ref()
                .and_then(|t| t.messages.iter().find(|m| &m.id == id).cloned())
        });
        live.or_else(|| self.message.clone())
    }

    /// A file the reply changed shows what its turn did, which stays after a commit.
    fn turn(&self, cx: &Context<Self>) -> Option<TurnTrees> {
        let path = self.path.as_ref()?;
        let message = self.live_message(cx)?;
        message
            .files
            .iter()
            .any(|f| f == path)
            .then_some(message.turn)
            .flatten()
    }

    /// The workspace opens and updates the canvas while it is itself being updated, and a load
    /// reads it, so the load waits for that update to end.
    pub fn load(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let _ = this.update(cx, |this, cx| this.load_now(cx));
        })
        .detach();
    }

    /// Reads the file, its diff, which of its changes are staged, and whether it is an image or a
    /// video, off the main thread.
    fn load_now(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        self.load_seq += 1;
        let seq = self.load_seq;
        let (root, turn) = (self.root.clone(), self.turn(cx));
        let task = cx.background_spawn(async move {
            use patchwork_server::orchestration::changes::checkpoint::turn_file_diff;
            use patchwork_server::workspace as ws;
            let file = ws::read_file(&root, &path).map_err(|e| e.to_string());
            let diff = match &turn {
                Some(turn) => turn_file_diff(&root, turn, &path),
                None => ws::git_file_diff(&root, &path).map_err(|e| e.to_string()),
            };
            let staged = ws::staged_hunks(&root, &path, turn.as_ref()).unwrap_or_default();
            let media = ws::file_media(&root, std::slice::from_ref(&path))
                .ok()
                .and_then(|m| m.into_iter().find(|m| m.path == path));
            (file, diff, staged, media)
        });
        cx.spawn(async move |this, cx| {
            let (file, diff, staged, media) = task.await;
            this.update(cx, |this, cx| {
                if this.load_seq != seq {
                    return;
                }
                this.lines = match &file {
                    Ok(text) => split_lines(text)
                        .into_iter()
                        .map(|l| SharedString::from(l.replace('\t', "  ")))
                        .collect(),
                    Err(_) => Vec::new(),
                };
                this.runs_theme = None;
                this.file = Some(file);
                this.diff = Some(diff);
                this.staged = staged;
                this.media = media;
                cx.notify();
            })
        })
        .detach();
    }

    /// Reloads when a settled burst of changes touched the file, or `.git`, since staging and
    /// commits change its diff.
    pub fn on_paths(&mut self, paths: &[String], cx: &mut Context<Self>) {
        let Some(path) = &self.path else {
            return;
        };
        if paths.iter().any(|p| p == path || p.starts_with(".git/")) {
            self.load(cx);
        }
    }

    /// The file is an image or a video, shown in place of its code and diff.
    fn preview(&self) -> Option<CanvasTab> {
        match self.media.as_ref()?.kind {
            PreviewKind::Image => Some(CanvasTab::Image),
            PreviewKind::Video => Some(CanvasTab::Video),
            _ => None,
        }
    }

    fn active_tab(&self) -> CanvasTab {
        match (self.path.is_some(), self.preview()) {
            (false, _) => CanvasTab::Plan,
            (true, Some(preview)) if self.tab != CanvasTab::Plan => preview,
            _ => self.tab,
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        if let Some(w) = self.workspace.upgrade() {
            w.update(cx, |w, cx| w.close_canvas(cx));
        }
    }

    /// Runs a git action on the file, then refreshes git's state and the panel.
    fn run_action(
        &mut self,
        action: impl FnOnce(&str) -> Result<(), String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.busy = true;
        self.action_error = None;
        let root = self.root.clone();
        let task = cx.background_spawn(async move { action(&root) });
        cx.spawn(async move |this, cx| {
            let done = task.await;
            this.update(cx, |this, cx| {
                this.busy = false;
                if let Err(e) = done {
                    this.action_error = Some(e);
                }
                if let Some(w) = this.workspace.upgrade() {
                    w.update(cx, |w, cx| w.load_git(cx));
                }
                this.load(cx);
                cx.notify();
            })
        })
        .detach();
        cx.notify();
    }

    fn stage_file(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        self.run_action(
            move |root| {
                patchwork_server::workspace::accept_file(root, &path).map_err(|e| e.to_string())
            },
            cx,
        );
    }

    fn reject(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            "Discard",
            Some(&format!("Discard changes to {}?", base_name(&path))),
            &["Discard", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.run_action(
                    move |root| {
                        patchwork_server::workspace::revert_file(root, &path)
                            .map_err(|e| e.to_string())
                    },
                    cx,
                )
            });
        })
        .detach();
    }

    /// Stages, unstages, or discards one change, asking before a discard.
    fn act_on_hunk(
        &mut self,
        hunk: String,
        action: HunkAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let turn = self.turn(cx);
        let run = move |this: &mut Self, cx: &mut Context<Self>| {
            this.run_action(
                move |root| {
                    patchwork_server::workspace::apply_hunk(
                        root,
                        &path,
                        turn.as_ref(),
                        &hunk,
                        action,
                    )
                    .map_err(|e| e.to_string())
                },
                cx,
            )
        };
        if !matches!(action, HunkAction::Discard) {
            return run(self, cx);
        }
        let name = self.path.as_deref().map(base_name).unwrap_or_default();
        let answer = window.prompt(
            PromptLevel::Warning,
            "Discard",
            Some(&format!("Discard this change to {name}?")),
            &["Discard", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await == Ok(0) {
                let _ = this.update(cx, run);
            }
        })
        .detach();
    }

    fn open_in_editor(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        self.action_error = None;
        let root = self.root.clone();
        let task = cx.background_spawn(async move {
            patchwork_server::workspace::open_in_editor(&root, &path, None)
        });
        cx.spawn(async move |this, cx| {
            if let Err(e) = task.await {
                let _ = this.update(cx, |this, cx| {
                    this.action_error = Some(e.to_string());
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn copy(&mut self, cx: &mut Context<Self>) {
        let Some(Ok(content)) = &self.file else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(content.clone()));
        self.copied = true;
        self.copied_seq += 1;
        let seq = self.copied_seq;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.copied_seq == seq {
                    this.copied = false;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// Scrolls the next or previous change to the top of the view.
    fn jump_to_change(&mut self, step: isize, cx: &mut Context<Self>) {
        let top = self.body.bounds().top();
        let offsets: Vec<f32> = self
            .hunk_children
            .iter()
            .filter_map(|&i| self.body.bounds_for_item(i))
            .map(|b| f32::from(b.top() - top))
            .collect();
        let target = if step > 0 {
            offsets.iter().copied().find(|&o| o > 1.)
        } else {
            offsets.iter().rev().copied().find(|&o| o < -1.)
        };
        if let Some(by) = target {
            let mut offset = self.body.offset();
            offset.y -= px(by);
            self.body.set_offset(offset);
            cx.notify();
        }
    }

    fn send_review(&mut self, cx: &mut Context<Self>) {
        let notes = cx
            .try_global::<Review>()
            .map(|r| r.notes(&self.project_id).to_vec())
            .unwrap_or_default();
        if notes.is_empty() {
            return;
        }
        let text = review_message(&notes);
        cx.default_global::<Review>().clear(&self.project_id);
        let project = self.project_id.clone();
        if let Some(w) = self.workspace.upgrade() {
            w.update(cx, |w, cx| w.send_review(&project, text, cx));
        }
        cx.notify();
    }

    fn header(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let name = self
            .path
            .as_deref()
            .map(base_name)
            .unwrap_or("No file selected")
            .to_string();
        let subtitle = self.path.as_deref().map(|path| {
            let dir = dir_name(path);
            let mut parts: Vec<String> = Vec::new();
            if !dir.is_empty() {
                parts.push(dir.to_string());
            }
            parts.push(describe_file(path).label);
            if let Some(Ok(_)) = &self.file {
                parts.push(format!("{} lines", self.lines.len()));
            }
            parts.join(" · ")
        });
        let review = cx
            .try_global::<Review>()
            .map_or(0, |r| r.notes(&self.project_id).len());
        let has_content = matches!(self.file, Some(Ok(_)));
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .flex_none()
            .py(px(14.))
            .px(px(20.))
            .border_b_1()
            .border_color(t.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_none()
                    .size(px(36.))
                    .rounded(px(8.))
                    .bg(t.bg_accent_soft)
                    .text_color(t.accent_text)
                    .child(icon("code", 18.)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .id("canvas-name")
                            .text_size(px(FS_LG))
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .when_some(self.path.clone(), |d, path| d.tooltip(tooltip(path)))
                            .child(name),
                    )
                    .children(subtitle.map(|s| {
                        div()
                            .truncate()
                            .text_size(px(FS_SM))
                            .text_color(t.text_muted)
                            .child(s)
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .flex_none()
                    .when(review > 0, |d| {
                        d.child(
                            btn(
                                "send-review",
                                format!("Send review ({review})"),
                                BtnKind::Primary,
                                false,
                                t,
                            )
                            .child(icon("chat-text", 14.))
                            .child(format!("Send review ({review})"))
                            .on_click(cx.listener(|this, _, _, cx| this.send_review(cx))),
                        )
                    })
                    .when(self.path.is_some(), |d| {
                        d.child(
                            btn("open-in-editor", "Open in editor", BtnKind::Plain, false, t)
                                .child(icon("arrow-square-out", 14.))
                                .child("Open in editor")
                                .on_click(cx.listener(|this, _, _, cx| this.open_in_editor(cx))),
                        )
                        .child({
                            let label = if self.copied { "Copied" } else { "Copy" };
                            btn("copy-file", label, BtnKind::Plain, !has_content, t)
                                .child(icon(if self.copied { "check" } else { "copy" }, 14.))
                                .child(label)
                                .when(has_content, |b| {
                                    b.on_click(cx.listener(|this, _, _, cx| this.copy(cx)))
                                })
                        })
                    })
                    .child(
                        icon_button("close-canvas", "Close canvas", "x", 16., cx)
                            .tooltip(tooltip("Close"))
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    ),
            )
    }

    fn tabs(
        &self,
        message: Option<&Message>,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let active = self.active_tab();
        let plan = message.map_or(0, |m| m.plan.len());
        let change = self.change(cx);
        let diff_count = if self.turn(cx).is_some() {
            match &self.diff {
                Some(Ok(diff)) => diff
                    .lines()
                    .filter(|l| {
                        (l.starts_with('+') || l.starts_with('-'))
                            && !l.starts_with("+++ ")
                            && !l.starts_with("--- ")
                    })
                    .count(),
                _ => 0,
            }
        } else {
            change.map_or(0, |c| (c.additions + c.deletions) as usize)
        };
        let tabs: Vec<(CanvasTab, &str, usize)> = match (self.path.is_some(), self.preview()) {
            (false, _) => vec![(CanvasTab::Plan, "Plan", plan)],
            (true, Some(preview)) => vec![
                (CanvasTab::Plan, "Plan", plan),
                (
                    preview,
                    if preview == CanvasTab::Image {
                        "Image"
                    } else {
                        "Video"
                    },
                    0,
                ),
            ],
            (true, None) => vec![
                (CanvasTab::Plan, "Plan", plan),
                (CanvasTab::Code, "Code", 0),
                (CanvasTab::Diff, "Diff", diff_count),
            ],
        };
        let (fg, muted, selected) = (t.text, t.text_muted, t.bg_selected_muted);
        div()
            .id("canvas-tabs")
            .role(Role::TabList)
            .aria_label("File views")
            .flex()
            .items_center()
            .gap(px(4.))
            .flex_none()
            .h(px(40.))
            .px(px(12.))
            .border_b_1()
            .border_color(t.border)
            .children(tabs.into_iter().map(|(id, label, count)| {
                let on = active == id;
                let aria = if count > 0 {
                    format!("{label} ({count})")
                } else {
                    label.to_string()
                };
                div()
                    .id(SharedString::from(format!("canvas-tab-{label}")))
                    .role(Role::Tab)
                    .aria_label(aria)
                    .aria_selected(on)
                    .tab_index(0)
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(px(28.))
                    .px(px(10.))
                    .rounded(px(RADIUS))
                    .cursor_pointer()
                    .whitespace_nowrap()
                    .text_color(if on { fg } else { muted })
                    .when(on, |d| d.bg(selected))
                    .hover(move |s| s.text_color(fg))
                    .child(label.to_string())
                    .when(count > 0, |d| {
                        let b = badge(count.to_string(), t);
                        d.child(if on {
                            b.bg(t.border_strong).text_color(t.text)
                        } else {
                            b
                        })
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tab = id;
                        cx.notify();
                    }))
            }))
            .when(active == CanvasTab::Diff, |d| {
                d.child(
                    div()
                        .ml_auto()
                        .flex()
                        .flex_none()
                        .gap(px(4.))
                        .child(
                            icon_button("previous-change", "Previous change", "caret-up", 16., cx)
                                .tooltip(tooltip("Previous change"))
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.jump_to_change(-1, cx)),
                                ),
                        )
                        .child(
                            icon_button("next-change", "Next change", "caret-down", 16., cx)
                                .tooltip(tooltip("Next change"))
                                .on_click(cx.listener(|this, _, _, cx| this.jump_to_change(1, cx))),
                        ),
                )
            })
    }

    /// The file's entry in the project's uncommitted changes.
    fn change(&self, cx: &Context<Self>) -> Option<patchwork_server::workspace::FileChange> {
        let path = self.path.as_ref()?;
        let w = self.workspace.upgrade()?;
        w.read(cx).changes.iter().find(|c| &c.path == path).cloned()
    }

    fn body(
        &mut self,
        message: Option<&Message>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = *theme(cx);
        let active = self.active_tab();
        let path = match (&self.path, active) {
            (Some(path), tab) if tab != CanvasTab::Plan => path.clone(),
            _ => return plan_view(message, &t).into_any_element(),
        };
        match active {
            CanvasTab::Image => {
                let Some(media) = &self.media else {
                    return centered(div()).into_any_element();
                };
                return div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(16.))
                    .child(
                        img(PathBuf::from(&media.absolute))
                            .id("canvas-image")
                            .role(Role::Image)
                            .aria_label(base_name(&path).to_string())
                            .max_w_full()
                            .max_h_full()
                            .object_fit(ObjectFit::Contain),
                    )
                    .into_any_element();
            }
            CanvasTab::Video => {
                let Some(media) = &self.media else {
                    return centered(div()).into_any_element();
                };
                let file = PathBuf::from(&media.absolute);
                return centered(div())
                    .flex_col()
                    .gap(px(12.))
                    .text_color(t.text_muted)
                    .child(icon("film-strip", 32.))
                    .child(base_name(&path).to_string())
                    .child(
                        btn("play-video", "Play", BtnKind::Plain, false, &t)
                            .child(icon("play", 14.))
                            .child("Play")
                            .on_click(move |_, _, cx| cx.open_with_system(&file)),
                    )
                    .into_any_element();
            }
            _ => {}
        }
        let result = if active == CanvasTab::Code {
            &self.file
        } else {
            &self.diff
        };
        match result {
            None => centered(div())
                .text_color(t.text_subtle)
                .child(spinner(18., t.text_subtle))
                .into_any_element(),
            Some(Err(e)) => centered(div())
                .id("canvas-error")
                .text_color(t.text_muted)
                .tooltip(tooltip(e.clone()))
                .child(if active == CanvasTab::Code {
                    "This file can't be displayed. It may have been deleted or is binary."
                } else {
                    "The diff for this file could not be loaded."
                })
                .into_any_element(),
            Some(Ok(text)) if active == CanvasTab::Code => {
                if text.is_empty() {
                    return centered(div())
                        .text_color(t.text_muted)
                        .child("This file is empty.")
                        .into_any_element();
                }
                self.code_view(&path, &t, cx).into_any_element()
            }
            Some(Ok(text)) => {
                let text = text.clone();
                self.diff_view(&text, &path, &t, window, cx)
            }
        }
    }

    fn code_view(
        &mut self,
        path: &str,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let grammar = describe_file(path).grammar;
        if self.runs_theme != Some(t.bg_app) {
            self.runs = match grammar {
                Some(g) if self.lines.len() <= MAX_HIGHLIGHT_LINES => {
                    let lines: Vec<&str> = self.lines.iter().map(|l| l.as_ref()).collect();
                    highlight_lines(&lines, g, t)
                }
                _ => Vec::new(),
            };
            self.runs_theme = Some(t.bg_app);
        }
        let widest = self
            .lines
            .iter()
            .enumerate()
            .max_by_key(|(_, l)| l.chars().count())
            .map(|(i, _)| i);
        let (subtle, hover) = (t.text_subtle, crate::ui::mix(t.bg_hover, t.bg_editor, 40.));
        uniform_list(
            "canvas-code",
            self.lines.len(),
            cx.processor(move |this, range: std::ops::Range<usize>, _, _| {
                range
                    .map(|i| {
                        let line = this.lines[i].clone();
                        let runs = this.runs.get(i).cloned().unwrap_or_default();
                        div()
                            .id(i)
                            .flex()
                            .line_height(relative(1.65))
                            .hover(move |s| s.bg(hover))
                            .child(
                                div()
                                    .flex_none()
                                    .min_w(px(48.))
                                    .pr(px(16.))
                                    .flex()
                                    .justify_end()
                                    .text_color(subtle)
                                    .child((i + 1).to_string()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .whitespace_nowrap()
                                    .pr(px(16.))
                                    .child(StyledText::new(line).with_highlights(runs)),
                            )
                    })
                    .collect()
            }),
        )
        .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
        .with_width_from_item(widest)
        .track_scroll(&self.code_scroll)
        .size_full()
        .py(px(12.))
        .font_family(FONT_MONO)
        .text_size(px(FS_MD))
        .line_height(relative(1.65))
        .text_color(t.text)
    }

    fn footer(&self, t: &Theme, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        self.path.as_ref()?;
        let change = self.change(cx)?;
        let busy = self.busy;
        let state = if change.staged {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(icon("check", 14.).text_color(t.success))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.success)
                        .child("Changes applied"),
                )
        } else {
            div().text_color(t.text_muted).child(format!(
                "Unstaged changes · +{} −{}",
                change.additions, change.deletions
            ))
        };
        Some(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .flex_none()
                .py(px(12.))
                .px(px(20.))
                .border_t_1()
                .border_color(t.border)
                .bg(t.bg_panel)
                .child(div().min_w_0().text_size(px(FS_SM)).child(state))
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .gap(px(8.))
                        .child(
                            btn("reject-file", "Reject", BtnKind::Plain, busy, t)
                                .child("Reject")
                                .when(!busy, |b| {
                                    b.on_click(
                                        cx.listener(|this, _, window, cx| this.reject(window, cx)),
                                    )
                                }),
                        )
                        .child(
                            btn("stage-file", "Stage file", BtnKind::Success, busy, t)
                                .child(icon("plus", 14.))
                                .child("Stage file")
                                .when(!busy, |b| {
                                    b.on_click(cx.listener(|this, _, _, cx| this.stage_file(cx)))
                                }),
                        ),
                ),
        )
    }
}

/// A message in the middle of the body.
fn centered(d: gpui::Div) -> gpui::Div {
    d.size_full()
        .min_h(px(120.))
        .flex()
        .items_center()
        .justify_center()
        .p(px(24.))
        .text_center()
}

/// The reply's plan and the files it changed (`PlanView.tsx`).
fn plan_view(message: Option<&Message>, t: &Theme) -> gpui::Div {
    let steps = message.map(|m| m.plan.as_slice()).unwrap_or_default();
    let files = message.map(|m| m.files.as_slice()).unwrap_or_default();
    div()
        .py(px(16.))
        .px(px(20.))
        .child(if steps.is_empty() {
            centered(div())
                .text_color(t.text_muted)
                .child("The agent hasn't shared a plan for this task.")
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .children(steps.iter().map(|step| {
                    let mark = match step.status {
                        StepStatus::Done => icon("check", 14.)
                            .text_color(t.accent_text)
                            .into_any_element(),
                        StepStatus::Running => spinner(14., t.accent_text).into_any_element(),
                        StepStatus::Error => icon("x", 14.).text_color(t.danger).into_any_element(),
                        StepStatus::Pending => icon("circle", 14.)
                            .text_color(t.text_subtle)
                            .into_any_element(),
                    };
                    div()
                        .flex()
                        .items_start()
                        .gap(px(8.))
                        .child(div().mt(px(4.)).flex_none().child(mark))
                        .child(div().min_w_0().child(step.text.clone()))
                }))
        })
        .when(!files.is_empty(), |d| {
            d.child(
                div()
                    .mt(px(24.))
                    .mb(px(8.))
                    .text_size(px(crate::theme::FS_XS))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.text_subtle)
                    .child("FILES CHANGED"),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .children(files.iter().enumerate().map(|(i, file)| {
                        div()
                            .id(i)
                            .py(px(4.))
                            .px(px(8.))
                            .font_family(FONT_MONO)
                            .text_size(px(FS_SM))
                            .truncate()
                            .tooltip(tooltip(file.clone()))
                            .child(file.clone())
                    })),
            )
        })
}

impl Render for Canvas {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let message = self.live_message(cx);
        // The agent may have edited the file while it was streaming.
        let streaming = message
            .as_ref()
            .is_some_and(|m| m.status == MessageStatus::Streaming);
        if self.was_streaming && !streaming {
            self.load(cx);
        }
        self.was_streaming = streaming;
        let header = self.header(&t, cx);
        let tabs = self.tabs(message.as_ref(), &t, cx);
        let body = self.body(message.as_ref(), window, cx);
        let footer = self.footer(&t, cx);
        let scrolls = self.active_tab() != CanvasTab::Code;
        div()
            .id("canvas")
            .role(Role::Complementary)
            .aria_label("File viewer")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                if e.keystroke.key == "escape" && (this.draft.is_some() || this.editing.is_some()) {
                    this.cancel_note(window, cx);
                    cx.stop_propagation();
                }
            }))
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .bg(t.bg_editor)
            .border_l_1()
            .border_color(t.border)
            .child(header)
            .child(tabs)
            .child(
                div()
                    .id("canvas-body")
                    .role(Role::TabPanel)
                    .flex_1()
                    .min_h_0()
                    .when(scrolls, |d| d.overflow_scroll().track_scroll(&self.body))
                    .child(body),
            )
            .when_some(self.action_error.clone(), |d, e| {
                d.child(
                    div()
                        .id("canvas-action-error")
                        .role(Role::Alert)
                        .flex_none()
                        .py(px(8.))
                        .px(px(20.))
                        .border_t_1()
                        .border_color(t.border)
                        .text_color(t.danger)
                        .text_size(px(FS_SM))
                        .child(e),
                )
            })
            .children(footer)
    }
}
