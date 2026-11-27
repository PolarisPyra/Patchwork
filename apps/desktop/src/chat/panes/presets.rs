//! Ready-made layouts picked from the layout button: one pane, two side by side or stacked, three
//! side by side, a grid of four, or one beside two stacked.

// Rust guideline compliant 2026-10-09

use gpui::Context;
use patchwork_ui_theme::IconName;

use super::tree::Split;
use super::{Axis, Node, Pane, PaneId};
use crate::workspace::Workspace;

/// A ready-made layout.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Preset {
    Single,
    Columns,
    Rows,
    ThreeColumns,
    Grid,
    MainAndStack,
}

impl Preset {
    /// Every preset, in the order the menu lists them.
    pub(crate) const ALL: [Self; 6] = [
        Self::Single,
        Self::Columns,
        Self::Rows,
        Self::ThreeColumns,
        Self::Grid,
        Self::MainAndStack,
    ];

    /// The name the menu shows.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Single => "One pane",
            Self::Columns => "Two side by side",
            Self::Rows => "Two stacked",
            Self::ThreeColumns => "Three side by side",
            Self::Grid => "Grid of four",
            Self::MainAndStack => "One beside two stacked",
        }
    }

    /// The icon the menu shows, a picture of the layout.
    pub(crate) fn icon(self) -> IconName {
        match self {
            Self::Single => IconName::Square,
            Self::Columns => IconName::SquareSplitHorizontal,
            Self::Rows => IconName::SquareSplitVertical,
            Self::ThreeColumns => IconName::SquareSplitThree,
            Self::Grid => IconName::SquareGridFour,
            Self::MainAndStack => IconName::SquareSplitStack,
        }
    }

    /// How many panes it has.
    fn count(self) -> usize {
        match self {
            Self::Single => 1,
            Self::Columns | Self::Rows => 2,
            Self::ThreeColumns | Self::MainAndStack => 3,
            Self::Grid => 4,
        }
    }

    /// Its shape, with panes `ids` in reading order; `ids` holds [`Self::count`] of them.
    fn shape(self, ids: &[PaneId]) -> Node {
        let pane = |at: usize| Node::Pane(ids[at]);
        let split = |axis, parts| Node::Split(Split::even(axis, parts));
        match self {
            Self::Single => pane(0),
            Self::Columns => split(Axis::Row, vec![pane(0), pane(1)]),
            Self::Rows => split(Axis::Column, vec![pane(0), pane(1)]),
            Self::ThreeColumns => split(Axis::Row, vec![pane(0), pane(1), pane(2)]),
            Self::Grid => split(
                Axis::Row,
                vec![
                    split(Axis::Column, vec![pane(0), pane(1)]),
                    split(Axis::Column, vec![pane(2), pane(3)]),
                ],
            ),
            Self::MainAndStack => split(
                Axis::Row,
                vec![pane(0), split(Axis::Column, vec![pane(1), pane(2)])],
            ),
        }
    }
}

impl Workspace {
    /// Lays the shown project's panes out as `preset`. The panes there stay, in reading order;
    /// those past the preset's count give their tabs to its last pane. A new pane takes a tab
    /// from a pane with more than one, or else a task with no tab, and stays empty when there is
    /// neither, for a tab to be dragged into.
    pub(crate) fn apply_preset(&mut self, preset: Preset, cx: &mut Context<Self>) {
        self.apply_shape(preset.count(), |ids| preset.shape(ids), cx);
    }

    /// Lays the shown project's panes out as `shape` makes them from `count` panes in reading
    /// order, as [`Self::apply_preset`] does.
    pub(crate) fn apply_shape(
        &mut self,
        count: usize,
        shape: impl FnOnce(&[PaneId]) -> Node,
        cx: &mut Context<Self>,
    ) {
        if count == 0 {
            return;
        }
        let project = self.tabs_project();
        let open = self.chat.as_ref().map(|c| c.id.clone());
        let mut spare: Vec<String> = self
            .untabbed_tasks(project.as_deref())
            .into_iter()
            .map(|c| c.id.clone())
            .collect();
        let mut fresh = Vec::new();
        let mut order = Vec::new();
        self.layout_mut(project.as_deref())
            .root
            .pane_ids(&mut order);
        for _ in order.len()..count {
            fresh.push(self.next_pane());
        }
        let layout = self.layout_mut(project.as_deref());
        // Panes past the count fold into the last one kept.
        let mut kept: Vec<PaneId> = order.iter().copied().take(count).collect();
        if let Some(&last) = kept.last() {
            for gone in order.iter().skip(count) {
                let Some(at) = layout.panes.iter().position(|p| p.id == *gone) else {
                    continue;
                };
                let pane = layout.panes.remove(at);
                if layout.focused == pane.id {
                    layout.focused = last;
                }
                if let Some(into) = layout.pane_mut(last) {
                    into.tabs.extend(pane.tabs);
                }
            }
        }
        // The focused pane's chat is the open one, held by the workspace, not the pane.
        let focused = layout.focused;
        if let Some(pane) = layout.pane_mut(focused) {
            pane.view = None;
            if open.as_ref().is_some_and(|o| pane.tabs.contains(o)) {
                pane.shown.clone_from(&open);
            }
        }
        // New panes, each with a chat when there is one to spare.
        for id in fresh {
            let tab = spare_tab(&mut layout.panes)
                .or_else(|| (!spare.is_empty()).then(|| spare.remove(0)));
            layout.panes.push(Pane {
                id,
                shown: tab.clone(),
                tabs: tab.into_iter().collect(),
                view: None,
            });
            kept.push(id);
        }
        layout.root = shape(&kept);
        layout.maximized = None;
        cx.notify();
    }
}

/// A tab to move out of the pane with the most, when one has more than one: its last tab that
/// it does not show.
fn spare_tab(panes: &mut [Pane]) -> Option<String> {
    let pane = panes
        .iter_mut()
        .filter(|p| p.tabs.len() > 1)
        .max_by_key(|p| p.tabs.len())?;
    let at = pane
        .tabs
        .iter()
        .rposition(|t| pane.shown.as_ref() != Some(t))?;
    Some(pane.tabs.remove(at))
}
