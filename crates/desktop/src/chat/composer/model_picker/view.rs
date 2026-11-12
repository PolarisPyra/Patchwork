//! The picker drawn: the providers down the left, the search field over the models, and the Legacy models panel.

// Rust guideline compliant 2026-10-09

use super::rows::WIDTH;
use super::rows::HEIGHT;
use super::rows::LEGACY_WIDTH;
use super::rows::source_of;
use super::rows::ModelRow;
use gpui::{
    AnyElement, Focusable, FontWeight, MouseDownEvent, Role, SharedString, Window, div, prelude::*,
    px,
};
use patchwork_server::provider::AgentKind;
use crate::chat::composer::Composer;
use crate::chat::composer::menus::Pick;
use crate::ui::theme::{FS_SM, Theme};
use crate::ui::{agent_label, agent_logo, button, icon, popup_shadow};

impl Composer {
    /// The picker over the window, its rows already in `self.menu_picks` for the keyboard.
    pub(in crate::chat::composer) fn model_picker(
        &mut self,
        agents: &[AgentKind],
        current: (AgentKind, Option<&str>),
        t: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rows = self.model_rows(agents, cx);
        self.menu_picks = rows
            .iter()
            .map(|r| Pick::Model(r.kind, r.item.id.clone()))
            .collect();
        // Unstarring the last favourite shortens the list under the highlight.
        if self.picker.is_some() {
            self.highlight = self.highlight.min(self.menu_picks.len().saturating_sub(1));
        }
        let Some(picker) = &self.picker else {
            return div().into_any_element();
        };
        let searching = !picker.search.read(cx).content.trim().is_empty();
        let provider = picker.provider;
        let favorites = picker.favorites.clone();
        let legacy_toggle = (!searching).then_some(provider).flatten();
        let legacy = legacy_toggle
            .map(|kind| self.legacy_rows(kind))
            .unwrap_or_default();
        let legacy_open = legacy_toggle.filter(|k| picker.legacy_open == Some(*k));
        let search_focused = picker.search.read(cx).focus_handle(cx).is_focused(window);
        let search = picker.search.clone();
        let highlight = self.highlight;

        let model_row = |row: &ModelRow, index: Option<usize>, cx: &mut Context<Self>| {
            let selected = row.kind == current.0 && row.item.id.as_deref() == current.1;
            let favorite = favorites.contains(&row.key);
            let pick = Pick::Model(row.kind, row.item.id.clone());
            let key = row.key.clone();
            let label = row.item.label.clone();
            let hover = t.bg_hover;
            let shortcut = index.filter(|&i| i < 9).map(|i| {
                div()
                    .flex_none()
                    .rounded(px(4.))
                    .bg(t.text_tint_6)
                    .px(px(4.))
                    .py(px(2.))
                    .text_size(px(FS_SM))
                    .line_height(px(16.))
                    .text_color(t.text_muted)
                    .child(format!("Ctrl+{}", i + 1))
            });
            let (star_fg, star_hover) = if favorite {
                (t.accent_text, t.accent_text)
            } else {
                (t.text_subtle, t.text)
            };
            div()
                .id(SharedString::from(format!("model-{}", row.key)))
                .role(Role::ListBoxOption)
                .aria_label(label.clone())
                .aria_selected(selected)
                .tab_index(0)
                .mb(px(2.))
                .min_h(px(44.))
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded(px(4.))
                .px(px(8.))
                .py(px(4.))
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .when(index.is_some_and(|i| i == highlight), |r| r.bg(hover))
                .when_some(index, |r, i| {
                    r.on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if this.highlight != i {
                            this.highlight = i;
                            cx.notify();
                        }
                    }))
                })
                .on_click(
                    cx.listener(move |this, _, window, cx| this.apply(pick.clone(), window, cx)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .gap(px(4.))
                        .text_size(px(FS_SM))
                        .line_height(px(16.5))
                        .child(
                            div()
                                .truncate()
                                .font_weight(FontWeight::MEDIUM)
                                .child(label.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .text_color(t.text_subtle)
                                .child(agent_logo(
                                    row.kind,
                                    row.item.id.as_deref().or(row.item.default_model.as_deref()),
                                    12.,
                                ))
                                .child(div().truncate().child(source_of(row.kind, &row.item))),
                        ),
                )
                .children(shortcut)
                .child(
                    button(
                        SharedString::from(format!("star-{}", row.key)),
                        if favorite {
                            format!("Remove {label} from favorites")
                        } else {
                            format!("Add {label} to favorites")
                        },
                    )
                    .size(px(24.))
                    .flex_none()
                    .justify_center()
                    .rounded(px(4.))
                    .mr(px(-4.))
                    .text_color(star_fg)
                    .hover(move |s| s.bg(hover).text_color(star_hover))
                    .child(icon(if favorite { "star-fill" } else { "star" }, 12.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.toggle_favorite(&key, cx);
                    })),
                )
        };

        let rail = (!searching).then(|| {
            let rail_button = |id: SharedString,
                               label: String,
                               pressed: bool,
                               logo: AnyElement,
                               target: Option<AgentKind>,
                               cx: &mut Context<Self>| {
                let hover = t.bg_hover;
                button(id, label)
                    .relative()
                    .size(px(36.))
                    .flex_none()
                    .justify_center()
                    .rounded(px(6.))
                    .text_color(t.text)
                    .hover(move |s| s.bg(hover))
                    .aria_toggled(if pressed {
                        gpui::Toggled::True
                    } else {
                        gpui::Toggled::False
                    })
                    .child(logo)
                    .when(pressed, |b| {
                        b.child(
                            div()
                                .absolute()
                                .top(px(8.))
                                .right(px(-4.))
                                .w(px(3.))
                                .h(px(20.))
                                .rounded_l(px(3.))
                                .bg(t.accent),
                        )
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(picker) = &mut this.picker {
                            picker.provider = target;
                            let focus = picker.search.read(cx).focus_handle(cx);
                            window.focus(&focus, cx);
                        }
                        cx.notify();
                    }))
            };
            let mut nav = div()
                .id("providers")
                .role(Role::Navigation)
                .aria_label("Providers")
                .flex()
                .flex_col()
                .flex_none()
                .w(px(44.))
                .gap(px(4.))
                .p(px(4.))
                .bg(t.bg_app.opacity(0.3))
                .child(rail_button(
                    "rail-favorites".into(),
                    "Favorites".into(),
                    provider.is_none(),
                    icon("star-fill", 20.).into_any_element(),
                    None,
                    cx,
                ))
                .child(div().h(px(1.)).flex_none().bg(t.border));
            for &kind in agents {
                nav = nav.child(rail_button(
                    SharedString::from(format!("rail-{}", agent_label(kind))),
                    agent_label(kind).into(),
                    provider == Some(kind),
                    agent_logo(kind, None, 20.),
                    Some(kind),
                    cx,
                ));
            }
            nav
        });

        let list_rows: Vec<_> = rows
            .iter()
            .enumerate()
            .map(|(i, row)| model_row(row, Some(i), cx))
            .collect();
        let legacy_toggle_row = legacy_toggle.filter(|_| !legacy.is_empty()).map(|kind| {
            let hover = t.bg_hover;
            button("legacy-toggle", "Legacy models")
                .aria_expanded(legacy_open.is_some())
                .mb(px(2.))
                .min_h(px(44.))
                .w_full()
                .gap(px(6.))
                .rounded(px(4.))
                .px(px(8.))
                .py(px(4.))
                .hover(move |s| s.bg(hover))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .gap(px(4.))
                        .text_size(px(FS_SM))
                        .line_height(px(16.5))
                        .child(div().font_weight(FontWeight::MEDIUM).child("Legacy models"))
                        .child(
                            div()
                                .text_color(t.text_subtle)
                                .child(legacy.len().to_string()),
                        ),
                )
                .child(icon("caret-right", 14.).text_color(t.text_subtle))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(picker) = &mut this.picker {
                        picker.legacy_open = if picker.legacy_open == Some(kind) {
                            None
                        } else {
                            Some(kind)
                        };
                        let focus = picker.search.read(cx).focus_handle(cx);
                        window.focus(&focus, cx);
                    }
                    cx.notify();
                }))
        });
        let empty = (rows.is_empty() && legacy_toggle_row.is_none()).then(|| {
            div()
                .p(px(8.))
                .text_center()
                .text_size(px(14.))
                .text_color(t.text_subtle)
                .child("No models found")
        });
        let flyout = legacy_open.map(|_| {
            let legacy_rows: Vec<_> = legacy.iter().map(|row| model_row(row, None, cx)).collect();
            div()
                .id("legacy-models")
                .role(Role::ListBox)
                .aria_label("Legacy models")
                .flex()
                .flex_col()
                .w(px(LEGACY_WIDTH))
                .h_full()
                .overflow_hidden()
                .rounded(px(8.))
                .border_1()
                .border_color(t.border_strong)
                .bg(t.bg_elevated)
                .shadow(popup_shadow(t))
                .child(
                    div()
                        .px(px(12.))
                        .pt(px(12.))
                        .pb(px(6.))
                        .text_size(px(FS_SM))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.text_subtle)
                        .child("Legacy models"),
                )
                .child(
                    div()
                        .id("legacy-list")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .px(px(8.))
                        .pb(px(6.))
                        .children(legacy_rows),
                )
        });

        div()
            .id("model-picker")
            .occlude()
            .when_some(
                self.picker.as_ref().map(|p| p.dialog.clone()),
                |d, dialog| d.track_focus(&dialog),
            )
            .on_action(cx.listener(|this, _: &crate::FocusNext, window, cx| {
                this.picker_tab(true, window, cx);
            }))
            .on_action(cx.listener(|this, _: &crate::FocusPrev, window, cx| {
                this.picker_tab(false, window, cx);
            }))
            .flex()
            .gap(px(6.))
            .h(px(HEIGHT))
            .on_mouse_down_out(cx.listener(|this, e: &MouseDownEvent, window, cx| {
                this.dismissed = this.menu.take().map(|m| (m, e.position));
                this.restore_focus(window, cx);
                this.picker = None;
                cx.notify();
            }))
            .on_key_down(cx.listener(Self::picker_key))
            .child(
                div()
                    .id("model-panel")
                    .role(Role::Dialog)
                    .aria_label("Choose model")
                    .flex()
                    .w(px(WIDTH))
                    .h_full()
                    .overflow_hidden()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(t.border_strong)
                    .bg(t.bg_elevated)
                    .text_color(t.text)
                    .shadow(popup_shadow(t))
                    .children(rail)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .overflow_hidden()
                            .when(!searching, |c| c.border_l_1().border_color(t.border))
                            .child(
                                div()
                                    .mx(px(12.))
                                    .mt(px(9.))
                                    .h(px(34.))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .border_b_1()
                                    .border_color(if search_focused { t.accent } else { t.border })
                                    .pb(px(6.))
                                    .text_color(t.text_subtle)
                                    .child(icon("magnifying-glass", 16.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .child(search),
                                    ),
                            )
                            .child(
                                div()
                                    .id("model-options")
                                    .role(Role::ListBox)
                                    .aria_label("Models")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .py(px(6.))
                                    .px(px(8.))
                                    .children(list_rows)
                                    .children(legacy_toggle_row)
                                    .children(empty),
                            ),
                    ),
            )
            .children(flyout)
            .into_any_element()
    }
}
