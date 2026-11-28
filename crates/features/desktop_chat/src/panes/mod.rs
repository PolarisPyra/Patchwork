//! Panes: the chat area cut into side-by-side and stacked panes, each with tabs of its own.
//!
//! Each project keeps its own layout. One pane of it has the focus and shows the open chat with
//! its composer; every other pane shows its chat's transcript, kept live as replies arrive. A
//! click in a pane gives it the focus. A tab dragged to a pane's edge splits the pane, and one
//! dragged onto a pane's middle or its tabs moves there.
//!
//! `tree.rs` is the layout's shape, `docking.rs` what a dragged tab does, `commands.rs` what the
//! pane keys do, `draw.rs` the panes drawn, `presets.rs` the ready-made layouts, `named.rs` those
//! the user named, and `saved.rs` the layouts kept across launches.

// Rust guideline compliant 2026-11-27

pub mod commands;
pub mod docking;
pub mod draw;
pub mod named;
pub mod presets;
pub mod saved;
pub mod tree;

use crate::prelude::*;
use patchwork_client::AppState;
use std::collections::HashSet;

use gpui::{App, Context, FollowMode};
use patchwork_client::local::core::chats::Chat;

use crate::transcript::view_state::TranscriptView;

pub use commands::Dir;
pub use docking::{TabDrag, Zone};
pub use draw::panes;
pub use presets::Preset;
pub use tree::{Axis, Node};

/// A pane's number, unique in the window.
pub type PaneId = u64;

/// One pane: its tabs and the chat it shows.
pub struct Pane {
    pub id: PaneId,
    /// Chat ids, in order; a chat has a tab in one pane at most.
    pub tabs: Vec<String>,
    /// The chat the pane shows. The focused pane's is the open chat, once there is one.
    pub shown: Option<String>,
    /// The chat and its transcript while the pane is out of focus. The focused pane has none: the
    /// workspace holds its state.
    pub view: Option<PaneChat>,
}

/// A project's panes and how they are laid out.
pub struct Layout {
    pub root: Node,
    pub panes: Vec<Pane>,
    pub focused: PaneId,
    /// The pane shown alone, filling the chat area, until it is shown with the rest again.
    pub maximized: Option<PaneId>,
}

impl Layout {
    /// A layout of one empty pane.
    fn new(id: PaneId) -> Self {
        Self {
            root: Node::Pane(id),
            panes: vec![Pane {
                id,
                tabs: Vec::new(),
                shown: None,
                view: None,
            }],
            focused: id,
            maximized: None,
        }
    }

    #[must_use]
    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.panes.iter().find(|p| p.id == id)
    }

    pub fn pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        self.panes.iter_mut().find(|p| p.id == id)
    }

    /// The focused pane, which a layout always has.
    pub fn focused_pane(&mut self) -> &mut Pane {
        let id = self.focused;
        let at = self.panes.iter().position(|p| p.id == id).unwrap_or(0);
        &mut self.panes[at]
    }

    /// The pane with a tab for chat `id`.
    #[must_use]
    pub fn pane_of(&self, id: &str) -> Option<PaneId> {
        self.panes
            .iter()
            .find(|p| p.tabs.iter().any(|t| t == id))
            .map(|p| p.id)
    }

    /// Takes pane `id` out of the layout, and the split it leaves with one pane with it.
    pub fn remove(&mut self, id: PaneId) {
        if self.panes.len() < 2 {
            return;
        }
        self.panes.retain(|p| p.id != id);
        self.root.remove(id);
        if self.focused == id {
            self.focused = self.root.first();
        }
        if self.maximized == Some(id) {
            self.maximized = None;
        }
    }
}

/// A chat drawn out of focus, and its transcript's state: what the workspace holds for the open
/// chat, swapped in while the pane draws.
pub struct PaneChat {
    chat: Option<Chat>,
    /// The worktree the chat works in, where its replies' files are read; `None` for its
    /// project's own folder.
    folder: Option<String>,
    transcript: TranscriptView,
}

impl PaneChat {
    /// `chat` with its transcript at the end, not yet laid out.
    fn new(chat: Option<Chat>, cx: &mut App) -> Self {
        let transcript = TranscriptView::new(cx);
        transcript.list.set_follow_mode(FollowMode::Tail);
        Self {
            chat,
            folder: None,
            transcript,
        }
    }

    /// The id of the chat it holds.
    #[must_use]
    pub fn chat_id(&self) -> Option<&str> {
        self.chat.as_ref().map(|c| c.id.as_str())
    }
}

/// The window's panes methods, for any [`ChatWindow`].
pub trait PanesExt: ChatWindow {
    /// The layout of `project`'s panes, made with one empty pane the first time.
    fn layout_mut(&mut self, project: Option<&str>) -> &mut Layout {
        let key = project.map(str::to_owned);
        if !self.layouts.contains_key(&key) {
            let id = self.next_pane();
            self.layouts.insert(key.clone(), Layout::new(id));
        }
        self.layouts
            .get_mut(&key)
            .expect("the layout was just made")
    }

    /// The layout whose panes show: that of the open chat's project, or of the project picked
    /// for a new task.
    fn shown_layout(&mut self, cx: &gpui::App) -> &mut Layout {
        let project = self.tabs_project(cx);
        self.layout_mut(project.as_deref())
    }

    /// A pane number not used before.
    fn next_pane(&mut self) -> PaneId {
        self.pane_seq += 1;
        self.pane_seq
    }

    /// The project of chat `id`, or `None` when it has none or is not in the list.
    fn project_of(&self, id: &str, cx: &gpui::App) -> Option<String> {
        if let Some(chat) = self.chat.as_ref().filter(|c| c.id == id) {
            return chat.project_id.clone();
        }
        AppState::get(cx)
            .chats
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| c.project_id.clone())
    }

    /// Whether chat `id` has a tab in any pane.
    fn has_tab(&self, id: &str) -> bool {
        self.layouts
            .values()
            .any(|l| l.panes.iter().any(|p| p.tabs.iter().any(|t| t == id)))
    }

    /// Swaps the open chat and its transcript's state with `slot`'s.
    fn swap_view(&mut self, slot: &mut PaneChat) {
        std::mem::swap(&mut self.chat, &mut slot.chat);
        std::mem::swap(&mut self.transcript, &mut slot.transcript);
    }

    /// Takes the open chat and its transcript's state out of the workspace, leaving none open.
    fn take_open(&mut self, cx: &mut App) -> Option<PaneChat> {
        self.chat.as_ref()?;
        let mut slot = PaneChat::new(None, cx);
        slot.folder = self.chat_folder();
        self.swap_view(&mut slot);
        Some(slot)
    }

    /// Puts `slot`'s transcript state back under the open chat when it is the same chat, so a pane
    /// given the focus keeps where it was scrolled and what was open.
    fn restore_view(&mut self, mut slot: PaneChat) {
        let same = self.chat.as_ref().map(|c| c.id.as_str()) == slot.chat_id();
        if !same {
            return;
        }
        // The chat just read is newer than the one the pane held.
        slot.chat = self.chat.take();
        self.swap_view(&mut slot);
    }

    /// Before chat `id` shows: when another pane of its project has its tab, that pane takes the
    /// focus, and the pane leaving it keeps the open chat to draw. Returns the state the pane kept
    /// for the chat, for [`Self::restore_view`].
    fn enter_pane_of(&mut self, id: &str, cx: &mut App) -> Option<PaneChat> {
        let project = self.project_of(id, cx);
        let pane = self.layout_mut(project.as_deref()).pane_of(id)?;
        let focused = self.layout_mut(project.as_deref()).focused;
        if pane == focused {
            return None;
        }
        // The open chat stays drawn in its pane only when that pane is in the same layout.
        let open_here = self.chat.as_ref().is_some_and(|c| c.project_id == project);
        let kept = if open_here { self.take_open(cx) } else { None };
        let layout = self.layout_mut(project.as_deref());
        layout.focused_pane().view = kept;
        layout.focused = pane;
        layout.pane_mut(pane).and_then(|p| p.view.take())
    }

    /// Gives pane `id` of the shown layout the focus, opening its chat.
    fn focus_pane(&mut self, id: PaneId, cx: &mut Context<Self>) {
        let layout = self.shown_layout(cx);
        if layout.focused == id {
            return;
        }
        let Some(shown) = layout.pane(id).and_then(|p| p.shown.clone()) else {
            return;
        };
        self.open_chat(&shown, cx);
    }

    /// Reads each pane's chat out of focus from the store again, as the open chat is on a reload.
    fn reload_pane_chats(&mut self) {
        let core = std::sync::Arc::clone(&self.core);
        for layout in self.layouts.values_mut() {
            for pane in &mut layout.panes {
                let Some(view) = pane.view.as_mut() else {
                    continue;
                };
                let Some(id) = view.chat_id().map(str::to_owned) else {
                    continue;
                };
                match core.chat(&id) {
                    Ok(chat) => view.chat = Some(chat),
                    Err(e) => {
                        log::warn!(chat:% = id, error:% = e; "reloading chat {{chat}}: {{error}}");
                    }
                }
            }
        }
    }

    /// The chat `id` held by a pane out of focus, for an agent's event to stream into.
    fn pane_chat_mut(&mut self, id: &str) -> Option<&mut Chat> {
        self.layouts
            .values_mut()
            .flat_map(|l| l.panes.iter_mut())
            .filter_map(|p| p.view.as_mut())
            .filter_map(|v| v.chat.as_mut())
            .find(|c| c.id == id)
    }

    /// Gives each pane of the shown layout out of focus its chat to draw, read from the store
    /// when it has none or holds one whose tab it no longer has, or that left the task list.
    fn load_pane_views(&mut self, cx: &mut App) {
        let core = std::sync::Arc::clone(&self.core);
        // Only a chat still in the task list is read, so a deleted one is not asked for each frame.
        let listed: HashSet<String> = AppState::get(cx)
            .chats
            .iter()
            .filter(|c| c.archived_at.is_none())
            .map(|c| c.id.clone())
            .collect();
        let layout = self.shown_layout(cx);
        let focused = layout.focused;
        for pane in layout.panes.iter_mut().filter(|p| p.id != focused) {
            if pane
                .shown
                .as_ref()
                .is_none_or(|s| !pane.tabs.contains(s) || !listed.contains(s))
            {
                pane.shown = pane.tabs.iter().find(|t| listed.contains(*t)).cloned();
            }
            let Some(shown) = pane.shown.clone() else {
                pane.view = None;
                continue;
            };
            if pane.view.as_ref().and_then(PaneChat::chat_id) == Some(shown.as_str()) {
                continue;
            }
            pane.view = match core.chat(&shown) {
                Ok(chat) => {
                    let mut view = PaneChat::new(Some(chat), cx);
                    view.folder = core.chat_worktree(&shown).map(|w| w.root);
                    Some(view)
                }
                Err(e) => {
                    log::warn!(chat:% = shown, error:% = e; "could not read chat {{chat}}: {{error}}");
                    None
                }
            };
        }
    }

    /// The keyboard handle of the line after part `at` of the split at `path`, made the first
    /// time it is asked for.
    fn line_focus(&mut self, path: &[usize], at: usize, cx: &mut App) -> gpui::FocusHandle {
        let key = (path.to_vec(), at);
        self.line_focus
            .entry(key)
            .or_insert_with(|| cx.focus_handle())
            .clone()
    }

    /// Runs `f` with the chat of pane `id`, out of focus, standing in for the open chat.
    fn in_pane<R>(&mut self, id: PaneId, f: impl FnOnce(&mut Self) -> R) -> Option<R> {
        let mut slot = self.take_pane_view(id)?;
        self.swap_view(&mut slot);
        let out = f(self);
        self.swap_view(&mut slot);
        self.put_pane_view(id, slot);
        Some(out)
    }

    /// The worktree pane `id`'s chat works in, as [`PaneChat::folder`].
    fn pane_folder(&self, id: PaneId) -> Option<String> {
        self.layouts
            .values()
            .find_map(|l| l.pane(id))
            .and_then(|p| p.view.as_ref())
            .and_then(|v| v.folder.clone())
    }

    fn take_pane_view(&mut self, id: PaneId) -> Option<PaneChat> {
        self.layouts
            .values_mut()
            .find_map(|l| l.pane_mut(id))
            .and_then(|p| p.view.take())
    }

    fn put_pane_view(&mut self, id: PaneId, slot: PaneChat) {
        if let Some(pane) = self.layouts.values_mut().find_map(|l| l.pane_mut(id)) {
            pane.view = Some(slot);
        }
    }
}

impl<W: ChatWindow> PanesExt for W {}
