//! The Artifacts page as the window drives it: opening it on an artifact a chat made, and the
//! chat an artifact came from in a card floating over it, so the artifact stays in sight while
//! the agent is asked to change it.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{AnyElement, Entity, KeyDownEvent, Role, Window, div, prelude::*, px, relative};

use crate::artifacts::ArtifactsPage;

use super::state::{Page, Workspace};
use crate::ui::theme::{Theme, fs_md, radius_lg};
use crate::ui::{display_title, icon_button, popup_shadow, tooltip};

/// The card's widest and tallest; a smaller window gets a smaller card.
const SIZE: (f32, f32) = (440., 640.);

impl Workspace {
    /// The Artifacts page, made the first time it shows.
    pub(super) fn artifacts_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ArtifactsPage> {
        if let Some(page) = &self.artifacts {
            return page.clone();
        }
        let core = Arc::clone(&self.core);
        let thumbs = self.thumbs.clone();
        let came_from = self.thread.as_ref().map(|t| t.id.clone());
        let page = cx.new(|cx| ArtifactsPage::new(core, thumbs, came_from, cx));
        cx.subscribe_in(&page, window, Self::on_artifacts).detach();
        self.artifacts = Some(page.clone());
        page
    }

    /// Stops the spare browser once neither the Artifacts page nor the panel beside a chat is
    /// left to use it.
    pub(super) fn cool_browser_unless_shown(&self, cx: &mut Context<Self>) {
        if self.artifacts.is_none() && self.artifact_beside.is_none() {
            cx.background_spawn(async { patchwork_core::browser::cool_browser() })
                .detach();
        }
    }

    /// Shows version `n` of the artifact `id` on the Artifacts page.
    pub(crate) fn open_artifact(
        &mut self,
        id: String,
        n: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.page = Page::Artifacts;
        self.peeking = false;
        let page = self.artifacts_page(window, cx);
        page.update(cx, |page, cx| page.open(id, n, cx));
        cx.notify();
    }

    /// Reads every artifact again, for the chats' artifact cards.
    pub(super) fn load_artifact_list(&mut self, cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
        crate::ui::off_thread(
            cx,
            move || core.artifacts(),
            |this, list, cx| {
                this.artifact_list = list;
                cx.notify();
            },
        );
    }

    /// Opens chat `id` in the card over the Artifacts page, with its message box focused.
    pub(super) fn open_artifact_chat(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_thread(id, cx);
        // Opening a chat shows the chats' page; the card shows it over this one instead.
        self.page = Page::Artifacts;
        if self.thread.as_ref().is_none_or(|t| t.id != id) {
            self.error = Some("That chat is gone".to_owned());
            cx.notify();
            return;
        }
        self.artifact_chat = true;
        self.set_artifact_chat_shown(Some(id.to_owned()), cx);
        self.composer.update(cx, |c, cx| c.focus(window, cx));
        cx.notify();
    }

    /// Closes the card.
    pub(super) fn close_artifact_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.artifact_chat = false;
        self.set_artifact_chat_shown(None, cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Tells the page which chat the card shows, for its chat button.
    fn set_artifact_chat_shown(&self, shown: Option<String>, cx: &mut Context<Self>) {
        if let Some(page) = &self.artifacts {
            page.update(cx, |page, cx| page.set_chat_shown(shown, cx));
        }
    }

    /// The card, while it shows: the chat's title and close button over the chat itself.
    pub(super) fn artifact_chat_card(
        &self,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.artifact_chat || self.page != Page::Artifacts {
            return None;
        }
        let title = self
            .thread
            .as_ref()
            .map(|thread| display_title(&thread.title))
            .unwrap_or_default();
        let card = div()
            .id("artifact-chat")
            .role(Role::Dialog)
            .aria_label(format!("Chat: {title}"))
            .occlude()
            .absolute()
            .right(px(16.))
            .bottom(px(16.))
            .w(px(SIZE.0))
            .max_w(relative(0.9))
            .h(px(SIZE.1))
            .max_h(relative(0.9))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(radius_lg()))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_app)
            .shadow(popup_shadow(t))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                if e.keystroke.key == "escape" {
                    this.close_artifact_chat(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.))
                    .pl(px(14.))
                    .pr(px(8.))
                    .py(px(6.))
                    .border_b_1()
                    .border_color(t.border)
                    .bg(t.bg_panel)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(fs_md()))
                            .child(title),
                    )
                    .child(
                        icon_button("artifact-chat-close", "Close chat", "x", 14., cx)
                            .tooltip(tooltip("Close chat"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_artifact_chat(window, cx);
                            })),
                    ),
            )
            .child(div().flex().flex_1().min_h_0().child(self.chat_layers()));
        Some(card.into_any_element())
    }
}
