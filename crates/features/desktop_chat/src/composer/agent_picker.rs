//! The custom-agent picker: a searchable, single-line list matching the model picker, with
//! full descriptions on hover and a hover-revealed edit button on each row.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, Entity, FocusHandle, Focusable, FontWeight, KeyDownEvent, KeystrokeEvent,
    MouseButton, MouseDownEvent, Role, ScrollHandle, SharedString, Subscription, Window, div,
    prelude::*, px,
};
use patchwork_client::local::core::extensions::agents::Agent;
use patchwork_views_actions::{FocusNext, FocusPrev};
use patchwork_design_components::{popup_shadow, tooltip};
use patchwork_design_editor::{Changed, Submitted, TextArea};
use patchwork_design_icons::{IconName, icon};
use patchwork_design_theme::{Theme, fs, fs_md, fs_xs, radius};

use super::Composer;
use super::menus::{Pick, Roles};

/// The card's width, wide enough for a name and its one-line description side by side.
const WIDTH: f32 = 420.;
/// The list's maximum height, which scrolls once there are more agents than fit.
const MAX_BODY: f32 = 320.;
/// One row's fixed height, matching the model picker.
const ROW: f32 = 32.;

pub struct AgentPicker {
    /// The keys' home: typing filters by name or description, and arrows move through the list.
    pub search: Entity<TextArea>,
    /// The card's focus handle, so clicking inside it keeps Escape working.
    pub dialog: FocusHandle,
    /// The row under the arrows or the pointer.
    pub index: usize,
    /// The list's scroll, which follows the row under the keys.
    pub scroll: ScrollHandle,
    /// Takes arrow keys from the search field before they move its caret; dropped with the picker.
    _arrows: Subscription,
}

/// One selectable entry in the filtered list.
#[derive(Clone)]
enum Entry {
    /// Clears the chat's custom agent.
    None,
    /// Selects a custom agent.
    Agent(Agent),
}

impl Entry {
    fn pick(&self) -> Pick {
        match self {
            Self::None => Pick::Role(None),
            Self::Agent(agent) => Pick::Role(Some(agent.name.clone())),
        }
    }

    fn matches_role(&self, role: Option<&str>) -> bool {
        match self {
            Self::None => role.is_none(),
            Self::Agent(agent) => role == Some(agent.name.as_str()),
        }
    }
}

/// Filters `"No agent"` and `agents` against `query` (trimmed, case-insensitive).
fn filtered_entries(agents: &[Agent], query: &str) -> Vec<Entry> {
    let q = query.trim().to_lowercase();
    let mut entries = Vec::with_capacity(agents.len() + 1);
    if q.is_empty() || "no agent".contains(&q) || "none".contains(&q) {
        entries.push(Entry::None);
    }
    for agent in agents {
        if q.is_empty() || matches_query(agent, &q) {
            entries.push(Entry::Agent(agent.clone()));
        }
    }
    entries
}

fn matches_query(agent: &Agent, query: &str) -> bool {
    agent.name.to_lowercase().contains(query)
        || agent
            .title
            .as_deref()
            .is_some_and(|t| t.to_lowercase().contains(query))
        || agent
            .description
            .as_deref()
            .is_some_and(|d| d.to_lowercase().contains(query))
}

/// Moves `index` by `by` steps through `len` rows, wrapping at both ends.
fn step_row(len: usize, index: usize, by: isize) -> usize {
    if len == 0 {
        return 0;
    }
    let len = isize::try_from(len).unwrap_or(isize::MAX);
    let at = isize::try_from(index.min(usize::try_from(len - 1).unwrap_or(0))).unwrap_or(0);
    usize::try_from((at + by).rem_euclid(len)).unwrap_or(0)
}

/// The tooltip shown on hover for a custom agent row: its full description, and its model if set.
fn agent_tooltip(agent: &Agent) -> Option<String> {
    match (&agent.description, &agent.model) {
        (Some(desc), Some(model)) => Some(format!("{desc}\nModel: {model}")),
        (Some(desc), None) => Some(desc.clone()),
        (None, Some(model)) => Some(format!("Model: {model}")),
        (None, None) => None,
    }
}

impl Composer {
    /// Opens the agent picker with the chat's current agent highlighted and the search focused.
    pub(super) fn open_agent_picker(
        &mut self,
        current: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.load_roles(cx);
        let search = cx.new(|cx| {
            let mut field = TextArea::new("Search agents", "Search agents", cx);
            field.single_line = true;
            field
        });
        cx.subscribe_in(&search, window, |this, _, _: &Changed, _, cx| {
            if let Some(picker) = &mut this.agent_picker {
                picker.index = 0;
                picker.scroll.scroll_to_item(0);
            }
            cx.notify();
        })
        .detach();
        cx.subscribe_in(&search, window, |this, _, _: &Submitted, window, cx| {
            this.pick_agent_spot(window, cx);
        })
        .detach();
        let focus = search.read(cx).focus_handle(cx);
        let composer = cx.entity().downgrade();
        let watched = focus.clone();
        let arrows = cx.intercept_keystrokes(move |event: &KeystrokeEvent, window, cx| {
            if !watched.is_focused(window) {
                return;
            }
            let keys = &event.keystroke;
            let m = keys.modifiers;
            if m.alt || m.control || m.platform || m.shift {
                return;
            }
            let handled = composer
                .update(cx, |this, cx| match keys.key.as_str() {
                    "up" => this.move_agent_row(-1, cx),
                    "down" => this.move_agent_row(1, cx),
                    "home" => this.jump_agent_row(false, cx),
                    "end" => this.jump_agent_row(true, cx),
                    _ => false,
                })
                .is_ok_and(|done| done);
            if handled {
                cx.stop_propagation();
            }
        });
        let entries = self.agent_entries("");
        let index = entries
            .iter()
            .position(|e| e.matches_role(current))
            .unwrap_or(0);
        let scroll = ScrollHandle::new();
        scroll.scroll_to_item(index);
        self.agent_picker = Some(AgentPicker {
            search,
            dialog: cx.focus_handle(),
            index,
            scroll,
            _arrows: arrows,
        });
        window.focus(&focus, cx);
    }

    /// The filtered entries for `query` from the currently loaded custom agents.
    fn agent_entries(&self, query: &str) -> Vec<Entry> {
        let agents = match &self.roles {
            Roles::Loaded(agents) => agents.as_slice(),
            Roles::Loading => &[],
        };
        filtered_entries(agents, query)
    }

    fn agent_query(&self, cx: &gpui::App) -> String {
        self.agent_picker
            .as_ref()
            .map(|p| p.search.read(cx).content.clone())
            .unwrap_or_default()
    }

    fn move_agent_row(&mut self, by: isize, cx: &mut Context<Self>) -> bool {
        let query = self.agent_query(cx);
        let len = self.agent_entries(&query).len();
        let Some(picker) = &mut self.agent_picker else {
            return false;
        };
        picker.index = step_row(len, picker.index, by);
        picker.scroll.scroll_to_item(picker.index);
        cx.notify();
        true
    }

    fn jump_agent_row(&mut self, end: bool, cx: &mut Context<Self>) -> bool {
        let query = self.agent_query(cx);
        let len = self.agent_entries(&query).len();
        let Some(picker) = &mut self.agent_picker else {
            return false;
        };
        picker.index = if end { len.saturating_sub(1) } else { 0 };
        picker.scroll.scroll_to_item(picker.index);
        cx.notify();
        true
    }

    fn pick_agent_spot(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.agent_query(cx);
        let entries = self.agent_entries(&query);
        let Some(index) = self.agent_picker.as_ref().map(|p| p.index) else {
            return;
        };
        if let Some(entry) = entries.get(index) {
            self.apply(entry.pick(), window, cx);
        }
    }

    fn agent_picker_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key == "escape" {
            self.close_menu(window, cx);
            cx.stop_propagation();
        }
    }

    /// Renders the redesigned custom-agent picker card.
    pub(super) fn agent_picker(
        &mut self,
        current: Option<&str>,
        t: &Theme,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let query = self.agent_query(cx);
        let searching = !query.trim().is_empty();
        let entries = self.agent_entries(&query);
        let loading = matches!(self.roles, Roles::Loading);
        let no_custom_agents = matches!(&self.roles, Roles::Loaded(a) if a.is_empty());

        self.menu_picks = entries.iter().map(Entry::pick).collect();
        let Some(picker) = &mut self.agent_picker else {
            return div().into_any_element();
        };
        picker.index = picker.index.min(entries.len().saturating_sub(1));
        let spotted_index = picker.index;
        let search = picker.search.clone();
        let dialog = picker.dialog.clone();
        let scroll = picker.scroll.clone();
        let search_focus = search.read(cx).focus_handle(cx);

        let has_none_row = matches!(entries.first(), Some(Entry::None));
        let rows: Vec<AnyElement> = entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let divider = has_none_row && i == 1;
                agent_row(i, entry, current, i == spotted_index, divider, t, cx)
            })
            .collect();

        let status = if loading {
            Some("Loading agents…")
        } else if entries.is_empty() && searching {
            Some("Nothing matches")
        } else if no_custom_agents && !searching {
            Some("No custom agents")
        } else {
            None
        }
        .map(|message| {
            div()
                .id("agents-status")
                .role(Role::Status)
                .px(px(12.))
                .py(px(14.))
                .text_center()
                .text_size(px(fs_xs()))
                .text_color(t.text_subtle)
                .child(message)
        });

        div()
            .id("agent-picker")
            .role(Role::Dialog)
            .aria_label("Agent")
            .occlude()
            .track_focus(&dialog)
            .on_action(cx.listener(|this, _: &FocusNext, window, cx| {
                this.close_menu(window, cx);
            }))
            .on_action(cx.listener(|this, _: &FocusPrev, window, cx| {
                this.close_menu(window, cx);
            }))
            .on_mouse_down_out(cx.listener(|this, e: &MouseDownEvent, window, cx| {
                this.dismissed = this.menu.take().map(|m| (m, e.position));
                this.agent_picker = None;
                this.restore_focus(window, cx);
                cx.notify();
            }))
            .on_key_down(cx.listener(Self::agent_picker_key))
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(radius()))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_elevated)
            .text_color(t.text)
            .shadow(popup_shadow(t))
            .child(
                div()
                    .h(px(40.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pl(px(12.))
                    .pr(px(10.))
                    .border_b_1()
                    .border_color(t.border)
                    .cursor_text()
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        window.focus(&search_focus, cx);
                    })
                    .child(
                        icon(IconName::MagnifyingGlass, 14.)
                            .flex_none()
                            .text_color(t.text_subtle),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(fs_md()))
                            .line_height(px(20.))
                            .child(search),
                    ),
            )
            .child(
                div()
                    .id("agent-list")
                    .role(Role::ListBox)
                    .aria_label("Agents")
                    .track_scroll(&scroll)
                    .max_h(px(MAX_BODY))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .p(px(6.))
                    .children(rows)
                    .children(status),
            )
            .into_any_element()
    }

    pub(super) fn edit_agent(
        &mut self,
        agent: &Agent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = std::path::PathBuf::from(&agent.path);
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
            return;
        };
        let (dir, name) = (
            dir.to_string_lossy().into_owned(),
            name.to_string_lossy().into_owned(),
        );
        cx.background_spawn(async move {
            if let Err(e) = patchwork_platform::open_in_editor(&dir, &name, None) {
                log::warn!(error:% = e; "could not open the agent: {{error}}");
            }
        })
        .detach();
        self.close_menu(window, cx);
    }
}

/// One single-line row in the agent picker: icon, name, truncated description, check when active,
/// and an edit button that fades in on hover or keyboard focus.
fn agent_row(
    i: usize,
    entry: &Entry,
    current: Option<&str>,
    spotted: bool,
    divider: bool,
    t: &Theme,
    cx: &mut Context<Composer>,
) -> AnyElement {
    let selected = entry.matches_role(current);
    let pick = entry.pick();
    let group = SharedString::from(format!("agent-row-{i}"));
    let fg = if selected || spotted {
        t.text
    } else {
        t.text_muted
    };
    let icon_color = if selected || spotted {
        t.text_muted
    } else {
        t.text_subtle
    };

    let (aria_label, lead_icon, name, description, tip, edit_agent) = match entry {
        Entry::None => (
            SharedString::from("No agent"),
            IconName::Prohibit,
            SharedString::from("No agent"),
            None,
            None,
            None,
        ),
        Entry::Agent(agent) => (
            SharedString::from(agent.name.clone()),
            IconName::Robot,
            SharedString::from(agent.name.clone()),
            agent.description.clone(),
            agent_tooltip(agent),
            Some(agent.clone()),
        ),
    };

    let hover = t.bg_hover;
    let edit_fg = t.text;
    let edit = edit_agent.map(|agent| {
        let edit_label = format!("Edit {}", agent.name);
        div()
            .id(SharedString::from(format!("edit-agent-{i}")))
            .role(Role::Button)
            .aria_label(SharedString::from(edit_label.clone()))
            .size(px(22.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .text_color(t.text_subtle)
            .when(!spotted, |s| {
                s.opacity(0.).group_hover(group.clone(), |s| s.opacity(1.))
            })
            .hover(move |s| s.bg(hover).text_color(edit_fg))
            .tooltip(tooltip(edit_label))
            .child(icon(IconName::PencilSimple, 12.))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.edit_agent(&agent, window, cx);
            }))
    });

    let line = div()
        .id(SharedString::from(format!("agent-option-{i}")))
        .group(group)
        .role(Role::ListBoxOption)
        .aria_label(aria_label)
        .aria_selected(selected)
        .h(px(ROW))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.))
        .pl(px(8.))
        .pr(px(4.))
        .rounded(px(6.))
        .when(spotted, |r| r.bg(t.bg_hover))
        .text_size(px(fs(13.)))
        .text_color(fg)
        .cursor_pointer()
        .when_some(tip, |r, tip| r.tooltip(tooltip(tip)))
        .child(icon(lead_icon, 14.).flex_none().text_color(icon_color))
        .child(
            div()
                .flex_none()
                .max_w(px(160.))
                .truncate()
                .when(selected, |d| d.font_weight(FontWeight::MEDIUM))
                .child(name),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(fs_xs()))
                .text_color(t.text_subtle)
                .children(description),
        )
        .when(selected, |r| {
            r.child(
                icon(IconName::CheckBold, 12.)
                    .flex_none()
                    .text_color(t.accent_text),
            )
        })
        .children(edit)
        .on_mouse_move(cx.listener(move |this, _, _, cx| {
            if let Some(picker) = &mut this.agent_picker
                && picker.index != i
            {
                picker.index = i;
                cx.notify();
            }
        }))
        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
        .on_click(cx.listener(move |this, _, window, cx| this.apply(pick.clone(), window, cx)));

    div()
        .flex_none()
        .flex()
        .flex_col()
        .when(divider, |d| {
            d.child(div().my(px(4.)).mx(px(4.)).h(px(1.)).bg(t.border))
        })
        .child(line)
        .into_any_element()
}
