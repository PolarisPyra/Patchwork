//! The banner over the chat box for the request that is waiting.

// Rust guideline compliant 2026-10-09

use super::card::code_block;
use super::card::shown_pattern;
use super::card::AllowChoice;
use gpui::{
    Anchor, Div, FontWeight, MouseButton, Role, SharedString, anchored, deferred, div, point,
    prelude::*, px,
};
use crate::ui::theme::{font_mono, FS_SM, FS_XS, radius, radius_sm, theme};
use crate::ui::{button, icon, popup};
use crate::workspace::Workspace;

impl Workspace {
    /// The first request waiting, with Deny and Allow. Allow can also save a rule for the
    /// thread's project (or chats) or for everywhere, and offers the choice made last for the
    /// same kind of request.
    pub fn approval_banner(&self, cx: &mut Context<Self>) -> Option<gpui::Stateful<Div>> {
        let pending = self.pending_permissions();
        let a = (*pending.first()?).clone();
        let t = *theme(cx);
        let can_save = !a.suggestions.is_empty();
        let scope_name = self.thread_scope().1;
        let choices = [
            (AllowChoice::Once, "Allow once".to_string()),
            (AllowChoice::Local, format!("Always allow in {scope_name}")),
            (
                AllowChoice::Everywhere,
                "Always allow everywhere".to_string(),
            ),
        ];
        let saved = a
            .kind
            .and_then(|k| self.allow_choices.get(&k).copied())
            .unwrap_or(AllowChoice::Once);
        let main = if can_save { saved } else { AllowChoice::Once };
        let main_label = match main {
            AllowChoice::Once => "Allow".to_string(),
            _ => choices
                .iter()
                .find(|(c, _)| *c == main)
                .map_or(String::new(), |(_, l)| l.clone()),
        };
        let patterns = a
            .suggestions
            .iter()
            .map(|p| shown_pattern(p))
            .collect::<Vec<_>>()
            .join(", ");
        let menu_open = self.allow_menu.as_deref() == Some(a.id.as_str());
        let (deny_id, allow_id, menu_id) = (a.id.clone(), a.id.clone(), a.id.clone());
        let kind = a.kind;

        let btn = |id: &str, label: SharedString| {
            button(SharedString::from(id.to_string()), label.clone())
                .h(px(28.))
                .px(px(14.))
                .justify_center()
                .rounded(px(radius_sm()))
                .border_1()
                .text_size(px(crate::ui::theme::FS_MD))
                .child(label)
        };
        let (hover, accent_hover) = (t.bg_hover, t.accent_hover);
        let deny = btn("approval-deny", "Deny".into())
            .border_color(t.border_strong)
            .bg(t.bg_editor)
            .text_color(t.text)
            .hover(move |s| s.bg(hover))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.respond_permission(&deny_id, false, None, cx);
            }));
        let allow = btn("approval-allow", main_label.into())
            .border_color(t.accent)
            .bg(t.accent)
            .text_color(t.text_on_accent)
            .hover(move |s| s.bg(accent_hover))
            .when(can_save, gpui::Styled::rounded_r_none)
            .on_click(cx.listener(move |this, _, _, cx| this.allow(&allow_id, kind, main, cx)));
        let mut divider = t.text_on_accent;
        divider.a = 0.3;
        let more = can_save.then(|| {
            let menu = menu_open.then(|| {
                let rows = choices.iter().map(|(choice, label)| {
                    let (choice, approval_id) = (*choice, a.id.clone());
                    div()
                        .id(SharedString::from(format!("allow-{label}")))
                        .role(Role::MenuItem)
                        .aria_label(label.clone())
                        .tab_index(0)
                        .focus_visible(crate::ui::focus_ring)
                        .min_w(px(260.))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .rounded(px(radius_sm()))
                        .px(px(8.))
                        .py(px(5.))
                        .cursor_pointer()
                        .text_color(t.text)
                        .when(choice == main, |r| r.bg(t.bg_selected_muted))
                        .when(choice != main, |r| r.hover(move |s| s.bg(hover)))
                        .child(label.clone())
                        .when(choice != AllowChoice::Once, |r| {
                            r.child(
                                div()
                                    .max_w(px(360.))
                                    .truncate()
                                    .font_family(font_mono())
                                    .text_size(px(FS_XS))
                                    .text_color(t.text_muted)
                                    .child(patterns.clone()),
                            )
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.allow(&approval_id, kind, choice, cx);
                        }))
                });
                div().absolute().top_0().right_0().child(
                    deferred(
                        anchored()
                            .anchor(Anchor::BottomRight)
                            .offset(point(px(0.), px(-4.)))
                            .snap_to_window_with_margin(px(8.))
                            .child(
                                popup(&t)
                                    .id("allow-menu")
                                    .role(Role::Menu)
                                    .aria_label("More ways to allow")
                                    .occlude()
                                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                        this.allow_menu = None;
                                        cx.notify();
                                    }))
                                    .on_key_down(cx.listener(
                                        |this, e: &gpui::KeyDownEvent, _, cx| {
                                            if e.keystroke.key == "escape" {
                                                this.allow_menu = None;
                                                cx.notify();
                                            }
                                        },
                                    ))
                                    .children(rows),
                            ),
                    )
                    .with_priority(1),
                )
            });
            div()
                .relative()
                .child(
                    button("approval-more", "More ways to allow")
                        .aria_expanded(menu_open)
                        .h(px(28.))
                        .px(px(6.))
                        .justify_center()
                        .rounded_r(px(radius_sm()))
                        .relative()
                        .border_1()
                        .border_color(t.accent)
                        .bg(t.accent)
                        .text_color(t.text_on_accent)
                        // The split line between Allow and its menu.
                        .child(
                            div()
                                .absolute()
                                .left(px(-1.))
                                .top_0()
                                .bottom_0()
                                .w(px(1.))
                                .bg(divider),
                        )
                        .child(icon("caret-down", 12.))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.allow_menu = match this.allow_menu {
                                Some(_) => None,
                                None => Some(menu_id.clone()),
                            };
                            cx.notify();
                        })),
                )
                .children(menu)
        });

        Some(
            div()
                .id("approval-request")
                .role(Role::Group)
                .aria_label("Approval request")
                .mb(px(6.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .rounded(px(radius()))
                .border_1()
                .border_color(t.warning)
                .bg(t.bg_panel)
                .p(px(10.))
                .text_size(px(FS_SM))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_color(t.warning)
                        .child(icon("shield-warning", 15.))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(t.text)
                                .child(a.title.clone()),
                        )
                        .when(pending.len() > 1, |row| {
                            row.child(
                                div()
                                    .ml_auto()
                                    .flex_none()
                                    .text_color(t.text_muted)
                                    .child(format!("1/{}", pending.len())),
                            )
                        }),
                )
                .when(!a.detail.is_empty(), |c| {
                    c.child(
                        div()
                            .id("approval-detail")
                            .max_h(px(160.))
                            .overflow_y_scroll()
                            .child(code_block(&a.detail, &t)),
                    )
                })
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(6.))
                        .child(deny)
                        .child(div().flex().child(allow).children(more)),
                ),
        )
    }
}
