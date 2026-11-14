//! What every dialog is built from: the layer over the window it sits on, the raised card that
//! holds it, and the row at the card's top with its title and close button. Each is the part
//! the accessibility rules ask of a dialog, so a new dialog gets them by using these. Closing on
//! Escape is its owner's: only the owner knows what closing does.

// Rust guideline compliant 2026-10-09

use gpui::{
    Div, ElementId, FocusHandle, FontWeight, MouseButton, Role, SharedString, Stateful, div,
    prelude::*, px,
};

use crate::ui::popup_shadow;
use crate::ui::theme::{FS_LG, radius_lg, Theme};

/// The layer a dialog sits on: over the whole window, which it keeps the pointer from, with the
/// card centred across it. `dim` darkens the window behind; a dialog that shows its effect on
/// the window as it is used leaves it clear. Its owner places the card up and down, and closes
/// on a press that reaches the layer.
pub fn backdrop(id: impl Into<ElementId>, dim: bool) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .justify_center()
        .occlude()
        .when(dim, |layer| layer.bg(gpui::hsla(0., 0., 0., 0.4)))
}

/// A dialog's card: named `label` for a screen reader, holding the focus, raised over the
/// backdrop, and keeping a press on it from reaching the backdrop. Its owner lays it out.
pub fn card(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    focus: &FocusHandle,
    t: &Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Dialog)
        .aria_label(label)
        .track_focus(focus)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .rounded(px(radius_lg()))
        .border_1()
        .border_color(t.border_strong)
        .bg(t.bg_elevated)
        .shadow(popup_shadow(t))
}

/// The row at the top of a card: its title, cut short when it is too long, and `close`, the
/// button that closes the dialog.
pub fn title_row(title: impl Into<SharedString>, close: Stateful<Div>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(px(FS_LG))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.into()),
        )
        .child(close)
}
