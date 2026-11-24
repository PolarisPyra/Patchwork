//! Chat tabs: several chats of one project kept open side by side over the transcript, to
//! switch between without the task list.
//!
//! A chat goes into a tab with "Send to tab" in its menu, from the task list or from another
//! tab; the chat open then gets a tab too. The strip shows while the open chat's project has any
//! tabs, whichever of its chats is open, so it does not come and go while moving between them,
//! and the tab just sent to shows at once. Each tab keeps one size, open or not, busy or not.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, MouseButton, MouseDownEvent, Role, SharedString, div, prelude::*, px};
use patchwork_core::chats::Chat;
use patchwork_ui::theme::{Theme, fs_sm, radius};
use patchwork_ui::{agent_logo, button, display_title, icon_button, spinner, tooltip};

use crate::sidebar::tasks::row_menu::Target;
use crate::workspace::{Workspace, is_busy};

/// Every tab's width, so opening, closing or a reply starting moves nothing.
const TAB_WIDTH: f32 = 180.;

/// The strip's height, the same as the file views' tabs.
const STRIP_HEIGHT: f32 = 40.;

impl Workspace {
    /// Puts chat `id` in a tab beside the open chat's, giving the open chat a tab too, and
    /// opens it.
    pub(crate) fn send_to_tab(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.chat_tabs.iter().any(|t| t == id) {
            let open = self
                .chat
                .as_ref()
                .map(|c| (c.id.clone(), c.project_id.clone()));
            let project = self
                .chats
                .iter()
                .find(|c| c.id == id)
                .map(|c| &c.project_id);
            let at = match open {
                // Beside the open chat, when it is of the same project.
                Some((open, of)) if Some(&of) == project && open != id => {
                    let at = self.chat_tabs.iter().position(|t| *t == open);
                    at.unwrap_or_else(|| {
                        self.chat_tabs.push(open);
                        self.chat_tabs.len() - 1
                    }) + 1
                }
                _ => self.chat_tabs.len(),
            };
            self.chat_tabs.insert(at, id.to_owned());
        }
        self.open_chat(id, cx);
    }

    /// Takes chat `id` out of the tabs, showing its neighbour when it was the one open.
    pub(crate) fn close_tab(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(at) = self.chat_tabs.iter().position(|t| t == id) else {
            return;
        };
        self.chat_tabs.remove(at);
        let open = self.chat.as_ref().is_some_and(|c| c.id == id);
        let project = self.chat.as_ref().and_then(|c| c.project_id.clone());
        if open {
            // The tab after it, or else the one before, of the same project.
            let (before, after) = self.chat_tabs.split_at(at);
            let next = after
                .iter()
                .chain(before.iter().rev())
                .find(|t| self.listed_in(t, project.as_deref()))
                .cloned();
            if let Some(next) = next {
                self.open_chat(&next, cx);
            }
        }
        cx.notify();
    }

    /// Whether chat `id` is in the task list, not archived, and of `project`.
    fn listed_in(&self, id: &str, project: Option<&str>) -> bool {
        self.chats
            .iter()
            .any(|c| c.id == id && c.archived_at.is_none() && c.project_id.as_deref() == project)
    }

    /// The tabs of the open chat's project, in order, when there are any.
    fn shown_tabs(&self) -> Option<Vec<&Chat>> {
        let open = self.chat.as_ref()?;
        let tabs: Vec<&Chat> = self
            .chat_tabs
            .iter()
            .filter_map(|id| self.chats.iter().find(|c| c.id == *id))
            .map(AsRef::as_ref)
            .filter(|c| c.archived_at.is_none() && c.project_id == open.project_id)
            .collect();
        (!tabs.is_empty()).then_some(tabs)
    }
}

/// The strip of the open chat's project's tabs, when it has any.
pub(super) fn tab_strip(
    w: &Workspace,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Option<AnyElement> {
    let tabs = w.shown_tabs()?;
    let open = w.chat.as_ref().map(|c| c.id.clone());
    let count = tabs.len();
    let strip = div()
        .id("chat-tabs")
        .role(Role::TabList)
        .aria_label("Chat tabs")
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.))
        .h(px(STRIP_HEIGHT))
        .px(px(12.))
        .border_b_1()
        .border_color(t.border)
        .overflow_x_scroll()
        .children(tabs.into_iter().enumerate().map(|(i, chat)| {
            let on = open.as_deref() == Some(chat.id.as_str());
            tab(chat, on, i, count, t, cx)
        }));
    Some(strip.into_any_element())
}

/// One tab: the agent's logo, or a spinner while it replies, the title, and its close button.
/// Right-click opens the chat's menu, as on its row in the task list.
fn tab(
    chat: &Chat,
    on: bool,
    i: usize,
    count: usize,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> impl IntoElement + use<> {
    let (fg, muted, hover, selected) = (t.text, t.text_muted, t.bg_hover, t.bg_selected_muted);
    let title = display_title(&chat.title);
    let title = if title.trim().is_empty() {
        "New chat".to_owned()
    } else {
        title
    };
    let (id, close_id, menu_id) = (chat.id.clone(), chat.id.clone(), chat.id.clone());
    let mark = if is_busy(chat) {
        div()
            .id("tab-busy")
            .role(Role::Status)
            .aria_label("Working")
            .child(spinner(12., muted))
            .into_any_element()
    } else {
        agent_logo(chat.agent, chat.model.as_deref(), 12.).into_any_element()
    };
    button(
        SharedString::from(format!("chat-tab-{}", chat.id)),
        format!("{title}, tab {} of {count}", i + 1),
    )
    .role(Role::Tab)
    .aria_selected(on)
    .group("chat-tab")
    .flex_none()
    .w(px(TAB_WIDTH))
    .h(px(28.))
    .gap(px(6.))
    .pl(px(10.))
    .pr(px(4.))
    .rounded(px(radius()))
    .text_size(px(fs_sm()))
    .map(|b| {
        if on {
            b.bg(selected).text_color(fg)
        } else {
            b.text_color(muted)
                .hover(move |s| s.bg(hover).text_color(fg))
        }
    })
    .child(
        div()
            .flex_none()
            .size(px(14.))
            .flex()
            .items_center()
            .justify_center()
            .child(mark),
    )
    .child(div().flex_1().min_w_0().truncate().child(title.clone()))
    .child(
        icon_button(
            SharedString::from(format!("chat-tab-close-{}", chat.id)),
            format!("Close tab {title}"),
            "x",
            12.,
            cx,
        )
        .flex_none()
        .size(px(20.))
        .tooltip(tooltip("Close tab"))
        .when(!on, |b| {
            b.opacity(0.)
                .group_hover("chat-tab", |s| s.opacity(1.))
                .focus(|s| s.opacity(1.))
        })
        .on_click(cx.listener(move |w, _, _, cx| {
            cx.stop_propagation();
            w.close_tab(&close_id, cx);
        })),
    )
    .on_mouse_down(
        MouseButton::Right,
        cx.listener(move |w, e: &MouseDownEvent, window, cx| {
            w.open_context_menu(Target::Chat(menu_id.clone()), e, window, cx);
        }),
    )
    .on_click(cx.listener(move |w, _, _, cx| w.open_chat(&id, cx)))
}
