//! The Settings list: its search at the top, then either the pages under their group headers or
//! what the search finds.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, Entity, Focusable, FontWeight, Role, SharedString, Window, div, prelude::*, px};
use patchwork_design_editor::{Arrow, Changed, Submitted, TextArea};
use patchwork_design_icons::IconName;
use patchwork_design_theme::{Theme, fs_md, fs_xs, input_placeholder, radius, radius_sm, theme};
use patchwork_design_components::{button, icon_button, mix};
use patchwork_design_icons::icon;
use super::controls::{center_when_drawn, seek};
use super::dialog::SettingsDialog;
use super::pages::{PAGES, Page};
use super::shortcuts;

/// The most results the search lists: enough to hold every page of a common word, few enough to
/// read at a glance.
const MAX_HITS: usize = 12;

/// One search result: a page, or a setting on it, or a shortcut command.
#[derive(Clone, Copy)]
pub(super) struct Hit {
    page: &'static Page,
    /// The setting's name; None when the result is the page itself.
    setting: Option<&'static str>,
    /// The shortcut command the result is, opened picked on the Shortcuts page.
    command: Option<&'static str>,
}

/// What `query`, in lower case, finds: the pages whose name has it, then the settings, then the
/// shortcut commands.
pub(super) fn hits(query: &str) -> Vec<Hit> {
    let has = |text: &str| text.to_lowercase().contains(query);
    let pages = PAGES.iter().filter(move |p| has(p.label)).map(|page| Hit {
        page,
        setting: None,
        command: None,
    });
    let settings = PAGES.iter().flat_map(move |page| {
        page.words
            .iter()
            // A word that is the page's own name is already the page's result.
            .filter(move |w| has(w) && !w.eq_ignore_ascii_case(page.label))
            .map(move |w| Hit {
                page,
                setting: Some(w),
                command: None,
            })
    });
    // Commands come last: a setting named the same is more likely what is looked for.
    let shortcuts_page = PAGES.iter().find(|p| p.id == "shortcuts");
    let commands = shortcuts::searchable()
        .filter(move |(_, title)| has(title))
        .filter_map(move |(id, name)| {
            Some(Hit {
                page: shortcuts_page?,
                setting: Some(name),
                command: Some(id),
            })
        });
    pages
        .chain(settings)
        .chain(commands)
        .take(MAX_HITS)
        .collect()
}

/// Every page's id, in the order the list shows them.
pub(super) fn page_order() -> impl Iterator<Item = &'static str> {
    PAGES.iter().map(|p| p.id)
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

    /// Opens the page of the result at `at` and empties the search. A setting's row is scrolled
    /// to the middle of the page and flashed, so the eye finds it.
    pub(super) fn open_hit(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(hit) = hits(&self.query(cx)).get(at).copied() else {
            return;
        };
        self.clear_search(cx);
        self.open_section(hit.page.id, cx);
        self.focus_row(hit.page.id, window, cx);
        if let Some(id) = hit.command {
            self.show_command(id, cx);
            return;
        }
        if let Some(name) = hit.setting {
            seek(Some((name, theme(cx).bg_accent_soft)));
            center_when_drawn(self.scroll.clone(), window);
        }
    }

    pub(super) fn clear_search(&mut self, cx: &mut Context<Self>) {
        if let Some(search) = &self.search {
            search.update(cx, |area, cx| area.set_text("", cx));
        }
        self.search_at = 0;
        cx.notify();
    }

    /// Focuses the list's row for the page `id`.
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
        window.focus(&row, cx);
    }

    /// The search field, made and focused the first time the list is drawn, so typing at once
    /// searches.
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
            cx.notify();
        })
        .detach();
        cx.subscribe(&search, |d, _, step: &Arrow, cx| d.step_hit(step.0, cx))
            .detach();
        cx.subscribe_in(&search, window, |d, _, _: &Submitted, window, cx| {
            d.open_hit(d.search_at, window, cx);
        })
        .detach();
        let focus = search.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        self.search = Some(search.clone());
        search
    }

    /// The search field at the top of the list, with an X that empties it.
    pub(super) fn nav_field(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = *theme(cx);
        let search = self.nav_search(window, cx);
        let focused = search.read(cx).focus_handle(cx).is_focused(window);
        let typed = !self.query(cx).is_empty();
        div()
            .flex()
            .flex_none()
            .h(px(32.))
            .items_center()
            .gap(px(8.))
            .rounded(px(radius()))
            .border_1()
            .border_color(if focused { t.border_strong } else { t.border })
            .bg(t.bg_input)
            .px(px(10.))
            .text_size(px(fs_md()))
            .child(
                div()
                    .text_color(t.text_subtle)
                    .child(icon(IconName::MagnifyingGlass, 14.)),
            )
            .child(div().flex_1().min_w_0().child(search))
            .when(typed, |d| {
                d.child(
                    icon_button(
                        "settings-search-clear",
                        "Clear the search",
                        IconName::X,
                        12.,
                        cx,
                    )
                    .on_click(cx.listener(|d, _, window, cx| {
                        d.clear_search(cx);
                        if let Some(search) = &d.search {
                            let focus = search.read(cx).focus_handle(cx);
                            window.focus(&focus, cx);
                        }
                    })),
                )
            })
            .into_any_element()
    }

    /// The list under the search: the results while something is typed, else the pages.
    pub(super) fn nav_list(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let t = *theme(cx);
        let query = self.query(cx);
        if query.is_empty() {
            self.pages(&t, cx)
        } else {
            vec![self.results(&query, &t, cx)]
        }
    }

    fn results(&self, query: &str, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let found = hits(query);
        if found.is_empty() {
            return div()
                .id("settings-no-results")
                .role(Role::Status)
                .px(px(8.))
                .py(px(24.))
                .text_center()
                .text_size(px(fs_md()))
                .text_color(t.text_subtle)
                .child("No settings match")
                .into_any_element();
        }
        let (hover, fg, muted, subtle) = (t.bg_hover, t.text, t.text_muted, t.text_subtle);
        let rows = found.into_iter().enumerate().map(|(at, hit)| {
            let current = at == self.search_at;
            let (name, under) = match hit.setting {
                Some(setting) => (setting, hit.page.label),
                None => (hit.page.label, hit.page.group),
            };
            button(
                SharedString::from(format!("hit-{at}")),
                SharedString::from(format!("{name}, {under}")),
            )
            .role(Role::ListBoxOption)
            .aria_selected(current)
            .flex_none()
            .w_full()
            .gap(px(10.))
            .rounded(px(radius()))
            .px(px(8.))
            .py(px(6.))
            .map(|b| {
                if current {
                    b.bg(hover).text_color(fg)
                } else {
                    b.text_color(muted)
                }
            })
            .on_hover(cx.listener(move |d, over: &bool, _, cx| {
                if *over && d.search_at != at {
                    d.search_at = at;
                    cx.notify();
                }
            }))
            .child(div().text_color(subtle).child(icon(hit.page.icon, 15.)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(div().truncate().text_size(px(fs_md())).child(name))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(fs_xs()))
                            .text_color(subtle)
                            .child(under),
                    ),
            )
            .on_click(cx.listener(move |d, _, window, cx| d.open_hit(at, window, cx)))
        });
        div()
            .id("settings-results")
            .role(Role::ListBox)
            .aria_label("Matches")
            .flex()
            .flex_col()
            .children(rows)
            .into_any_element()
    }

    /// The pages under their group headers. The open page's row is lit and its icon filled.
    fn pages(&mut self, t: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let section = self.section;
        let (hover, fg, muted, subtle) = (t.bg_hover, t.text, t.text_muted, t.text_subtle);
        // The web list lightens a row only partly under the pointer, so the open row still stands out.
        let soft_hover = mix(t.bg_hover, t.bg_app, 60.);
        let mut rows = Vec::with_capacity(PAGES.len() + 4);
        for (at, page) in PAGES.iter().enumerate() {
            if at == 0 || PAGES[at - 1].group != page.group {
                rows.push(
                    div()
                        .flex_none()
                        .px(px(8.))
                        .pt(px(if at == 0 { 8. } else { 16. }))
                        .pb(px(4.))
                        .text_size(px(fs_xs()))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(subtle)
                        .child(page.group)
                        .into_any_element(),
                );
            }
            let id = page.id;
            let on = id == section;
            let focus = self
                .rows
                .entry(id)
                .or_insert_with(|| cx.focus_handle())
                .clone();
            rows.push(
                button(SharedString::from(format!("page-{id}")), page.label)
                    .track_focus(&focus)
                    .aria_selected(on)
                    .flex_none()
                    .h(px(32.))
                    .gap(px(10.))
                    .rounded(px(radius_sm()))
                    .px(px(8.))
                    .text_size(px(fs_md()))
                    .map(|b| {
                        if on {
                            b.bg(hover).text_color(fg)
                        } else {
                            b.text_color(muted)
                                .hover(move |s| s.bg(soft_hover).text_color(fg))
                        }
                    })
                    .child(
                        div()
                            .text_color(if on { t.accent_text } else { subtle })
                            .child(icon(if on { page.icon_on } else { page.icon }, 16.)),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(page.label))
                    .on_click(cx.listener(move |d, _, _, cx| d.open_section(id, cx)))
                    .into_any_element(),
            );
        }
        rows
    }
}
