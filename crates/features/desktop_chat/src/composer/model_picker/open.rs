//! Opening the picker, and its keys.
//!
//! Up and Down move through the list, Left and Right switch the rail's tab, Shift with Left or
//! Right steps the effort, Enter picks without closing, and Escape or a press outside closes it.

// Rust guideline compliant 2026-10-09

use gpui::{Focusable, KeyDownEvent, KeystrokeEvent, ScrollHandle, Window, prelude::*};
use patchwork_model::provider::AgentKind;
use patchwork_design_editor::{Changed, Submitted, TextArea};

use super::nav::{self, Tab};
use super::rows::ModelPicker;
use crate::composer::Composer;
use crate::composer::menus::Pick;

impl Composer {
    /// Escape closes the picker, and Ctrl+1 to Ctrl+9 pick that row of the list.
    pub(super) fn picker_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keys = &event.keystroke;
        if keys.key == "escape" {
            self.close_menu(window, cx);
        } else if keys.modifiers.control
            && let Some(n) = keys
                .key
                .parse::<usize>()
                .ok()
                .filter(|n| (1..=9).contains(n))
        {
            if let Some(pick) = self.menu_picks.get(n - 1).cloned() {
                if let Some(picker) = &mut self.picker {
                    picker.index = n - 1;
                }
                self.commit(pick, cx);
            }
        } else {
            return;
        }
        cx.stop_propagation();
    }

    /// Tab (`forward`) or Shift+Tab moves between the search field and the effort slider; past
    /// either end it comes round rather than leaving the card open behind it.
    pub(super) fn picker_tab(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(picker) = &self.picker else {
            return;
        };
        let (dialog, search) = (
            picker.dialog.clone(),
            picker.search.read(cx).focus_handle(cx),
        );
        if forward {
            window.focus_next(cx);
        } else {
            window.focus_prev(cx);
        }
        if !dialog.contains_focused(window, cx) {
            window.focus(&search, cx);
        }
    }

    /// Opens the picker with the chat's model under the keys and the search empty.
    pub(in crate::composer) fn open_model_picker(
        &mut self,
        agents: &[AgentKind],
        current: (AgentKind, Option<String>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // With no agent set up yet, the chat's own agent still offers its models.
        let agents = &picker_agents(agents, current.0);
        self.reload_bare_models(agents, cx);
        self.effort_drag = None;
        let search = cx.new(|cx| {
            let mut field = TextArea::new("Search", "Search models", cx);
            field.single_line = true;
            field
        });
        cx.subscribe_in(&search, window, |this, _, _: &Changed, _, cx| {
            // What is typed changes the list; the keys start again at its first row.
            if let Some(picker) = &mut this.picker {
                picker.index = 0;
                picker.scroll.scroll_to_item(0);
            }
            cx.notify();
        })
        .detach();
        cx.subscribe_in(&search, window, |this, _, _: &Submitted, _, cx| {
            this.pick_spot(cx);
        })
        .detach();
        let focus = search.read(cx).focus_handle(cx);
        // The arrows are taken before the search field moves its caret with them, and only while
        // it has the keys: the slider has its own.
        let composer = cx.entity().downgrade();
        let watched = focus.clone();
        let arrows = cx.intercept_keystrokes(move |event: &KeystrokeEvent, window, cx| {
            if !watched.is_focused(window) {
                return;
            }
            let keys = &event.keystroke;
            let m = keys.modifiers;
            if m.alt || m.platform {
                return;
            }
            if m.control {
                if keys.key == "d" && !m.shift {
                    let _ = composer.update(cx, Composer::star_spot);
                    cx.stop_propagation();
                }
                return;
            }
            let handled = composer
                .update(cx, |this, cx| match (keys.key.as_str(), m.shift) {
                    ("left", true) => {
                        this.nudge_effort(-1, cx);
                        true
                    }
                    ("right", true) => {
                        this.nudge_effort(1, cx);
                        true
                    }
                    ("up", false) => this.move_row(-1, cx),
                    ("down", false) => this.move_row(1, cx),
                    // While searching there is no tab to switch, and the caret keeps the keys.
                    ("left", false) if this.picker_query_empty(cx) => this.move_tab(-1, cx),
                    ("right", false) if this.picker_query_empty(cx) => this.move_tab(1, cx),
                    _ => false,
                })
                .is_ok_and(|done| done);
            if handled {
                cx.stop_propagation();
            }
        });
        let (agent, model) = current;
        let tab = if agents.contains(&agent) {
            Tab::Agent(agent)
        } else {
            agents.first().map_or(Tab::Agent(agent), |&k| Tab::Agent(k))
        };
        self.picker = Some(ModelPicker {
            search,
            favorites: self.favorites.clone(),
            dialog: cx.focus_handle(),
            effort: cx.focus_handle().tab_stop(true),
            tab,
            index: 0,
            agents: agents.clone(),
            scroll: ScrollHandle::new(),
            _arrows: arrows,
        });
        // The chat's model starts under the keys, its own agent's tab open.
        let index = self
            .picker_rows(agents, cx)
            .iter()
            .position(|r| r.row.kind == agent && r.row.item.id == model)
            .unwrap_or_default();
        if let Some(picker) = &mut self.picker {
            picker.index = index;
            picker.scroll.scroll_to_item(index);
        }
        window.focus(&focus, cx);
    }

    /// Whether nothing is typed in the search field.
    fn picker_query_empty(&self, cx: &gpui::App) -> bool {
        self.picker
            .as_ref()
            .is_none_or(|p| p.search.read(cx).content.trim().is_empty())
    }

    /// Moves the keys `by` rows, wrapping round the list.
    fn move_row(&mut self, by: isize, cx: &mut Context<Self>) -> bool {
        let Some(picker) = &self.picker else {
            return false;
        };
        let len = self.picker_rows(&picker.agents, cx).len();
        if let Some(picker) = &mut self.picker {
            picker.index = nav::step_row(len, picker.index, by);
            picker.scroll.scroll_to_item(picker.index);
        }
        cx.notify();
        true
    }

    /// Opens the rail tab `by` steps along, with the keys on its first row.
    fn move_tab(&mut self, by: isize, cx: &mut Context<Self>) -> bool {
        let Some(picker) = &self.picker else {
            return false;
        };
        let tabs = self.picker_tabs(&picker.agents);
        let tab = nav::step_tab(&tabs, picker.tab, by);
        self.open_tab(tab, cx);
        true
    }

    /// Lists `tab`'s models, with the keys on its first row.
    pub(super) fn open_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if let Some(picker) = &mut self.picker {
            picker.tab = tab;
            picker.index = 0;
            picker.scroll.scroll_to_item(0);
        }
        cx.notify();
    }

    /// The pick of the row under the keys, and its `agent:model` key.
    fn spotted(&self, cx: &gpui::App) -> Option<(Pick, String)> {
        let picker = self.picker.as_ref()?;
        let rows = self.picker_rows(&picker.agents, cx);
        let row = &rows.get(picker.index.min(rows.len().checked_sub(1)?))?.row;
        Some((Pick::Model(row.kind, row.item.id.clone()), row.key.clone()))
    }

    /// Enter: picks the row under the keys, with the card left open for the effort.
    fn pick_spot(&mut self, cx: &mut Context<Self>) {
        if let Some((pick, _)) = self.spotted(cx) {
            self.commit(pick, cx);
        }
    }

    /// Ctrl+D: stars the row under the keys, or unstars it.
    pub(super) fn star_spot(&mut self, cx: &mut Context<Self>) {
        if let Some((_, key)) = self.spotted(cx) {
            self.toggle_favorite(&key, cx);
        }
    }
}
/// The agents the picker lists: `agents`, or the chat's own agent when none is set up.
pub(in crate::composer) fn picker_agents(agents: &[AgentKind], own: AgentKind) -> Vec<AgentKind> {
    if agents.is_empty() {
        vec![own]
    } else {
        agents.to_vec()
    }
}
