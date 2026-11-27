//! What the pane keys do: split, move the focus to a pane by direction or number, step through a
//! pane's tabs, reopen the tab closed last, send the open tab to the next pane over, trade places
//! with the next pane over, and show one pane alone.

// Rust guideline compliant 2026-10-09

use gpui::{AppContext as _, Context};

use super::{Layout, Pane, PaneId, Zone};
use crate::workspace::Workspace;

/// How many closed tabs are remembered for reopening.
const CLOSED_KEPT: usize = 20;

/// A way to go from one pane to the next.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Dir {
    Left,
    Right,
    Up,
    Down,
}

impl Dir {
    /// The edge a tab moved this way docks at when there is no pane there.
    fn zone(self) -> Zone {
        match self {
            Self::Left => Zone::Left,
            Self::Right => Zone::Right,
            Self::Up => Zone::Top,
            Self::Down => Zone::Bottom,
        }
    }
}

impl Layout {
    /// The pane next to pane `from` going `dir`: of those past its edge and beside it, the
    /// nearest, then the one sharing the most of its edge.
    pub(crate) fn neighbour(&self, from: PaneId, dir: Dir) -> Option<PaneId> {
        const EPS: f32 = 1e-4;
        let mut rects = Vec::new();
        self.root.rects(&mut rects);
        let [l, t, r, b] = rects.iter().find(|(id, _)| *id == from)?.1;
        rects
            .iter()
            .filter(|(id, _)| *id != from)
            .filter_map(|(id, [ol, ot, or, ob])| {
                let (gap, overlap) = match dir {
                    Dir::Left => (l - or, t.max(*ot) - b.min(*ob)),
                    Dir::Right => (ol - r, t.max(*ot) - b.min(*ob)),
                    Dir::Up => (t - ob, l.max(*ol) - r.min(*or)),
                    Dir::Down => (ot - b, l.max(*ol) - r.min(*or)),
                };
                // `overlap` is negative while the two share some of their edge.
                (gap > -EPS && overlap < -EPS).then_some((*id, gap, overlap))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.2.total_cmp(&b.2)))
            .map(|(id, _, _)| id)
    }

    /// The panes in reading order.
    pub(crate) fn order(&self) -> Vec<PaneId> {
        let mut ids = Vec::new();
        self.root.pane_ids(&mut ids);
        ids
    }
}

impl Workspace {
    /// Splits the focused pane along `zone`'s edge, as [`Self::split_pane_at`].
    pub(crate) fn split_pane(&mut self, zone: Zone, cx: &mut Context<Self>) {
        let focused = self.shown_layout().focused;
        self.split_pane_at(focused, zone, cx);
    }

    /// Splits pane `target` of the shown layout along `zone`'s edge. For the focused pane, the
    /// open chat moves to the new pane when the focused one has another tab to keep; otherwise
    /// the new pane takes a task with no tab, or stays empty for one to be dragged in.
    pub(crate) fn split_pane_at(&mut self, target: PaneId, zone: Zone, cx: &mut Context<Self>) {
        let focused = self.shown_layout().focused;
        if target == focused
            && let Some(open) = self.chat.as_ref().map(|c| c.id.clone())
            && self.can_split(&open)
        {
            self.open_in_split(&open, zone, cx);
            return;
        }
        let project = self.tabs_project();
        let spare = self
            .untabbed_tasks(project.as_deref())
            .first()
            .map(|c| c.id.clone());
        let new = self.next_pane();
        let layout = self.layout_mut(project.as_deref());
        let Some((axis, before)) = zone.split() else {
            return;
        };
        if layout.pane(target).is_none() {
            return;
        }
        layout.root.split(target, new, axis, before);
        layout.panes.push(Pane {
            id: new,
            shown: spare.clone(),
            tabs: spare.into_iter().collect(),
            view: None,
        });
        layout.maximized = None;
        cx.notify();
    }

    /// Gives the focus to the pane next to the focused one going `dir`.
    pub(crate) fn focus_pane_toward(&mut self, dir: Dir, cx: &mut Context<Self>) {
        let layout = self.shown_layout();
        if let Some(next) = layout.neighbour(layout.focused, dir) {
            self.focus_pane(next, cx);
        }
    }

    /// Gives the focus to pane number `n`, counting from 1 in reading order.
    pub(crate) fn focus_pane_number(&mut self, n: usize, cx: &mut Context<Self>) {
        let order = self.shown_layout().order();
        if let Some(id) = n.checked_sub(1).and_then(|at| order.get(at)) {
            self.focus_pane(*id, cx);
        }
    }

    /// Shows the tab after the open chat's in its pane, or before it when `back`, going round.
    pub(crate) fn step_tab(&mut self, back: bool, cx: &mut Context<Self>) {
        let Some(open) = self.chat.as_ref().map(|c| c.id.clone()) else {
            return;
        };
        let tabs: Vec<String> = {
            let layout = self.shown_layout();
            let focused = layout.focused;
            layout
                .pane(focused)
                .map(|p| p.tabs.clone())
                .unwrap_or_default()
        };
        let tabs: Vec<&String> = tabs.iter().filter(|t| self.listed(t)).collect();
        let Some(at) = tabs.iter().position(|t| **t == open) else {
            return;
        };
        let len = tabs.len();
        let next = if back {
            (at + len - 1) % len
        } else {
            (at + 1) % len
        };
        if next != at {
            let id = tabs[next].clone();
            self.open_chat(&id, cx);
        }
    }

    /// Remembers chat `id`'s tab as closed, for [`Self::reopen_tab`].
    pub(crate) fn remember_closed(&mut self, id: &str) {
        self.closed_tabs.retain(|c| c != id);
        self.closed_tabs.push(id.to_owned());
        if self.closed_tabs.len() > CLOSED_KEPT {
            self.closed_tabs.remove(0);
        }
    }

    /// Opens the tab closed last whose chat is still listed and has no tab again.
    pub(crate) fn reopen_tab(&mut self, cx: &mut Context<Self>) {
        while let Some(id) = self.closed_tabs.pop() {
            if self.listed(&id) && !self.has_tab(&id) {
                self.open_in_new_tab(&id, cx);
                return;
            }
        }
    }

    /// Sends the open chat's tab to the pane next to its own going `dir`, or into a new pane at
    /// that edge when there is none and its pane has another tab to keep.
    pub(crate) fn move_tab_toward(&mut self, dir: Dir, cx: &mut Context<Self>) {
        let Some(open) = self.chat.as_ref().map(|c| c.id.clone()) else {
            return;
        };
        let layout = self.shown_layout();
        let focused = layout.focused;
        match layout.neighbour(focused, dir) {
            Some(next) => self.dock_tab(&open, next, Zone::Middle, cx),
            None => self.dock_tab(&open, focused, dir.zone(), cx),
        }
    }

    /// Trades the places of the focused pane and the pane next to it going `dir`. The focus
    /// goes with the focused pane, and each place keeps its size.
    pub(crate) fn swap_pane_toward(&mut self, dir: Dir, cx: &mut Context<Self>) {
        let layout = self.shown_layout();
        let focused = layout.focused;
        if let Some(next) = layout.neighbour(focused, dir) {
            self.swap_panes(focused, next, cx);
        }
    }

    /// Trades the places of panes `a` and `b` of the shown layout.
    pub(crate) fn swap_panes(&mut self, a: PaneId, b: PaneId, cx: &mut Context<Self>) {
        let layout = self.shown_layout();
        layout.root.swap_panes(a, b);
        layout.maximized = None;
        cx.notify();
    }

    /// Whether chat `id`'s pane has other tabs, and whether it has tabs after `id`'s.
    pub(crate) fn tabs_beside(&self, id: &str) -> (bool, bool) {
        let Some(tabs) = self.pane_tabs_of(id) else {
            return (false, false);
        };
        let at = tabs.iter().position(|t| t == id).unwrap_or(0);
        (tabs.len() > 1, at + 1 < tabs.len())
    }

    /// Closes the tabs beside chat `id`'s in its pane: those after it when `right`, else all.
    pub(crate) fn close_tabs_beside(&mut self, id: &str, right: bool, cx: &mut Context<Self>) {
        let Some(tabs) = self.pane_tabs_of(id) else {
            return;
        };
        let at = tabs.iter().position(|t| t == id).unwrap_or(0);
        // The chat kept shows first, so closing the others never opens one of them on the way.
        if self.chat.as_ref().is_some_and(|c| tabs.contains(&c.id)) {
            self.open_chat(id, cx);
        }
        for (i, tab) in tabs.iter().enumerate() {
            if i != at && (!right || i > at) {
                self.close_tab(tab, cx);
            }
        }
    }

    /// Closes pane `id` of the shown layout with its tabs, unless it is the only pane.
    pub(crate) fn close_pane(&mut self, id: PaneId, cx: &mut Context<Self>) {
        let layout = self.shown_layout();
        if layout.panes.len() < 2 {
            return;
        }
        let Some(tabs) = layout.pane(id).map(|p| p.tabs.clone()) else {
            return;
        };
        if tabs.is_empty() {
            let focused = layout.focused == id;
            layout.remove(id);
            if focused {
                let project = self.tabs_project();
                self.show_focused_pane(project.as_deref(), cx);
            }
            cx.notify();
            return;
        }
        // Its last tab closing takes the pane with it.
        for tab in tabs {
            self.close_tab(&tab, cx);
        }
    }

    /// The tabs of the pane with chat `id`'s tab.
    fn pane_tabs_of(&self, id: &str) -> Option<Vec<String>> {
        self.layouts
            .values()
            .find_map(|l| l.pane_of(id).and_then(|p| l.pane(p)))
            .map(|p| p.tabs.clone())
    }

    /// When messages go to every pane, sends `text`, just sent to chat `from`, to the chat each
    /// other pane of the shown layout shows.
    pub(crate) fn broadcast(
        &mut self,
        from: &str,
        text: &str,
        queue: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.broadcasting {
            return;
        }
        let targets: Vec<String> = self
            .shown_layout()
            .panes
            .iter()
            .filter_map(|p| p.shown.clone())
            .filter(|s| s != from)
            .collect();
        for id in targets {
            self.in_cli.remove(&id);
            let core = std::sync::Arc::clone(&self.core);
            let text = text.to_owned();
            let task = cx.background_spawn(async move {
                let sent = core.chats().send(&id, text, Vec::new(), queue);
                (id, sent)
            });
            cx.spawn(async move |this, cx| {
                let (id, sent) = task.await;
                // The workspace is gone only as the window closes.
                let _ = this.update(cx, |w, cx| {
                    if let Err(e) = sent {
                        log::warn!(chat:% = id, error:% = e; "could not send to chat {{chat}}: {{error}}");
                        w.error = Some(e.to_string());
                    }
                    w.reload(cx);
                });
            })
            .detach();
        }
    }

    /// Turns sending each message to every pane on or off.
    pub(crate) fn toggle_broadcast(&mut self, cx: &mut Context<Self>) {
        self.broadcasting = !self.broadcasting;
        cx.notify();
    }

    /// Opens chats `ids` side by side, each in a pane of its own, the first with the focus.
    pub(crate) fn open_side_by_side(&mut self, ids: &[String], cx: &mut Context<Self>) {
        let Some(first) = ids.first() else {
            return;
        };
        self.open_chat(first, cx);
        let mut last = self.shown_layout().focused;
        for id in &ids[1..] {
            self.dock_tab(id, last, Zone::Right, cx);
            last = self.shown_layout().focused;
        }
        let layout = self.shown_layout();
        if let super::Node::Split(split) = &mut layout.root {
            split.equalize();
        }
        self.open_chat(first, cx);
    }

    /// Shows the focused pane alone, or every pane again when one is shown alone.
    pub(crate) fn toggle_maximized(&mut self, cx: &mut Context<Self>) {
        let layout = self.shown_layout();
        layout.maximized =
            (layout.maximized.is_none() && layout.panes.len() > 1).then_some(layout.focused);
        cx.notify();
    }
}
