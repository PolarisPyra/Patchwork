//! A chat's or an agent's own background: the picker in a card over the chat or the agent's page,
//! which stays undimmed behind it to show each pick. Escape or a press outside closes it.

// Rust guideline compliant 2026-10-09

use gpui::{
    Entity, EventEmitter, FocusHandle, KeyDownEvent, MouseButton, SharedString, Window, prelude::*,
    px,
};

use patchwork_ui::background_picker::BackgroundPicker;
use patchwork_ui::controls::dialog;
use patchwork_ui::icon_button;
use patchwork_ui::theme::theme;

/// The user closed the card.
#[derive(Debug, Clone, Copy)]
pub struct Closed;

/// The card around a background picker, named for whose background it picks.
pub struct BackgroundDialog {
    title: SharedString,
    picker: Entity<BackgroundPicker>,
    focus: FocusHandle,
}

impl std::fmt::Debug for BackgroundDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackgroundDialog")
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

impl EventEmitter<Closed> for BackgroundDialog {}

impl BackgroundDialog {
    /// The card around `picker`, focused, for the chat or agent named `name`.
    #[must_use]
    pub fn new(
        name: impl Into<SharedString>,
        picker: Entity<BackgroundPicker>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            title: name.into(),
            picker,
            focus,
        }
    }
}

impl Render for BackgroundDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let viewport = window.viewport_size();
        dialog::backdrop("background-dialog-backdrop", false)
            .items_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(Closed)),
            )
            .child(
                dialog::card(
                    "background-dialog",
                    format!("Background of {}", self.title),
                    &self.focus,
                    &t,
                )
                .on_key_down(cx.listener(|_, e: &KeyDownEvent, _, cx| {
                    if e.keystroke.key == "escape" {
                        cx.stop_propagation();
                        cx.emit(Closed);
                    }
                }))
                .flex()
                .flex_col()
                .gap(px(12.))
                // Three cards across, with the card padding making the edges even.
                .w(px(482.))
                .max_w(viewport.width - px(48.))
                .px(px(10.))
                .pt(px(14.))
                .pb(px(10.))
                .child(
                    dialog::title_row(
                        self.title.clone(),
                        icon_button("background-dialog-close", "Close", "x", 16., cx)
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(Closed))),
                    )
                    // In line with the cards' pictures.
                    .pl(px(7.))
                    .pr(px(3.)),
                )
                .child(self.picker.clone()),
            )
    }
}
