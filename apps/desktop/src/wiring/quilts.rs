//! The Quilts page as the window drives it: opening it on a quilt a chat made, and the
//! chat a quilt came from in a card floating over it, so the quilt stays in sight while
//! the agent is asked to change it.

// Rust guideline compliant 2026-10-09

use gpui::component::button::Button;
use gpui::component::button::ButtonVariants as _;
use gpui::component::Sizable as _;
use patchwork_desktop_chat::prelude::*;
use patchwork_client::AppState;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    AnyElement, AnyView, App, Entity, KeyDownEvent, Role, WeakEntity, Window, div, prelude::*, px,
    relative,
};

use patchwork_desktop_browser::{BrowserView, ElementPicked, PageErrorsChanged, PickingChanged};
use patchwork_desktop_quilts::{PickedElement, QuiltPreview, QuiltsHost, QuiltsPage};

use crate::nav::{Page, Place};

use crate::window::Workspace;
use patchwork_design_icons::IconName;
use patchwork_design_theme::{Theme, fs_md, radius_lg};
use patchwork_design_text::display_title;
use patchwork_design_components::popup_shadow;

/// The card's widest and tallest; a smaller window gets a smaller card.
const SIZE: (f32, f32) = (440., 640.);

/// Draws quilts' pages in the browser sidecar's view.
pub(crate) struct BrowserPreviews;

impl QuiltsHost for BrowserPreviews {
    fn preview(
        &self,
        url: String,
        title: String,
        page: WeakEntity<QuiltsPage>,
        cx: &mut App,
    ) -> Rc<dyn QuiltPreview> {
        let view = cx.new(|cx| BrowserView::new(url, title, false, cx));
        let changed = page.clone();
        cx.subscribe(&view, move |_, _: &PageErrorsChanged, cx| {
            let _ = changed.update(cx, QuiltsPage::preview_changed);
        })
        .detach();
        let changed = page.clone();
        cx.subscribe(&view, move |_, _: &PickingChanged, cx| {
            let _ = changed.update(cx, QuiltsPage::preview_changed);
        })
        .detach();
        cx.subscribe(&view, move |_, picked: &ElementPicked, cx| {
            let picked = PickedElement {
                selector: picked.selector.clone(),
                html: picked.html.clone(),
                styles: picked.styles.clone(),
            };
            let _ = page.update(cx, |page, cx| page.element_picked(&picked, cx));
        })
        .detach();
        Rc::new(BrowserPreview(view))
    }
}

/// A quilt's page drawn in the browser sidecar's view.
struct BrowserPreview(Entity<BrowserView>);

impl QuiltPreview for BrowserPreview {
    fn view(&self) -> AnyView {
        self.0.clone().into()
    }

    fn open(&self, url: &str, title: String, cx: &mut App) {
        self.0.update(cx, |view, cx| {
            view.relabel(title);
            view.open(url, cx);
        });
    }

    fn errors(&self, cx: &App) -> Vec<String> {
        self.0.read(cx).errors().to_vec()
    }

    fn picking(&self, cx: &App) -> bool {
        self.0.read(cx).picking()
    }

    fn set_picking(&self, on: bool, cx: &mut App) {
        self.0.update(cx, |view, cx| view.set_picking(on, cx));
    }
}

impl Workspace {
    /// The Quilts page, made the first time it shows.
    pub(crate) fn quilts_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<QuiltsPage> {
        if let Some(page) = &self.quilts {
            return page.clone();
        }
        let core = Arc::clone(&self.core);
        let thumbs = self.thumbs.clone();
        let came_from = self.chat.as_ref().map(|t| t.id.clone());
        let open = self.quilt_open.clone();
        let workspace = cx.entity();
        let page = cx.new(|cx| {
            // The workspace draws it cached, so it draws again when the chat's scheme changes.
            cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
            let mut page = QuiltsPage::new(core, Rc::new(BrowserPreviews), thumbs, came_from, cx);
            page.go(open, cx);
            page
        });
        cx.subscribe_in(&page, window, Self::on_quilts).detach();
        self.quilts = Some(page.clone());
        page
    }

    /// Stops the spare browser once neither the Quilts page nor the panel beside a chat is
    /// left to use it.
    pub(crate) fn cool_browser_unless_shown(&self, cx: &mut Context<Self>) {
        if self.quilts.is_none() && self.quilt_beside.is_none() {
            cx.background_spawn(async { patchwork_core::browser::cool_browser() })
                .detach();
        }
    }

    /// Shows version `n` of the quilt `id` on the Quilts page.
    pub(crate) fn open_quilt(
        &mut self,
        id: String,
        n: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let from = self.place();
        self.page = Page::Quilts;
        self.peeking = false;
        self.quilt_open = Some(id.clone());
        self.went_from(from);
        let page = self.quilts_page(window, cx);
        page.update(cx, |page, cx| page.open(id, n, cx));
        cx.notify();
    }

    /// Shows the Quilts page, as its icon and its shortcut do: on the gallery, or as it is when
    /// it already shows.
    pub(crate) fn show_quilts_page(&mut self, cx: &mut Context<Self>) {
        let open = self
            .quilt_open
            .clone()
            .filter(|_| self.page == Page::Quilts);
        self.go_to(&Place::Quilts(open), cx);
    }

    /// Shows the Quilts page with the quilt `open` open, or its gallery, and whether it
    /// could: a quilt deleted since is not.
    pub(crate) fn show_quilts(&mut self, open: Option<&String>, cx: &mut Context<Self>) -> bool {
        if open.is_some_and(|id| !AppState::get(cx).quilts.iter().any(|a| &a.id == id)) {
            return false;
        }
        self.page = Page::Quilts;
        self.peeking = false;
        self.quilt_open = open.cloned();
        if let Some(page) = &self.quilts {
            page.update(cx, |page, cx| page.go(open.cloned(), cx));
        }
        cx.notify();
        true
    }

    /// Reads every quilt again, for the chats' quilt cards.
    pub(crate) fn load_quilt_list(&mut self, cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
        patchwork_design_tasks::off_thread(
            cx,
            move || core.quilts(),
            |_, list, cx| {
                AppState::update(cx, |s| s.set_quilts(list));
                cx.notify();
            },
        );
    }

    /// Opens chat `id` in the card over the Quilts page, with its message box focused.
    pub(crate) fn open_quilt_chat(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_chat(id, cx);
        // Opening a chat shows the chats' page; the card shows it over this one instead, and the
        // window stays where it was for the back button.
        self.page = Page::Quilts;
        if self.chat.as_ref().is_none_or(|t| t.id != id) {
            AppState::set_error(cx, Some("That chat is gone".to_owned()));
            cx.notify();
            return;
        }
        self.quilt_chat = true;
        self.set_quilt_chat_shown(Some(id.to_owned()), cx);
        self.composer.update(cx, |c, cx| c.focus(window, cx));
        cx.notify();
    }

    /// Closes the card.
    pub(crate) fn close_quilt_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.quilt_chat = false;
        self.set_quilt_chat_shown(None, cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Tells the page which chat the card shows, for its chat button.
    fn set_quilt_chat_shown(&self, shown: Option<String>, cx: &mut Context<Self>) {
        if let Some(page) = &self.quilts {
            page.update(cx, |page, cx| page.set_chat_shown(shown, cx));
        }
    }

    /// The card, while it shows: the chat's title and close button over the chat itself.
    pub(crate) fn quilt_chat_card(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.quilt_chat || self.page != Page::Quilts {
            return None;
        }
        let title = self
            .chat
            .as_ref()
            .map(|chat| display_title(&chat.title))
            .unwrap_or_default();
        let card = div()
            .id("quilt-chat")
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
                    this.close_quilt_chat(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.))
                    .pl(px(12.))
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
                        Button::new("quilt-chat-close")
                            .ghost()
                            .small()
                            .icon(IconName::X)
                            .accessibility_label("Close chat")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_quilt_chat(window, cx);
                            })),
                    ),
            )
            .child(div().flex().flex_1().min_h_0().child(self.chat_layers()));
        Some(card.into_any_element())
    }
}

impl Workspace {
    /// Starts a chat asking for a new quilt, or opens the chat a quilt came from.
    pub(crate) fn on_quilts(
        &mut self,
        _: &gpui::Entity<patchwork_desktop_quilts::QuiltsPage>,
        event: &patchwork_desktop_quilts::QuiltsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            patchwork_desktop_quilts::QuiltsEvent::OpenChat(id) => {
                let same = self.chat.as_ref().is_some_and(|t| &t.id == id);
                if self.quilt_chat && same {
                    self.close_quilt_chat(window, cx);
                } else {
                    self.open_quilt_chat(id, window, cx);
                }
            }
            patchwork_desktop_quilts::QuiltsEvent::Closed => {
                self.quilt_beside = None;
                self.cool_browser_unless_shown(cx);
                window.focus(&self.focus, cx);
            }
            patchwork_desktop_quilts::QuiltsEvent::OpenFull { id, n } => {
                self.quilt_beside = None;
                self.open_quilt(id.clone(), *n, window, cx);
            }
            patchwork_desktop_quilts::QuiltsEvent::Shown(open) => {
                if self.page == Page::Quilts && self.quilt_open != *open {
                    let from = self.place();
                    self.quilt_open.clone_from(open);
                    self.went_from(from);
                }
            }
            patchwork_desktop_quilts::QuiltsEvent::AskFix { chat_id, text } => {
                if self.page == Page::Quilts {
                    self.open_quilt_chat(chat_id, window, cx);
                } else {
                    self.open_chat(chat_id, cx);
                }
                if self.chat.as_ref().is_some_and(|t| &t.id == chat_id) {
                    self.send(text.clone(), Vec::new(), None, None, cx);
                }
            }
            patchwork_desktop_quilts::QuiltsEvent::Picked { chat_id, text } => {
                if self.page == Page::Quilts {
                    self.open_quilt_chat(chat_id, window, cx);
                } else {
                    self.open_chat(chat_id, cx);
                }
                if self.chat.as_ref().is_some_and(|t| &t.id == chat_id) {
                    self.composer
                        .update(cx, |c, cx| c.fill(text, Vec::new(), window, cx));
                }
            }
        }
        cx.notify();
    }
}
