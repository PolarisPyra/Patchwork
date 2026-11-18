//! A chat's or a bot's own background: the picker in a card over the chat or the bot's page,
//! which stays undimmed behind it to show each pick. Escape or a press outside closes it.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use gpui::{AnyElement, Entity, FocusHandle, MouseButton, Window, deferred, prelude::*, px};

use patchwork_core::backgrounds::ChatBackground;

use super::Workspace;
use crate::ui::background_picker::{BackgroundPicker, PickerEvent};
use crate::ui::dialog;
use crate::ui::theme::theme;
use crate::ui::{display_title, icon_button};

/// Whose background the dialog picks.
#[derive(Clone)]
enum Owner {
    Thread(String),
    Bot(String),
}

pub struct BackgroundDialog {
    owner: Owner,
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
        let Ok(thread) = self.core.thread(id) else {
            return;
        };
        if thread.parent.is_some() {
            return;
        }
        let owner = Owner::Thread(thread.id);
        self.open_picker(
            owner,
            "Background of this chat",
            thread.background,
            window,
            cx,
        );
    }

    /// Opens the background of bot `id`, which its page and its runs show.
    pub fn open_bot_background_dialog(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(bot) = self.core.bot(id) else {
            return;
        };
        let owner = Owner::Bot(bot.id);
        self.open_picker(owner, "Background of this bot", bot.background, window, cx);
    }

    /// The background `owner` has of its own, read again.
    fn own_background(&self, owner: &Owner) -> Option<ChatBackground> {
        match owner {
            Owner::Thread(id) => self.core.thread(id).ok().and_then(|t| t.background),
            Owner::Bot(id) => self.core.bot(id).and_then(|b| b.background),
        }
    }

    /// Saves `background` as `owner`'s own, and shows it on the bot's page.
    fn save_background(
        &mut self,
        owner: &Owner,
        background: Option<ChatBackground>,
        cx: &mut Context<Self>,
    ) -> Result<(), patchwork_core::Error> {
        match owner {
            Owner::Thread(id) => self.core.set_thread_background(id, background),
            Owner::Bot(id) => {
                let bot = self.core.set_bot_background(id, background)?;
                if let Some(page) = &self.bot_page {
                    page.update(cx, |page, cx| page.show_saved(bot, cx));
                }
                self.reload_bots(cx);
                Ok(())
            }
        }
    }

    fn open_picker(
        &mut self,
        owner: Owner,
        label: &'static str,
        value: Option<ChatBackground>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (core, shared) = (
            Arc::clone(&self.core),
            self.settings.chat_background.clone(),
        );
        let picker = cx.new(|cx| BackgroundPicker::new(core, label, value, None, Some(shared), cx));
        cx.subscribe(&picker, |this, picker, e: &PickerEvent, cx| {
            let Some(owner) = this.background_dialog.as_ref().map(|d| d.owner.clone()) else {
                return;
            };
            match e {
                PickerEvent::Picked(background) => {
                    if let Err(e) = this.save_background(&owner, background.clone(), cx) {
                        let before = this.own_background(&owner);
                        picker.update(cx, |p, cx| p.failed(before, e.to_string(), cx));
                    }
                }
                // The pictures moved the backgrounds that showed them: this one, others', and
                // the one all chats share.
                PickerEvent::PicturesChanged => {
                    this.backgrounds.reload();
                    this.settings = this.core.settings();
                    this.bot_pictures_changed(cx);
                    let value = this.own_background(&owner);
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
            owner,
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
        let title = match &dialog.owner {
            Owner::Thread(id) => self
                .threads
                .iter()
                .find(|t| &t.id == id)
                .map(|t| display_title(&t.title)),
            Owner::Bot(id) => self
                .bots
                .iter()
                .find(|b| &b.id == id)
                .map(|b| b.name.clone()),
        }
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
                        // Three cards across, with the card padding making the edges even.
                        .w(px(482.))
                        .max_w(viewport.width - px(48.))
                        .px(px(10.))
                        .pt(px(14.))
                        .pb(px(10.))
                        .child(
                            dialog::title_row(
                                title,
                                icon_button("background-dialog-close", "Close", "x", 16., cx)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.close_background_dialog(window, cx);
                                    })),
                            )
                            // In line with the cards' pictures.
                            .pl(px(7.))
                            .pr(px(3.)),
                        )
                        .child(dialog.picker.clone()),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}
