//! A chat's own background (`components/ChatBackgroundDialog.tsx`): the picker in a card over the
//! chat, which stays undimmed behind it to show each pick. Escape or a press outside closes it.

use gpui::{
    AnyElement, Context, Entity, FocusHandle, FontWeight, MouseButton, Role, Window, deferred, div,
    prelude::*, px,
};

use super::Workspace;
use crate::ui::background_picker::{BackgroundPicker, PickerEvent};
use crate::ui::theme::{FS_LG, RADIUS_LG, theme};
use crate::ui::{display_title, icon_button, popup_shadow};

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
        let (server, shared) = (self.server.clone(), self.settings.chat_background.clone());
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
                        picker.update(cx, |p, cx| p.failed(before, e, cx));
                    }
                }
                // The pictures moved the backgrounds that showed them: this chat's, others', and
                // the one all chats share.
                PickerEvent::PicturesChanged => {
                    this.backgrounds = Default::default();
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
                div()
                    .id("background-dialog-backdrop")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .occlude()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| this.close_background_dialog(window, cx)),
                    )
                    .child(
                        div()
                            .id("background-dialog")
                            .role(Role::Dialog)
                            .aria_label(format!("Background of {title}"))
                            .track_focus(&dialog.focus)
                            .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                                if e.keystroke.key == "escape" {
                                    cx.stop_propagation();
                                    this.close_background_dialog(window, cx);
                                }
                            }))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .w(px(480.))
                            .max_w(viewport.width - px(48.))
                            .p(px(16.))
                            .rounded(px(RADIUS_LG))
                            .border_1()
                            .border_color(t.border_strong)
                            .bg(t.bg_elevated)
                            .shadow(popup_shadow(&t))
                            .child(
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
                                            .child(title),
                                    )
                                    .child(
                                        icon_button(
                                            "background-dialog-close",
                                            "Close",
                                            "x",
                                            16.,
                                            cx,
                                        )
                                        .on_click(
                                            cx.listener(|this, _, window, cx| {
                                                this.close_background_dialog(window, cx)
                                            }),
                                        ),
                                    ),
                            )
                            .child(dialog.picker.clone()),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}
