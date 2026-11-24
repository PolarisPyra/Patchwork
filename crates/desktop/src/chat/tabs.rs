//! Chat tabs: several chats of one project kept open side by side over the transcript, to
//! switch between without the task list.
//!
//! Each project keeps its own tabs, and the strip shows the open chat's project's. The open chat
//! always has a tab. Opening or starting a chat shows it in the tab that was open, as a browser
//! does, and "Open in new tab" in a chat's menu, or the strip's add button, gives a chat a tab of
//! its own beside the open one. Closing the open chat's last tab closes the chat. Each tab keeps one size, open or not, busy or not.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, MouseButton, MouseDownEvent, Role, SharedString, Window, div, prelude::*, px};
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
    /// Opens chat `id` in a new tab beside the open chat's.
    pub(crate) fn open_in_new_tab(&mut self, id: &str, cx: &mut Context<Self>) {
        self.new_tab = true;
        self.open_chat(id, cx);
    }

    /// Starts a task in `project` in a new tab beside the open chat's.
    pub(crate) fn new_task_tab(
        &mut self,
        project: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_tab = true;
        self.new_chat_in(project, None, window, cx);
        self.new_tab = false;
    }

    /// Gives the open chat a tab when it has none. It takes over `replace`'s tab when that is of
    /// the same project, or goes right after it when `new`, and else goes last.
    pub(crate) fn keep_tab(&mut self, replace: Option<&str>, new: bool) {
        let Some(open) = self.chat.as_ref() else {
            return;
        };
        if self.chat_tabs.contains(&open.id) {
            return;
        }
        let (id, project) = (open.id.clone(), open.project_id.clone());
        // A chat deleted since has no project to differ in, and its tab is free to take.
        let slot = replace
            .filter(|r| {
                self.chats
                    .iter()
                    .find(|c| c.id == *r)
                    .is_none_or(|c| c.project_id == project)
            })
            .and_then(|r| self.chat_tabs.iter().position(|t| t == r));
        match slot {
            Some(at) if new => self.chat_tabs.insert(at + 1, id),
            Some(at) => self.chat_tabs[at] = id,
            None => self.chat_tabs.push(id),
        }
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
            // The open chat always has a tab, so closing the last one closes the chat.
            match next {
                Some(next) => self.open_chat(&next, cx),
                None => self.close_chat(cx),
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

    /// The project whose tabs show: the open chat's, or else the one picked for a new task.
    fn tabs_project(&self) -> Option<String> {
        match &self.chat {
            Some(chat) => chat.project_id.clone(),
            None => self.settings.active_project_id.clone(),
        }
    }

    /// The tabs of `project`, in order.
    fn project_tabs(&self, project: Option<&str>) -> Vec<&Chat> {
        self.chat_tabs
            .iter()
            .filter_map(|id| self.chats.iter().find(|c| c.id == *id))
            .map(AsRef::as_ref)
            .filter(|c| c.archived_at.is_none() && c.project_id.as_deref() == project)
            .collect()
    }

    /// The tasks of `project` in the task list that have no tab, in its order.
    pub(crate) fn untabbed_tasks(&self, project: Option<&str>) -> Vec<&Chat> {
        self.chats
            .iter()
            .map(AsRef::as_ref)
            .filter(|c| {
                c.archived_at.is_none()
                    && c.parent.is_none()
                    && c.project_id.as_deref() == project
                    && !self.chat_tabs.contains(&c.id)
            })
            .collect()
    }
}

/// The strip of a project's tabs, led by the button that adds one. It shows while a chat is open,
/// or while the project picked for a new task has tabs.
pub(super) fn tab_strip(
    w: &Workspace,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Option<AnyElement> {
    let project = w.tabs_project();
    let tabs = w.project_tabs(project.as_deref());
    if w.chat.is_none() && tabs.is_empty() {
        return None;
    }
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
        .child(
            icon_button("chat-tab-add", "Add a tab", "plus", 14., cx)
                .flex_none()
                .tooltip(tooltip("Add a tab"))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |w, e: &MouseDownEvent, window, cx| {
                        let target = Target::Tabs(project.clone());
                        w.open_context_menu(target, e, window, cx);
                    }),
                ),
        )
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
