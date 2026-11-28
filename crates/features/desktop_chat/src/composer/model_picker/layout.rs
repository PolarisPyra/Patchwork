//! The picker drawn.
//!
//! The search field is across the top, a single row of swatches under it with one per agent and
//! one for the starred models, the swatch's models listed below a stitched seam, each with its
//! Ctrl+number at its left, and the effort slider at its foot.

// Rust guideline compliant 2026-10-09

use patchwork_views_logos::{agent_label, agent_logo};
use gpui::{
    AnyElement, Focusable, MouseButton, MouseDownEvent, Role, SharedString, Window, div,
    prelude::*, px,
};
use patchwork_model::provider::AgentKind;
use patchwork_design_icons::IconName;
use patchwork_design_theme::{Theme, font_mono, fs, fs_md, fs_xs, radius};
use patchwork_design_components::{button, tooltip};
use patchwork_design_icons::icon;

use super::nav::Tab;
use super::rows::{Heading, ListRow, source_of};
use crate::composer::Composer;
use crate::composer::menus::Pick;

/// The card's width.
const WIDTH: f32 = 460.;
/// The list's height, which scrolls rather than grows.
const BODY: f32 = 300.;
/// A swatch's height.
const SWATCH: f32 = 28.;
/// A model row's height.
const ROW: f32 = 32.;
/// The rows that Ctrl+1 to Ctrl+9 reach.
const NUMBERED: usize = 9;

/// What a row needs to know of the picker as it is drawn.
struct RowState<'a> {
    current: (AgentKind, Option<&'a str>),
    index: usize,
    favorites: &'a [String],
    /// Whether each row says whose it is, as when the list mixes agents.
    logos: bool,
}

impl Composer {
    /// The picker card, its rows' picks already in `self.menu_picks` in order for Ctrl+1 to
    /// Ctrl+9. `current` is the agent, model and effort the picks are for.
    pub(in crate::composer) fn model_picker(
        &mut self,
        agents: &[AgentKind],
        current: (AgentKind, Option<&str>, Option<&str>),
        t: &Theme,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let agents = super::open::picker_agents(agents, current.0);
        let tabs = self.picker_tabs(&agents);
        // Unstarring the last starred model takes its tab away.
        if let Some(picker) = &mut self.picker
            && !tabs.contains(&picker.tab)
            && let Some(&first) = tabs.first()
        {
            picker.tab = first;
            picker.index = 0;
        }
        let rows = self.picker_rows(&agents, cx);
        self.menu_picks = rows
            .iter()
            .map(|r| Pick::Model(r.row.kind, r.row.item.id.clone()))
            .collect();
        let (agent, model, effort) = current;
        let levels = agent.model_efforts(model, self.models_of(agent));
        let meter = self.effort_meter(&levels, effort, t, cx);
        let Some(picker) = &mut self.picker else {
            return div().into_any_element();
        };
        // Typing or unstarring can shrink the list under the keys.
        picker.index = picker.index.min(rows.len().saturating_sub(1));
        let search = picker.search.clone();
        let searching = !search.read(cx).content.trim().is_empty();
        let tab = picker.tab;
        let dialog = picker.dialog.clone();
        let scroll = picker.scroll.clone();
        let favorites = picker.favorites.clone();
        let search_focus = search.read(cx).focus_handle(cx);
        let state = RowState {
            current: (agent, model),
            index: picker.index,
            favorites: &favorites,
            logos: searching || tab == Tab::Favorites,
        };
        let spotted_starred = rows
            .get(state.index)
            .map(|r| favorites.contains(&r.row.key));

        let list: Vec<AnyElement> = rows
            .iter()
            .enumerate()
            .map(|(i, row)| row_of(i, row, &state, t, cx))
            .collect();
        let empty = rows.is_empty().then(|| {
            div()
                .px(px(14.))
                .py(px(24.))
                .text_center()
                .text_size(px(fs_xs()))
                .text_color(t.text_subtle)
                .child(if searching {
                    "Nothing matches"
                } else {
                    "No models yet"
                })
        });
        let swatches: Vec<AnyElement> = tabs
            .iter()
            .map(|&each| swatch_of(each, !searching && each == tab, t, cx))
            .collect();
        let star = spotted_starred.map(|starred| {
            let (fg, hover) = (t.text_subtle, t.bg_hover);
            let label = if starred {
                "Unstar the model (Ctrl+D)"
            } else {
                "Star the model (Ctrl+D)"
            };
            button("model-star", label)
                .size(px(24.))
                .flex_none()
                .justify_center()
                .rounded(px(6.))
                .text_color(if starred { t.accent_text } else { fg })
                .hover(move |s| s.bg(hover))
                .tooltip(tooltip(label))
                .child(icon(
                    if starred {
                        IconName::StarFill
                    } else {
                        IconName::Star
                    },
                    14.,
                ))
                .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                .on_click(cx.listener(|this, _, _, cx| this.star_spot(cx)))
        });

        div()
            .id("model-picker")
            .role(Role::Dialog)
            .aria_label("Choose model")
            .occlude()
            .track_focus(&dialog)
            .on_action(
                cx.listener(|this, _: &patchwork_views_actions::FocusNext, window, cx| {
                    this.picker_tab(true, window, cx);
                }),
            )
            .on_action(
                cx.listener(|this, _: &patchwork_views_actions::FocusPrev, window, cx| {
                    this.picker_tab(false, window, cx);
                }),
            )
            .on_mouse_down_out(cx.listener(|this, e: &MouseDownEvent, window, cx| {
                this.dismissed = this.menu.take().map(|m| (m, e.position));
                this.picker = None;
                this.effort_drag = None;
                this.restore_focus(window, cx);
                cx.notify();
            }))
            .on_key_down(cx.listener(Self::picker_key))
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(radius()))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_elevated)
            .text_color(t.text)
            .shadow(patchwork_design_components::popup_shadow(t))
            .child(
                div()
                    .h(px(44.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pl(px(16.))
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
                    )
                    .children(star),
            )
            .child(
                div()
                    .id("model-swatches")
                    .role(Role::TabList)
                    .aria_label("Providers")
                    .flex_none()
                    .overflow_x_scroll()
                    .flex()
                    .gap(px(4.))
                    .px(px(10.))
                    .py(px(10.))
                    .children(swatches),
            )
            .child(
                div()
                    .id("model-list")
                    .role(Role::ListBox)
                    .aria_label("Models")
                    .track_scroll(&scroll)
                    .h(px(BODY))
                    .flex_none()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .p(px(6.))
                    // The seam between the swatches and their models, stitched.
                    .border_t_1()
                    .border_dashed()
                    .border_color(t.border_strong)
                    .children(list)
                    .children(empty),
            )
            .children(meter)
            .into_any_element()
    }
}

/// A swatch: the agent's logo, or a star for the starred models, filled while its models are
/// listed.
fn swatch_of(each: Tab, open: bool, t: &Theme, cx: &mut Context<Composer>) -> AnyElement {
    let (id, name, mark) = match each {
        Tab::Favorites => (
            SharedString::from("model-tab-favorites"),
            "Starred",
            icon(IconName::StarFill, 14.)
                .text_color(t.accent_text)
                .into_any_element(),
        ),
        Tab::Agent(kind) => (
            SharedString::from(format!("model-tab-{}", agent_label(kind))),
            agent_label(kind),
            agent_logo(kind, None, 14.),
        ),
    };
    let (hover, fg) = (t.bg_hover, t.text);
    button(id, name)
        .role(Role::Tab)
        .aria_selected(open)
        .size(px(SWATCH))
        .flex_none()
        .justify_center()
        .rounded(px(6.))
        .when(open, |b| b.bg(t.bg_hover).text_color(t.text))
        .when(!open, |b| {
            b.text_color(t.text_muted)
                .hover(move |s| s.bg(hover).text_color(fg))
        })
        .child(mark)
        // Named on hover only, so every swatch keeps one size and they fit on one line.
        .tooltip(tooltip(name))
        // The search field keeps the keys through a press on a swatch.
        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
        .on_click(cx.listener(move |this, _, window, cx| {
            // A swatch shows its own models, so a search in progress is let go.
            if let Some(search) = this.picker.as_ref().map(|p| p.search.clone()) {
                search.update(cx, |field, cx| {
                    if !field.content.is_empty() {
                        field.clear(window, cx);
                    }
                });
            }
            this.open_tab(each, cx);
        }))
        .into_any_element()
}

/// A heading above a row: the agent whose matches follow, or the agent's older models.
fn heading_of(heading: Heading, first: bool, t: &Theme) -> AnyElement {
    let (logo, name) = match heading {
        Heading::Agent(kind) => (Some(agent_logo(kind, None, 12.)), agent_label(kind)),
        Heading::Older => (None, "Older models"),
    };
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(10.))
        .pt(px(if first { 4. } else { 12. }))
        .pb(px(4.))
        .text_size(px(fs_xs()))
        .text_color(t.text_subtle)
        .children(logo)
        .child(name)
        .into_any_element()
}

/// A model's row: its name and where it comes from, a check when it is the chat's model, and a
/// star that shows on hover or stays lit once starred.
fn row_of(
    i: usize,
    list_row: &ListRow,
    state: &RowState<'_>,
    t: &Theme,
    cx: &mut Context<Composer>,
) -> AnyElement {
    let row = &list_row.row;
    let item = &row.item;
    let selected = row.kind == state.current.0 && item.id.as_deref() == state.current.1;
    let spotted = i == state.index;
    let starred = state.favorites.contains(&row.key);
    let needs_cli = item
        .needs_cli
        .as_ref()
        .map(|min| format!("Needs {} {min}+", agent_label(row.kind)));
    // An API model's provider, or the model a CLI's default runs, beside the name; a CLI model's
    // id is left to the tooltip.
    let detail =
        (item.id.is_none() || row.kind == AgentKind::Api).then(|| source_of(row.kind, item));
    let mut tip = source_of(row.kind, item);
    if item.is_legacy {
        tip = format!("{tip} (older model)");
    }
    if let Some(note) = &needs_cli {
        tip = format!("{tip}. {note}");
    }
    let logo = (state.logos || row.kind == AgentKind::Api).then(|| {
        agent_logo(
            row.kind,
            item.id.as_deref().or(item.default_model.as_deref()),
            14.,
        )
    });
    let fg = if selected || spotted {
        t.text
    } else if item.is_legacy {
        t.text_subtle
    } else {
        t.text_muted
    };
    let group = SharedString::from(format!("model-row-{i}"));
    let pick = Pick::Model(row.kind, item.id.clone());
    let (star_key, menu_key) = (row.key.clone(), row.key.clone());
    let hover = t.bg_hover;
    let star = div()
        .id(SharedString::from(format!("model-star-{i}")))
        .size(px(22.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .text_color(if starred {
            t.accent_text
        } else {
            t.text_subtle
        })
        .when(!starred && !spotted, |s| {
            s.opacity(0.).group_hover(group.clone(), |s| s.opacity(1.))
        })
        .hover(move |s| s.bg(hover))
        .child(icon(
            if starred {
                IconName::StarFill
            } else {
                IconName::Star
            },
            12.,
        ))
        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.toggle_favorite(&star_key, cx);
        }));
    let line = div()
        .id(SharedString::from(format!("model-{i}-{}", row.key)))
        .group(group)
        .role(Role::ListBoxOption)
        .aria_label(SharedString::from(item.label.clone()))
        .aria_selected(selected)
        .h(px(ROW))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.))
        .pl(px(6.))
        .pr(px(4.))
        .rounded(px(6.))
        .when(spotted, |r| r.bg(t.bg_hover))
        .text_size(px(fs(13.)))
        .text_color(fg)
        .cursor_pointer()
        .tooltip(tooltip(tip))
        .child(
            div()
                .w(px(16.))
                .flex_none()
                .text_center()
                .font_family(font_mono())
                .text_size(px(fs_xs()))
                .text_color(if spotted { t.text_muted } else { t.text_subtle })
                .children((i < NUMBERED).then(|| (i + 1).to_string())),
        )
        .children(logo)
        .child(
            div()
                .flex_none()
                .max_w(px(200.))
                .truncate()
                .child(item.label.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(fs_xs()))
                .text_color(t.text_subtle)
                .children(detail),
        )
        .when(needs_cli.is_some(), |r| {
            r.child(
                icon(IconName::Warning, 12.)
                    .flex_none()
                    .text_color(t.warning),
            )
        })
        .when(selected, |r| {
            r.child(
                icon(IconName::CheckBold, 12.)
                    .flex_none()
                    .text_color(t.accent_text),
            )
        })
        .child(star)
        // The pointer and the keys share one row.
        .on_mouse_move(cx.listener(move |this, _, _, cx| {
            if let Some(picker) = &mut this.picker
                && picker.index != i
            {
                picker.index = i;
                cx.notify();
            }
        }))
        // The search field keeps the keys through a press on a row.
        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
        .on_click(cx.listener(move |this, _, _, cx| this.commit(pick.clone(), cx)))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.toggle_favorite(&menu_key, cx);
            }),
        );
    // The heading and its row are one child of the list, so the scroll can follow the keys by
    // row.
    div()
        .flex_none()
        .flex()
        .flex_col()
        .children(list_row.heading.map(|h| heading_of(h, i == 0, t)))
        .child(line)
        .into_any_element()
}
