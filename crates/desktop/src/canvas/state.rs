//! The canvas's state: the open file, its tabs, and what is loaded for it.

// Rust guideline compliant 2026-10-09

use gpui::{
    Entity, FocusHandle, ScrollHandle, SharedString, UniformListScrollHandle, WeakEntity, Window,
    prelude::*,
};
use patchwork_server::threads::{Message, MessageStatus, TurnTrees};
use patchwork_server::workspace::{FileMedia, PreviewKind};
use crate::ui::text_area::{Submitted, TextArea};
use crate::ui::highlight::{MAX_HIGHLIGHT_LINES, Run, highlight_file, split_lines};
use crate::ui::theme::theme;
use crate::workspace::Workspace;
use super::diff;

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
pub(super) type Loaded<T> = Option<Result<T, String>>;

pub struct Canvas {
    pub(super) workspace: WeakEntity<Workspace>,
    pub project_id: String,
    pub(super) root: String,
    pub path: Option<String>,
    /// The reply it was opened from, as it was then; the open chat's copy wins while it has one.
    pub(super) message: Option<Message>,
    pub(super) tab: CanvasTab,
    pub(super) file: Loaded<SharedString>,
    /// The file's lines as shown, tabs widened to two spaces, their colours, and the widest one.
    pub(super) lines: Vec<SharedString>,
    pub(super) runs: Vec<Vec<Run>>,
    pub(super) widest: Option<usize>,
    pub(super) diff: Loaded<SharedString>,
    pub(super) staged: Vec<bool>,
    pub(super) media: Option<FileMedia>,
    pub(super) load_seq: usize,
    pub(super) busy: bool,
    pub(super) action_error: Option<String>,
    pub(super) copied: bool,
    pub(super) copied_seq: usize,
    pub(super) was_streaming: bool,
    pub(super) body: ScrollHandle,
    pub(super) code_scroll: UniformListScrollHandle,
    /// The rows of the diff that start a change, by their place among the body's children.
    pub(super) hunk_children: Vec<usize>,
    pub(super) draft: Option<diff::Draft>,
    pub(super) editing: Option<usize>,
    pub(super) note_input: Entity<TextArea>,
    pub(super) focus: FocusHandle,
}

impl Canvas {
    #[expect(
        clippy::too_many_arguments,
        reason = "each argument is a separate piece of what it draws"
    )]
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
            this.save_note(window, cx);
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
            widest: None,
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
    pub(super) fn live_message(&self, cx: &Context<Self>) -> Option<Message> {
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
    pub(super) fn turn(&self, cx: &Context<Self>) -> Option<TurnTrees> {
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
            let _ = this.update(cx, Canvas::load_now);
        })
        .detach();
    }

    /// Reads the file, its diff, which of its changes are staged, and whether it is an image or a
    /// video, off the main thread.
    pub(super) fn load_now(&mut self, cx: &mut Context<Self>) {
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
                None => ws::git_file_diff(&root, &path),
            }
            .map_err(|e| e.to_string());
            let staged = ws::staged_hunks(&root, &path, turn.as_ref()).unwrap_or_default();
            let media = ws::file_media(&root, std::slice::from_ref(&path))
                .ok()
                .and_then(|m| m.into_iter().find(|m| m.path == path));
            let lines: Vec<String> = file.as_deref().map_or_else(
                |_| Vec::new(),
                |text| {
                    split_lines(text)
                        .into_iter()
                        .map(|l| l.replace('\t', "  "))
                        .collect()
                },
            );
            let runs = if lines.len() <= MAX_HIGHLIGHT_LINES {
                let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
                highlight_file(&path, &refs)
            } else {
                Vec::new()
            };
            let widest = lines
                .iter()
                .enumerate()
                .max_by_key(|(_, l)| l.chars().count())
                .map(|(i, _)| i);
            let lines: Vec<SharedString> = lines.into_iter().map(SharedString::from).collect();
            (file, diff, staged, media, lines, runs, widest)
        });
        cx.spawn(async move |this, cx| {
            let (file, diff, staged, media, lines, runs, widest) = task.await;
            this.update(cx, |this, cx| {
                if this.load_seq != seq {
                    return;
                }
                this.lines = lines;
                this.runs = runs;
                this.widest = widest;
                this.file = Some(file.map(SharedString::from));
                this.diff = Some(diff.map(SharedString::from));
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
    pub(super) fn preview(&self) -> Option<CanvasTab> {
        match self.media.as_ref()?.kind {
            PreviewKind::Image => Some(CanvasTab::Image),
            PreviewKind::Video => Some(CanvasTab::Video),
            _ => None,
        }
    }

    pub(super) fn active_tab(&self) -> CanvasTab {
        match (self.path.is_some(), self.preview()) {
            (false, _) => CanvasTab::Plan,
            (true, Some(preview)) if self.tab != CanvasTab::Plan => preview,
            _ => self.tab,
        }
    }

    pub(super) fn close(&mut self, cx: &mut Context<Self>) {
        if let Some(w) = self.workspace.upgrade() {
            w.update(cx, crate::workspace::Workspace::close_canvas);
        }
    }
}
