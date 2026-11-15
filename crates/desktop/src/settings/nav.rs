//! The Settings list: its search, its entries, and the panes of the open entry under it.

// Rust guideline compliant 2026-10-09

use std::time::Duration;
use gpui::{
    Animation, AnimationExt, ease_out_quint, AnyElement, Entity, Focusable, Role, ScrollAnchor,
    SharedString, Window, div, prelude::*, px,
};
use crate::ui::text_area::{Arrow, Changed, Submitted, TextArea};
use crate::ui::theme::{Theme, fs_md, fs_sm, input_placeholder, radius_sm, theme};
use crate::ui::{button, icon, mix};
use super::controls::seek;
use super::dialog::{ENTRIES, SettingsDialog, entry_of};

/// How tall a pane's row under its entry is.
const SUB_ROW: f32 = 28.;

/// How an entry's panes slide open and shut.
fn slide() -> Animation {
    Animation::new(Duration::from_millis(160)).with_easing(ease_out_quint())
}

/// The most results the search lists.
const MAX_HITS: usize = 10;

/// The settings the search finds, by the pane that holds them. A name here is the row's title in
/// its pane, so a row added to a pane is added here too.
const INDEX: &[(&str, &[&str])] = &[
    (
        "general",
        &[
            "App icon",
            "Notifications",
            "Goal completed",
            "Task completed",
            "Logs",
            "Open logs in chat menus",
            "Log folder",
            "Agents can start and message chats",
            "Click answers twice to send",
            "Cross-model review",
            "Enter steers a running reply",
            "Granular chats",
            "Resume interrupted replies at launch",
            "Stop agent after 200 tool steps",
            "Token limit per turn",
        ],
    ),
    (
        "updates",
        &[
            "Check for updates every",
            "Restart to update",
            "Release notes",
        ],
    ),
    (
        "themes",
        &[
            "Color scheme",
            "Color scheme in dark",
            "Match the system",
            "Accent",
            "Switch with",
            "Generate with",
            "Text size",
            "Reduce motion",
        ],
    ),
    ("code-theme", &["Syntax theme", "Generate with"]),
    (
        "orchestrator-background",
        &["Orchestrator theme", "Generate with"],
    ),
    ("chat-background", &["Chat theme", "Generate with"]),
    ("interface", &["Font"]),
    ("shortcuts", &["Keyboard shortcuts", "Reset all"]),
    (
        "providers",
        &[
            "Add a provider",
            "Default for new tasks",
            "Local providers",
            "Vision model",
        ],
    ),
    (
        "tokens",
        &[
            "Ask before resuming a large task",
            "Generate chat and task titles",
            "Only local models write titles",
            "Show token counts",
            "Warn above",
            "Write titles with",
        ],
    ),
    ("extensions", &["Agents", "Skills", "Hooks"]),
    (
        "permissions",
        &[
            "Let agents use Android devices",
            "Let the agent use the screen",
            "Restore defaults",
        ],
    ),
    ("projects", &["Imports", "Run checks after every task"]),
    ("scheduled", &["Scheduled tasks", "Weekly report"]),
    (
        "terminal",
        &[
            "Shell",
            "Starting folder",
            "Project folder",
            "Inherit environment",
            "Font",
            "Font size",
            "Font weight",
            "Bold font weight",
            "Line height",
            "Letter spacing",
            "Cursor style",
            "Cursor blinking",
            "Cursor width",
            "Scrollback",
            "Scroll speed",
            "Fast scroll speed",
            "Smooth scrolling",
            "Copy on selection",
            "Right click",
            "Alt-click moves the cursor",
            "Option is Meta",
            "Minimum contrast ratio",
            "Bold text in bright colors",
        ],
    ),
    ("ssh", &["SSH devices"]),
];

/// One search result: a setting's name and the pane that holds it.
#[derive(Clone, Copy)]
pub(super) struct Hit {
    pub(super) pane: &'static str,
    name: &'static str,
    /// The pane's name, shown under a setting's; empty when the result is the pane itself.
    place: &'static str,
}

/// What `query`, in lower case, finds: the panes whose name has it, then the settings.
pub(super) fn hits(query: &str) -> Vec<Hit> {
    let has = |text: &str| text.to_lowercase().contains(query);
    let panes = ENTRIES.iter().flat_map(|e| {
        e.panes
            .iter()
            .filter(move |p| has(p.label) || has(e.label))
            .map(|p| Hit {
                pane: p.id,
                name: p.label,
                place: "",
            })
    });
    let settings = INDEX.iter().flat_map(|(pane, names)| {
        let place = entry_of(pane).map_or("", |(_, _, p)| p.label);
        names
            .iter()
            .filter(move |name| has(name))
            .map(move |name| Hit { pane, name, place })
    });
    panes.chain(settings).take(MAX_HITS).collect()
}

/// Every pane's id, in the order the list shows them.
pub(super) fn pane_order() -> impl Iterator<Item = &'static str> {
    ENTRIES.iter().flat_map(|e| e.panes.iter().map(|p| p.id))
}

impl SettingsDialog {
    /// What the search field holds, trimmed and in lower case.
    pub(super) fn query(&self, cx: &Context<Self>) -> String {
        self.search
            .as_ref()
            .map(|s| s.read(cx).content.trim().to_lowercase())
            .unwrap_or_default()
    }

    /// Moves the highlight along the results by `step`.
    pub(super) fn step_hit(&mut self, step: isize, cx: &mut Context<Self>) {
        let count = hits(&self.query(cx)).len();
        if count > 0 {
            self.search_at = (self.search_at as isize + step).clamp(0, count as isize - 1) as usize;
            cx.notify();
        }
    }

    /// Opens the pane of the result at `at`, empties the search and focuses the pane's row.
    pub(super) fn open_hit(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(hit) = hits(&self.query(cx)).get(at).copied() else {
            return;
        };
        self.clear_search(cx);
        self.open_section(hit.pane, cx);
        self.focus_row(hit.pane, window, cx);
        if hit.place.is_empty() {
            return;
        }
        // The setting's row is drawn on the next frame, and the pane scrolls to it on the one
        // after.
        let anchor = ScrollAnchor::for_handle(self.scroll.clone());
        seek(Some((hit.name, anchor.clone())));
        window.on_next_frame(move |window, cx| {
            anchor.scroll_to(window, cx);
            window.on_next_frame(|window, _| {
                seek(None);
                window.refresh();
            });
        });
    }

    pub(super) fn clear_search(&mut self, cx: &mut Context<Self>) {
        if let Some(search) = &self.search {
            search.update(cx, |area, cx| area.set_text("", cx));
        }
        self.search_at = 0;
        cx.notify();
    }

    /// Opens the entry at `at` and shows its panes; one already showing them closes over them.
    pub(super) fn toggle_entry(&mut self, at: usize, cx: &mut Context<Self>) {
        if !self.expanded.remove(&at) {
            return self.open_entry(at, cx);
        }
        self.closing = Some(at);
        self.opening = None;
        cx.notify();
    }

    /// Shows the panes of the entry at `at`, sliding them open when they were shut.
    pub(super) fn expand(&mut self, at: usize) {
        let group = ENTRIES.get(at).is_some_and(|e| e.panes.len() > 1);
        if group && self.expanded.insert(at) {
            self.opening = Some(at);
            if self.closing == Some(at) {
                self.closing = None;
            }
        }
    }

    /// Focuses the list's row for the pane `id`.
    pub(super) fn focus_row(
        &mut self,
        id: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let row = self
            .rows
            .entry(id)
            .or_insert_with(|| cx.focus_handle())
            .clone();
        // The row is drawn only while its entry shows its panes.
        if let Some((at, _, _)) = entry_of(id) {
            self.expand(at);
        }
        window.focus(&row, cx);
    }

    fn nav_search(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<TextArea> {
        if let Some(search) = &self.search {
            return search.clone();
        }
        let search = cx.new(|cx| {
            let mut area = TextArea::new("Search", "Search settings", cx);
            area.single_line = true;
            area.placeholder_color = Some(input_placeholder());
            area
        });
        cx.subscribe(&search, |d, _, _: &Changed, cx| {
            d.search_at = 0;
            // The list comes back as it stood, with nothing sliding.
            d.opening = None;
            d.closing = None;
            cx.notify();
        })
        .detach();
        cx.subscribe(&search, |d, _, step: &Arrow, cx| d.step_hit(step.0, cx))
            .detach();
        cx.subscribe_in(&search, window, |d, _, _: &Submitted, window, cx| {
            d.open_hit(d.search_at, window, cx);
        })
        .detach();
        self.search = Some(search.clone());
        search
    }

    /// The list: the search, then its results, or the entries with the open one's panes.
    pub(super) fn nav(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let t = *theme(cx);
        let search = self.nav_search(window, cx);
        let focused = search.read(cx).focus_handle(cx).is_focused(window);
        let field = div()
            .flex()
            .flex_none()
            .h(px(28.))
            .mb(px(6.))
            .items_center()
            .gap(px(6.))
            .rounded(px(radius_sm()))
            .border_1()
            .border_color(if focused { t.accent } else { t.border_strong })
            .bg(t.bg_editor)
            .px(px(8.))
            .text_size(px(fs_md()))
            .text_color(t.text_muted)
            .child(icon("magnifying-glass", 14.))
            .child(div().flex_1().min_w_0().child(search))
            .into_any_element();
        let query = self.query(cx);
        let mut rows = vec![field];
        if query.is_empty() {
            self.entries(&t, &mut rows, cx);
        } else {
            rows.push(self.results(&query, &t, cx));
        }
        rows
    }

    fn results(&self, query: &str, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let found = hits(query);
        if found.is_empty() {
            return div()
                .id("settings-no-results")
                .role(Role::Status)
                .px(px(8.))
                .py(px(6.))
                .text_color(t.text_muted)
                .child("No settings match")
                .into_any_element();
        }
        let (hover, selected, muted) = (t.bg_hover, t.bg_selected, t.text_muted);
        let rows = found.into_iter().enumerate().map(|(at, hit)| {
            let current = at == self.search_at;
            let label = if hit.place.is_empty() {
                SharedString::new_static(hit.name)
            } else {
                SharedString::from(format!("{}, {}", hit.name, hit.place))
            };
            button(SharedString::from(format!("hit-{at}")), label)
                .aria_selected(current)
                .flex_col()
                .items_start()
                .justify_center()
                .flex_none()
                .h(px(if hit.place.is_empty() { 32. } else { 42. }))
                .rounded(px(radius_sm()))
                .px(px(8.))
                .map(|b| {
                    if current {
                        b.bg(selected)
                    } else {
                        b.hover(move |s| s.bg(hover))
                    }
                })
                .child(div().w_full().truncate().child(hit.name))
                .when(!hit.place.is_empty(), |b| {
                    b.child(
                        div()
                            .w_full()
                            .truncate()
                            .text_size(px(fs_sm()))
                            .text_color(muted)
                            .child(hit.place),
                    )
                })
                .on_click(cx.listener(move |d, _, window, cx| d.open_hit(at, window, cx)))
        });
        div()
            .id("settings-results")
            .role(Role::List)
            .aria_label("Search results")
            .flex()
            .flex_col()
            .gap(px(2.))
            .children(rows.collect::<Vec<_>>())
            .into_any_element()
    }

    /// The entries. One with a single pane opens it; one with more shows them under itself until
    /// it is clicked shut.
    fn entries(&mut self, t: &Theme, rows: &mut Vec<AnyElement>, cx: &mut Context<Self>) {
        let section = self.section;
        let (hover, selected, muted) = (t.bg_hover, t.bg_selected, t.text_muted);
        let selected_hover = mix(t.bg_selected, t.text, 92.);
        for (at, e) in ENTRIES.iter().enumerate() {
            let open = e.panes.iter().any(|p| p.id == section);
            let group = e.panes.len() > 1;
            let row = button(SharedString::from(format!("section-{at}")), e.label)
                .flex_none()
                .h(px(32.))
                .gap(px(10.))
                .rounded(px(radius_sm()))
                .px(px(8.))
                .text_color(t.text)
                .child(icon(e.icon, 18.))
                .child(div().flex_1().min_w_0().child(e.label))
                .on_click(cx.listener(move |d, _, _, cx| d.toggle_entry(at, cx)));
            let shown = self.expanded.contains(&at);
            let row = if group {
                let caret = if shown { "caret-down" } else { "caret-right" };
                // Closed over the pane showing, the entry stands for it.
                let marks = open && !shown;
                row.aria_expanded(shown)
                    .aria_selected(marks)
                    .map(|b| {
                        if marks {
                            b.bg(selected).hover(move |s| s.bg(selected_hover))
                        } else {
                            b.hover(move |s| s.bg(hover))
                        }
                    })
                    .child(div().text_color(muted).child(icon(caret, 12.)))
            } else {
                let focus = self
                    .rows
                    .entry(e.panes[0].id)
                    .or_insert_with(|| cx.focus_handle())
                    .clone();
                row.track_focus(&focus).aria_selected(open).map(|b| {
                    if open {
                        b.bg(selected).hover(move |s| s.bg(selected_hover))
                    } else {
                        b.hover(move |s| s.bg(hover))
                    }
                })
            };
            // The entry and its panes are one item of the list, so panes that are shut add no gap
            // to it and the entries under them stay where they are.
            let mut parts = vec![row.into_any_element()];
            let full = e.panes.len() as f32 * (SUB_ROW + 2.);
            if group && self.closing == Some(at) {
                // The rows going away are only drawn: nothing in them takes focus or a click.
                let names = e.panes.iter().map(|p| {
                    div()
                        .flex()
                        .flex_none()
                        .h(px(SUB_ROW))
                        .items_center()
                        .pl(px(36.))
                        .text_color(muted)
                        .child(p.label)
                });
                parts.push(
                    div()
                        .flex()
                        .flex_col()
                        .flex_none()
                        .pt(px(2.))
                        .gap(px(2.))
                        .overflow_hidden()
                        .children(names.collect::<Vec<_>>())
                        .with_animation(
                            SharedString::from(format!("panes-closing-{at}")),
                            slide(),
                            move |rows, v| rows.h(px(full * (1. - v))).opacity(1. - v),
                        )
                        .into_any_element(),
                );
            }
            if !(group && shown) {
                rows.push(entry_group(parts));
                continue;
            }
            let mut subs = Vec::with_capacity(e.panes.len());
            for p in e.panes {
                let id = p.id;
                let current = id == section;
                let focus = self
                    .rows
                    .entry(id)
                    .or_insert_with(|| cx.focus_handle())
                    .clone();
                let sub = button(SharedString::from(format!("pane-{id}")), p.label)
                    .track_focus(&focus)
                    .aria_selected(current)
                    .flex_none()
                    .h(px(SUB_ROW))
                    .rounded(px(radius_sm()))
                    // Under the entry's name, past its icon.
                    .pl(px(36.))
                    .pr(px(8.))
                    .map(|b| {
                        if current {
                            b.bg(selected)
                                .text_color(t.text)
                                .hover(move |s| s.bg(selected_hover))
                        } else {
                            b.text_color(muted)
                                .hover(move |s| s.bg(hover).text_color(t.text))
                        }
                    })
                    .child(p.label)
                    .on_click(cx.listener(move |d, _, _, cx| d.open_section(id, cx)));
                subs.push(sub);
            }
            let panes = div()
                .flex()
                .flex_col()
                .flex_none()
                .pt(px(2.))
                .gap(px(2.))
                .overflow_hidden()
                .children(subs);
            // Only the entry just opened slides; the rest stand as they are.
            parts.push(if self.opening == Some(at) {
                panes
                    .with_animation(
                        SharedString::from(format!("panes-{at}")),
                        slide(),
                        move |rows, v| rows.h(px(full * v)).opacity(v),
                    )
                    .into_any_element()
            } else {
                panes.into_any_element()
            });
            rows.push(entry_group(parts));
        }
    }
}

/// An entry's row with its panes under it, as one item of the list.
fn entry_group(parts: Vec<AnyElement>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .children(parts)
        .into_any_element()
}
