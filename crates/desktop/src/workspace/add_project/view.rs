//! The dialog drawn: the path field over the list of folders.

// Rust guideline compliant 2026-10-09

use super::state::Closed;
use super::state::AddProject;
use gpui::{
    FocusHandle, Focusable, FontWeight, MouseButton, Role, SharedString, Window, div, point,
    prelude::*, px,
};
use crate::ui::dialog;
use crate::ui::theme::{font_mono, fs_md, fs_sm, fs_xs, radius, theme};
use crate::ui::{BtnKind, btn, icon, icon_button, spinner, tooltip};

impl Focusable for AddProject {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for AddProject {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        if std::mem::take(&mut self.refocus) {
            if self.browsing {
                window.focus(&self.field.read(cx).focus_handle(cx), cx);
            } else {
                window.focus(&self.focus, cx);
            }
        }
        let hosts: Vec<(String, Option<String>)> = self
            .machines
            .iter()
            .flatten()
            .filter(|m| !m.local)
            .map(|m| (m.name.clone(), m.hostname.clone()))
            .collect();
        let local_name = self
            .machines
            .iter()
            .flatten()
            .find(|m| m.local)
            .map_or_else(|| "This computer".to_string(), |m| m.name.clone());
        let row = || {
            div()
                .flex()
                .flex_none()
                .h(px(36.))
                .w_full()
                .items_center()
                .gap(px(12.))
                .px(px(12.))
                .rounded(px(radius()))
                .text_size(px(fs_md()))
                .text_color(t.text)
        };
        let hover = t.bg_hover;
        let viewport = window.viewport_size();
        let error = self.error.clone().map(|e| {
            div()
                .id("add-project-error")
                .role(Role::Alert)
                .border_t_1()
                .border_color(t.border)
                .px(px(20.))
                .py(px(8.))
                .text_size(px(fs_sm()))
                .text_color(t.danger)
                .child(e)
        });

        let body = if self.browsing {
            let follow = std::mem::take(&mut self.follow);
            let query = self.query(cx);
            let visible = self.visible(cx);
            let active = self.index.min(visible.len().saturating_sub(1));
            if follow {
                if active == 0 {
                    self.list_scroll.set_offset(point(px(0.), px(0.)));
                } else {
                    self.list_scroll.scroll_to_item(active);
                }
            }
            let empty = (!self.loading && visible.is_empty()).then(|| {
                div()
                    .px(px(12.))
                    .py(px(24.))
                    .text_center()
                    .text_size(px(fs_md()))
                    .text_color(t.text_subtle)
                    .child(if query.as_deref().is_some_and(|q| !q.is_empty()) {
                        "No matches"
                    } else {
                        "No folders"
                    })
            });
            let rows: Vec<_> = visible
                .iter()
                .enumerate()
                .map(|(i, entry)| {
                    let path = entry.path.clone();
                    let is_active = i == active;
                    row()
                        .id(SharedString::from(format!("folder-{i}")))
                        .role(Role::ListBoxOption)
                        .aria_label(SharedString::from(entry.name.clone()))
                        .aria_selected(is_active)
                        .cursor_pointer()
                        .map(|r| {
                            if is_active {
                                r.bg(t.bg_selected)
                            } else {
                                r.hover(move |s| s.bg(hover))
                            }
                        })
                        .child(
                            div()
                                .flex_none()
                                .text_color(t.text_muted)
                                .child(icon("folder", 16.)),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .child(entry.name.clone()),
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.browse(Some(path.clone()), cx)),
                        )
                })
                .collect();
            let parent = self.listing.as_ref().and_then(|l| l.parent.clone());
            let disabled = self.listing.is_none() || self.loading || self.adding;
            div()
                .flex()
                .flex_col()
                .min_h_0()
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .h(px(56.))
                        .items_center()
                        .gap(px(4.))
                        .pl(px(12.))
                        .pr(px(20.))
                        .border_b_1()
                        .border_color(t.border)
                        .when(!hosts.is_empty(), |bar| {
                            bar.child(
                                icon_button(
                                    "add-project-back",
                                    "Back to machines",
                                    "caret-left",
                                    16.,
                                    cx,
                                )
                                .tooltip(tooltip("Machines"))
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.show_machines(false, cx)),
                                ),
                            )
                        })
                        .child(
                            icon_button("add-project-up", "Parent folder", "arrow-up", 16., cx)
                                .tooltip(tooltip("Parent folder"))
                                .map(|b| match parent {
                                    Some(parent) => {
                                        b.on_click(cx.listener(move |this, _, _, cx| {
                                            this.browse(Some(parent.clone()), cx);
                                        }))
                                    }
                                    None => b.opacity(0.4).cursor_default(),
                                }),
                        )
                        .children(self.host.clone().map(|host| {
                            div()
                                .ml(px(4.))
                                .flex_none()
                                .max_w(px(160.))
                                .truncate()
                                .font_family(font_mono())
                                .text_size(px(fs_md()))
                                .text_color(t.text_muted)
                                .child(format!("{host}:"))
                        }))
                        .child(
                            div()
                                .ml(px(4.))
                                .flex_1()
                                .min_w_0()
                                .font_family(font_mono())
                                .text_size(px(fs_md()))
                                .child(self.field.clone()),
                        )
                        .when(self.loading, |bar| bar.child(spinner(14., t.text_subtle))),
                )
                .child(
                    div()
                        .id("add-project-folders")
                        .role(Role::ListBox)
                        .aria_label("Folders")
                        .flex()
                        .flex_col()
                        .h(px(320.))
                        .min_h_0()
                        .p(px(8.))
                        .overflow_y_scroll()
                        .track_scroll(&self.list_scroll)
                        .children(empty)
                        .children(rows),
                )
                .children(error)
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .h(px(48.))
                        .items_center()
                        .gap(px(16.))
                        .pl(px(20.))
                        .pr(px(10.))
                        .border_t_1()
                        .border_color(t.border)
                        .text_size(px(fs_xs()))
                        .text_color(t.text_subtle)
                        .child("↑↓ navigate")
                        .child("↵ open")
                        .child("⌫ up")
                        .child(
                            btn("add-project", "Add project", BtnKind::Primary, disabled, &t)
                                .ml_auto()
                                .when(self.adding, |b| b.child(spinner(13., t.text_on_accent)))
                                .child("Add project")
                                .when(!disabled, |b| {
                                    b.on_click(cx.listener(|this, _, _, cx| this.add(cx)))
                                }),
                        ),
                )
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .min_h_0()
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .h(px(56.))
                        .items_center()
                        .px(px(20.))
                        .border_b_1()
                        .border_color(t.border)
                        .text_size(px(crate::ui::theme::fs_lg()))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Add a project"),
                )
                .child(
                    div()
                        .id("machines")
                        .role(Role::Group)
                        .aria_label("Machines")
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .p(px(8.))
                        .child(
                            row()
                                .id("machine-local")
                                .role(Role::Button)
                                .aria_label(SharedString::from(local_name.clone()))
                                .tab_index(0)
                                .cursor_pointer()
                                .hover(move |s| s.bg(hover))
                                .child(
                                    div()
                                        .flex_none()
                                        .text_color(t.text_muted)
                                        .child(icon("desktop", 16.)),
                                )
                                .child(div().min_w_0().flex_1().truncate().child(local_name))
                                .child(
                                    div()
                                        .flex_none()
                                        .text_size(px(fs_xs()))
                                        .text_color(t.text_subtle)
                                        .child("This computer"),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_color(t.text_subtle)
                                        .child(icon("caret-right", 12.)),
                                )
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.open_machine(None, cx)),
                                ),
                        )
                        .child(
                            div()
                                .px(px(12.))
                                .pt(px(12.))
                                .pb(px(6.))
                                .text_size(px(fs_xs()))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(t.text_subtle)
                                .child("SSH HOSTS"),
                        )
                        .children(hosts.iter().map(|(name, hostname)| {
                            let host = name.clone();
                            row()
                                .id(SharedString::from(format!("machine-{name}")))
                                .role(Role::Button)
                                .aria_label(SharedString::from(name.clone()))
                                .tab_index(0)
                                .cursor_pointer()
                                .hover(move |s| s.bg(hover))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.open_machine(Some(host.clone()), cx);
                                }))
                                .child(
                                    div()
                                        .flex_none()
                                        .text_color(t.text_muted)
                                        .child(icon("hard-drives", 16.)),
                                )
                                .child(div().min_w_0().flex_1().truncate().child(name.clone()))
                                .children(hostname.clone().map(|h| {
                                    div()
                                        .flex_none()
                                        .max_w(px(220.))
                                        .truncate()
                                        .font_family(font_mono())
                                        .text_size(px(fs_xs()))
                                        .text_color(t.text_subtle)
                                        .child(h)
                                }))
                        })),
                )
                .children(error)
                .into_any_element()
        };

        // The machine list waits for the hosts, so it never flashes before switching to the
        // folders.
        let shown = self.machines.is_some() || self.browsing;
        dialog::backdrop("add-project-backdrop", false)
            .items_start()
            .pt(viewport.height * 0.15)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(Closed(None))),
            )
            .on_key_down(cx.listener(Self::on_key))
            .when(shown, |backdrop| {
                backdrop.child(
                    dialog::card("add-project", "Add a project", &self.focus, &t)
                        .flex()
                        .flex_col()
                        .w(px(560.))
                        .max_w(viewport.width - px(32.))
                        .max_h(viewport.height * 0.6)
                        .overflow_hidden()
                        .child(body),
                )
            })
    }
}
