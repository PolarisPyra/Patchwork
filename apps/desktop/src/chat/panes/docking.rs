//! A tab moved between panes: dragged onto a pane's edge to split it, or onto its middle or its
//! tabs to join them, or sent to a new pane from a chat's menu. A task dragged in from the
//! sidebar lands the same way, as a new tab.

// Rust guideline compliant 2026-10-09

use std::time::Duration;

use gpui::{Bounds, Context, Pixels, SharedString, Window};

use super::{Axis, Pane, PaneId};
use crate::workspace::Workspace;

/// A tab being dragged: its chat, and its title for what follows the pointer.
#[derive(Clone)]
pub(crate) struct TabDrag {
    pub(crate) chat: String,
    pub(crate) title: SharedString,
}

/// How long a dragged tab is held over another tab before that one shows.
const SPRING_DELAY: Duration = Duration::from_millis(600);

/// Where in a pane a tab is dropped: its middle joins the pane's tabs, an edge splits it there.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Zone {
    Middle,
    Left,
    Right,
    Top,
    Bottom,
}

impl Zone {
    /// The split an edge makes, and whether the new pane goes before the old one.
    pub(super) fn split(self) -> Option<(Axis, bool)> {
        match self {
            Self::Middle => None,
            Self::Left => Some((Axis::Row, true)),
            Self::Right => Some((Axis::Row, false)),
            Self::Top => Some((Axis::Column, true)),
            Self::Bottom => Some((Axis::Column, false)),
        }
    }
}

impl Workspace {
    /// Whether chat `id` can go to a pane of its own beside the focused one: it is of the
    /// project whose panes show, and the focused pane has another tab to keep.
    pub(crate) fn can_split(&self, id: &str) -> bool {
        let project = self.project_of(id);
        if self.tabs_project() != project {
            return false;
        }
        // Another tab stays in the focused pane, so neither pane is left empty.
        self.layouts
            .get(&project)
            .and_then(|l| l.pane(l.focused))
            .is_some_and(|p| p.tabs.iter().any(|t| t != id))
    }

    /// Opens chat `id` in a new pane at `zone`'s edge of the focused one.
    pub(crate) fn open_in_split(&mut self, id: &str, zone: Zone, cx: &mut Context<Self>) {
        let target = self.shown_layout().focused;
        self.dock_tab(id, target, zone, cx);
    }

    /// Moves chat `id`'s tab to pane `target`: among its tabs for the middle, or into a new pane
    /// at an edge. The chat moved shows and has the focus; a pane left with no tabs closes.
    pub(crate) fn dock_tab(
        &mut self,
        id: &str,
        target: PaneId,
        zone: Zone,
        cx: &mut Context<Self>,
    ) {
        let project = self.project_of(id);
        // A task of another project, dragged from the sidebar, opens among its own panes, and an
        // agent's chat, which takes no tab, opens alone.
        if self.tabs_project() != project || self.is_agent_chat(id) {
            self.open_chat(id, cx);
            return;
        }
        let new_pane = self.next_pane();
        let layout = self.layout_mut(project.as_deref());
        let Some(target_tabs) = layout.pane(target).map(|p| p.tabs.len()) else {
            return;
        };
        let from = layout.pane_of(id);
        let split = zone.split();
        if from == Some(target) && (split.is_none() || target_tabs == 1) {
            return;
        }
        // The open chat stays drawn in its pane, unless it is the one that moves.
        let open_here = self.chat.as_ref().is_some_and(|c| c.project_id == project);
        let kept = if open_here { self.take_open(cx) } else { None };
        let layout = self.layout_mut(project.as_deref());
        layout.focused_pane().view = kept;
        // Out of its pane, with its transcript.
        let mut moving = None;
        if let Some(pane) = from.and_then(|f| layout.pane_mut(f))
            && let Some(at) = pane.tabs.iter().position(|t| t == id)
        {
            pane.tabs.remove(at);
            if pane.view.as_ref().and_then(|v| v.chat_id()) == Some(id) {
                moving = pane.view.take();
            }
            if pane.shown.as_deref() == Some(id) {
                pane.shown = pane
                    .tabs
                    .get(at)
                    .or_else(|| at.checked_sub(1).and_then(|b| pane.tabs.get(b)))
                    .cloned();
            }
        }
        // Into the pane it goes to.
        let dest = match split {
            None => target,
            Some((axis, before)) => {
                layout.root.split(target, new_pane, axis, before);
                layout.panes.push(Pane {
                    id: new_pane,
                    tabs: Vec::new(),
                    shown: None,
                    view: None,
                });
                new_pane
            }
        };
        if let Some(pane) = layout.pane_mut(dest) {
            let after = pane
                .shown
                .as_ref()
                .and_then(|s| pane.tabs.iter().position(|t| t == s))
                .map_or(pane.tabs.len(), |at| at + 1);
            pane.tabs.insert(after, id.to_owned());
            pane.shown = Some(id.to_owned());
            pane.view = moving;
        }
        if let Some(f) = from
            && layout.pane(f).is_some_and(|p| p.tabs.is_empty())
        {
            layout.remove(f);
        }
        layout.focused = dest;
        let slot = layout.pane_mut(dest).and_then(|p| p.view.take());
        self.open_chat(id, cx);
        if let Some(slot) = slot {
            self.restore_view(slot);
        }
        cx.notify();
    }

    /// Moves chat `id`'s tab to just before chat `onto`'s, in `onto`'s pane.
    pub(crate) fn drop_tab_before(&mut self, id: &str, onto: &str, cx: &mut Context<Self>) {
        if id == onto {
            return;
        }
        let project = self.project_of(onto);
        let Some(pane) = self.layout_mut(project.as_deref()).pane_of(onto) else {
            return;
        };
        if self.layout_mut(project.as_deref()).pane_of(id) != Some(pane) {
            self.dock_tab(id, pane, Zone::Middle, cx);
        }
        let layout = self.layout_mut(project.as_deref());
        let Some(tabs) = layout.pane_mut(pane).map(|p| &mut p.tabs) else {
            return;
        };
        let Some(from) = tabs.iter().position(|t| t == id) else {
            return;
        };
        let moving = tabs.remove(from);
        let to = tabs.iter().position(|t| t == onto).unwrap_or(tabs.len());
        tabs.insert(to, moving);
        cx.notify();
    }

    /// While a tab is dragged over chat `onto`'s tab, lying in `bounds`: once it has been held
    /// there a moment, `onto`'s tab shows in its pane, so the drag can go on into its chat.
    pub(crate) fn hold_tab_over(
        &mut self,
        onto: &str,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.spring_tab.as_deref() == Some(onto) {
            return;
        }
        self.spring_tab = Some(onto.to_owned());
        let onto = onto.to_owned();
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(SPRING_DELAY).await;
            // The window is gone only as it closes, with no drag left to finish.
            let _ = this.update_in(cx, |w, window, cx| {
                if w.spring_tab.as_deref() != Some(onto.as_str()) {
                    return;
                }
                w.spring_tab = None;
                if cx.has_active_drag() && bounds.contains(&window.mouse_position()) {
                    w.show_tab(&onto, cx);
                }
            });
        })
        .detach();
    }

    /// Shows chat `id`'s tab in its pane: the open chat for the focused pane, else what the pane
    /// draws.
    fn show_tab(&mut self, id: &str, cx: &mut Context<Self>) {
        let project = self.project_of(id);
        let layout = self.layout_mut(project.as_deref());
        let Some(pane) = layout.pane_of(id) else {
            return;
        };
        if pane == layout.focused {
            self.open_chat(id, cx);
        } else if let Some(p) = layout.pane_mut(pane) {
            p.shown = Some(id.to_owned());
        }
        cx.notify();
    }
}
