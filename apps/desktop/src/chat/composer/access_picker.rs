//! The permissions picker: a structured card matching the model and agent pickers, with one
//! mode per row, a checkmark on the active mode, and single-line descriptions.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, FontWeight, MouseButton, MouseDownEvent, Role, SharedString, div, prelude::*, px,
};
use patchwork_backend::chats::AccessMode;
use patchwork_ui_components::{FocusNext, FocusPrev, popup_shadow, tooltip};
use patchwork_ui_theme::{IconName, Theme, fs, fs_xs, icon, radius};

use super::Composer;
use super::menus::Pick;

/// The card's width, wide enough for each mode's description to sit cleanly on a single line.
const WIDTH: f32 = 390.;

pub(super) const ACCESS_MODES: [(AccessMode, &str, &str, IconName); 4] = [
    (
        AccessMode::Supervised,
        "Supervised",
        "Ask before commands and file changes.",
        IconName::LockSimple,
    ),
    (
        AccessMode::AutoAcceptEdits,
        "Auto-accept edits",
        "Auto-approve edits, ask before other actions.",
        IconName::NotePencil,
    ),
    (
        AccessMode::Auto,
        "Auto",
        "Supported providers approve routine actions; others still ask.",
        IconName::Robot,
    ),
    (
        AccessMode::FullAccess,
        "Full access",
        "Allow commands and edits without prompts.",
        IconName::LockOpen,
    ),
];

pub fn access_of(mode: AccessMode) -> (&'static str, IconName) {
    ACCESS_MODES.iter().find(|(m, ..)| *m == mode).map_or(
        ("Full access", IconName::LockOpen),
        |(_, label, _, icon)| (*label, *icon),
    )
}

impl Composer {
    /// Renders the permissions picker card.
    pub(super) fn access_picker(
        &mut self,
        current: AccessMode,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.menu_picks = ACCESS_MODES
            .iter()
            .map(|&(mode, ..)| Pick::Access(mode))
            .collect();
        if self.highlight >= ACCESS_MODES.len() {
            self.highlight = ACCESS_MODES
                .iter()
                .position(|&(mode, ..)| mode == current)
                .unwrap_or(0);
        }
        let spotted_index = self.highlight;
        let focus = self.menu_focus.clone();

        let rows: Vec<AnyElement> = ACCESS_MODES
            .iter()
            .enumerate()
            .map(|(i, &(mode, label, description, glyph))| {
                let selected = current == mode;
                let spotted = spotted_index == i;
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
                div()
                    .id(SharedString::from(format!("access-mode-{i}")))
                    .role(Role::MenuItemRadio)
                    .aria_label(label)
                    .aria_toggled(selected.into())
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(8.))
                    .py(px(7.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(spotted, |r| r.bg(t.bg_hover))
                    .tooltip(tooltip(description))
                    .child(
                        div()
                            .size(px(26.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(glyph, 14.).text_color(icon_color)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(fs(13.)))
                                    .text_color(fg)
                                    .when(selected, |d| d.font_weight(FontWeight::MEDIUM))
                                    .child(label),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(fs_xs()))
                                    .text_color(t.text_subtle)
                                    .child(description),
                            ),
                    )
                    .child(
                        div()
                            .w(px(16.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(selected, |c| {
                                c.child(icon(IconName::CheckBold, 12.).text_color(t.accent_text))
                            }),
                    )
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if this.highlight != i {
                            this.highlight = i;
                            cx.notify();
                        }
                    }))
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.apply(Pick::Access(mode), window, cx);
                    }))
                    .into_any_element()
            })
            .collect();

        div()
            .id("access-picker")
            .role(Role::Menu)
            .aria_label("Permissions")
            .track_focus(&focus)
            .occlude()
            .on_key_down(cx.listener(Self::menu_key))
            .on_action(cx.listener(|this, _: &FocusNext, window, cx| {
                this.close_menu(window, cx);
            }))
            .on_action(cx.listener(|this, _: &FocusPrev, window, cx| {
                this.close_menu(window, cx);
            }))
            .on_mouse_down_out(cx.listener(|this, e: &MouseDownEvent, window, cx| {
                this.dismissed = this.menu.take().map(|m| (m, e.position));
                this.restore_focus(window, cx);
                cx.notify();
            }))
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
                    .h(px(36.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px(px(12.))
                    .border_b_1()
                    .border_color(t.border)
                    .text_size(px(fs_xs()))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(t.text_muted)
                    .child("Permissions"),
            )
            .child(
                div()
                    .id("access-list")
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .p(px(6.))
                    .children(rows),
            )
            .into_any_element()
    }
}
