//! The canvas's state: the open file, its tabs, and what is loaded for it.

// Rust guideline compliant 2026-10-09

use gpui::{
    Entity, FocusHandle, ScrollHandle, SharedString, UniformListScrollHandle, WeakEntity, Window,
    prelude::*,
};
use patchwork_core::chats::{Message, MessageStatus, TurnTrees};
use patchwork_core::projects::{FileMedia, PreviewKind};
use patchwork_ui::text::text_area::{Submitted, TextArea};
use patchwork_ui::text::highlight::{MAX_HIGHLIGHT_LINES, Run, highlight_file, split_lines};
use crate::workspace::Workspace;
use super::diff;

/// The panel's narrowest width, and the share of the row it may take at most.
pub const CANVAS_MIN: f32 = 340.;

pub const CANVAS_MAX_FRACTION: f32 = 0.7;

/// How many characters wide a conflict's first line is with its buttons, about.
const CONFLICT_ROW_CHARS: usize = 90;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CanvasTab {
    Plan,
    Code,
    Diff,
    History,
    Image,
    Video,
    Pdf,
}

/// A load's result: the value, or the error it hit.
pub(super) type Loaded<T> = Option<Result<T, String>>;

#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate fact about what the panel shows"
)]
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
    /// Which part of the file's change the diff shows, for a file staged in part.
    pub side: patchwork_core::source_control::DiffSide,
    /// The commit the diff starts from in place of the last one: a task's branch shown against
    /// where it left the branch it goes back into. Such a diff is read, not staged.
    pub since: Option<String>,
    pub(super) media: Option<FileMedia>,
    /// A PDF's pages as images, once drawn.
    pub(super) pdf: Loaded<Vec<std::path::PathBuf>>,
    /// The conflicts git left in the file, and the one last stepped to.
    pub(super) conflicts: Vec<patchwork_core::source_control::Conflict>,
    pub(super) conflict_shown: Option<usize>,
    /// Whether the code shows who last changed each line, and those runs of lines once read.
    pub(super) blame_on: bool,
    pub(super) blame: Loaded<std::rc::Rc<Vec<patchwork_core::source_control::BlameRange>>>,
    /// The commits that changed the file, once its history was opened, and whether more follow.
    pub(super) history: Loaded<Vec<patchwork_core::source_control::FileRevision>>,
    pub(super) history_more: bool,
    /// The commit whose changes the diff shows, in place of the uncommitted ones.
    pub(super) revision: Option<patchwork_core::source_control::FileRevision>,
    pub(super) load_seq: usize,
    pub(super) busy: bool,
    pub(super) action_error: Option<String>,
    pub(super) copied: bool,
    pub(super) copied_seq: usize,
    pub(super) was_streaming: bool,
    pub(super) body: ScrollHandle,
    pub(super) code_scroll: UniformListScrollHandle,
    /// The diff read into rows once it loads, and the list that draws the ones in view.
    pub(super) diff_rows: Option<std::rc::Rc<diff::Parsed>>,
    pub(super) diff_list: gpui::ListState,
    /// What the heights of the diff's lines last depended on: its notes and the open editor.
    pub(super) diff_shape: u64,
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
        let note_input = cx.new(|cx| {
            let mut area = TextArea::new("", "Note", cx);
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
            side: patchwork_core::source_control::DiffSide::All,
            since: None,
            media: None,
            conflicts: Vec::new(),
            conflict_shown: None,
            blame_on: false,
            blame: None,
            history: None,
            history_more: false,
            pdf: None,
            revision: None,
            load_seq: 0,
            busy: false,
            action_error: None,
            copied: false,
            copied_seq: 0,
            was_streaming,
            body: ScrollHandle::new(),
            code_scroll: UniformListScrollHandle::new(),
            diff_rows: None,
            diff_list: gpui::ListState::new(0, gpui::ListAlignment::Top, gpui::px(400.)),
            diff_shape: 0,
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
                .chat
                .as_ref()
                .and_then(|t| t.messages.iter().find(|m| &m.id == id).cloned())
        });
        live.map(|m| m.as_ref().clone())
            .or_else(|| self.message.clone())
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
        let (root, turn, side) = (self.root.clone(), self.turn(cx), self.side);
        let revision = self.revision.clone();
        let since = self.since.clone();
        let task = cx.background_spawn(async move {
            use patchwork_core::orchestration::changes::turn_files::turn_file_diff;
            use patchwork_core::source_control as ws;
            let file = patchwork_core::projects::read_file(&root, &path).map_err(|e| e.to_string());
            // One part of a file staged in part, while it still has that part.
            let part = match (&turn, side) {
                _ if revision.is_some() || since.is_some() => None,
                (Some(_), _) | (None, ws::DiffSide::All) => None,
                (None, side) => ws::git_file_diff(&root, &path, side)
                    .ok()
                    .filter(|diff| !diff.trim().is_empty()),
            };
            let staged = match &part {
                None if revision.is_some() || since.is_some() => Vec::new(),
                Some(diff) => {
                    let changes = diff.lines().filter(|l| l.starts_with("@@")).count();
                    vec![side == ws::DiffSide::Staged; changes]
                }
                None => ws::staged_hunks(&root, &path, turn.as_ref()).unwrap_or_default(),
            };
            let diff = match (&revision, &since, &turn, part) {
                (Some(shown), _, _, _) => ws::commit_file_diff(&root, &shown.hash, &shown.path),
                (None, Some(since), _, _) => ws::diff_since(&root, &path, since),
                (None, None, _, Some(part)) => Ok(part),
                (None, None, Some(turn), None) => turn_file_diff(&root, turn, &path),
                (None, None, None, None) => ws::git_file_diff(&root, &path, ws::DiffSide::All),
            }
            .map_err(|e| e.to_string());
            let media = patchwork_core::projects::file_media(&root, std::slice::from_ref(&path))
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
            let conflicts = file.as_deref().map(ws::conflicts_in).unwrap_or_default();
            let widest = lines
                .iter()
                .enumerate()
                .max_by_key(|(_, l)| l.chars().count())
                // A conflict's first line also holds its buttons, which a file of short lines
                // would otherwise cut off.
                .map(|(i, l)| match conflicts.first() {
                    Some(first) if l.chars().count() < CONFLICT_ROW_CHARS => first.opens,
                    _ => i,
                });
            let lines: Vec<SharedString> = lines.into_iter().map(SharedString::from).collect();
            let pdf = media
                .as_ref()
                .filter(|m| m.kind == PreviewKind::Pdf)
                .map(|m| {
                    patchwork_core::projects::pdf_pages(std::path::Path::new(&m.absolute))
                        .map_err(|e| e.to_string())
                });
            (
                file, diff, staged, media, lines, runs, widest, conflicts, pdf,
            )
        });
        cx.spawn(async move |this, cx| {
            let (file, diff, staged, media, lines, runs, widest, conflicts, pdf) = task.await;
            this.update(cx, |this, cx| {
                if this.load_seq != seq {
                    return;
                }
                this.lines = lines;
                this.runs = runs;
                this.widest = widest;
                this.conflicts = conflicts;
                this.conflict_shown = None;
                this.file = Some(file.map(SharedString::from));
                let diff = diff.map(SharedString::from);
                // A reload keeps the reader's place: the project's watcher reloads on every
                // burst of changes, and starting over at the top each time fought the scroll.
                let reloaded = matches!(&this.diff, Some(Ok(_)));
                if this.diff.as_ref() != Some(&diff) {
                    this.diff_rows = diff
                        .as_ref()
                        .ok()
                        .map(|text| std::rc::Rc::new(diff::parse(text)));
                    let items = this.diff_rows.as_ref().map_or(0, |p| p.rows.len() + 1);
                    let top = this.diff_list.logical_scroll_top();
                    this.diff_list.reset(items);
                    if reloaded && items > 0 {
                        this.diff_list.scroll_to(gpui::ListOffset {
                            item_ix: top.item_ix.min(items - 1),
                            offset_in_item: top.offset_in_item,
                        });
                    }
                }
                this.diff = Some(diff);
                this.staged = staged;
                this.media = media;
                this.pdf = pdf;
                // What was read of the file's past is of the file as it was.
                this.blame = None;
                if this.blame_on {
                    this.load_blame(cx);
                }
                if this.history.is_some() {
                    this.load_history(false, cx);
                }
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

    /// The file is an image, a video or a PDF, shown in place of its code and diff.
    pub(super) fn preview(&self) -> Option<CanvasTab> {
        match self.media.as_ref()?.kind {
            PreviewKind::Image => Some(CanvasTab::Image),
            PreviewKind::Video => Some(CanvasTab::Video),
            PreviewKind::Pdf => Some(CanvasTab::Pdf),
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
        let _ = self
            .workspace
            .update(cx, crate::workspace::Workspace::close_canvas);
    }
}
