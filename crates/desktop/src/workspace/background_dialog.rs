//! A chat's own background: the picker in a card over the
//! chat, which stays undimmed behind it to show each pick. Escape or a press outside closes it.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use gpui::{AnyElement, Entity, FocusHandle, MouseButton, Window, deferred, prelude::*, px};

use super::Workspace;
use crate::ui::background_picker::{BackgroundPicker, PickerEvent};
use crate::ui::dialog;
use crate::ui::theme::theme;
use crate::ui::{display_title, icon_button};

pub struct BackgroundDialog {
    thread_id: String,
    picker: Entity<BackgroundPicker>,
    focus: FocusHandle,
}

impl Workspace {
    /// Opens the background of the chat `id`; a subagent's chat has none of its own.
    pub fn open_background_dialog(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(thread) = self.server.thread(id) else {
            return;
        };
        if thread.parent.is_some() {
            return;
        }
        let (server, shared) = (
            Arc::clone(&self.server),
            self.settings.chat_background.clone(),
        );
        let picker = cx.new(|cx| {
            BackgroundPicker::new(
                server,
                "Background of this chat",
                thread.background.clone(),
                None,
                Some(shared),
                cx,
            )
        });
        cx.subscribe(&picker, |this, picker, e: &PickerEvent, cx| {
            let Some(id) = this.background_dialog.as_ref().map(|d| d.thread_id.clone()) else {
                return;
            };
            match e {
                PickerEvent::Picked(background) => {
                    if let Err(e) = this.server.set_thread_background(&id, background.clone()) {
                        let before = this.server.thread(&id).ok().and_then(|t| t.background);
                        picker.update(cx, |p, cx| p.failed(before, e.to_string(), cx));
                    }
                }
                // The pictures moved the backgrounds that showed them: this chat's, others', and
                // the one all chats share.
                PickerEvent::PicturesChanged => {
                    this.backgrounds = crate::ui::backdrop::Backgrounds::default();
                    this.settings = this.server.store.settings();
                    let value = this.server.thread(&id).ok().and_then(|t| t.background);
                    let shared = this.settings.chat_background.clone();
                    picker.update(cx, |p, cx| {
                        p.set_value(value, Some(shared));
                        cx.notify();
                    });
                }
            }
            this.reload(cx);
        })
        .detach();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.background_dialog = Some(BackgroundDialog {
            thread_id: thread.id,
            picker,
            focus,
        });
        cx.notify();
    }

    fn close_background_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.background_dialog.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    pub(super) fn background_dialog(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.background_dialog.as_ref()?;
        let t = *theme(cx);
        let title = self
            .threads
            .iter()
            .find(|t| t.id == dialog.thread_id)
            .map(|t| display_title(&t.title))
            .unwrap_or_default();
        let viewport = window.viewport_size();
        Some(
            deferred(
                dialog::backdrop("background-dialog-backdrop", false)
                    .items_center()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| this.close_background_dialog(window, cx)),
                    )
                    .child(
                        dialog::card(
                            "background-dialog",
                            format!("Background of {title}"),
                            &dialog.focus,
                            &t,
                        )
                        .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                            if e.keystroke.key == "escape" {
                                cx.stop_propagation();
                                this.close_background_dialog(window, cx);
                            }
                        }))
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .w(px(480.))
                        .max_w(viewport.width - px(48.))
                        .p(px(16.))
                        .child(dialog::title_row(
                            title,
                            icon_button("background-dialog-close", "Close", "x", 16., cx).on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.close_background_dialog(window, cx);
                                }),
                            ),
                        ))
                        .child(dialog.picker.clone()),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}
