//! Chat tabs: several chats of one project kept open over the transcript, to switch between
//! without the task list.
//!
//! Each pane of a project keeps its own tabs, and the strips show the open chat's project's. The
//! open chat always has a tab, in the focused pane. Opening or starting a chat shows it in the tab
//! that was open, as a browser does, and "Open in new tab" in a chat's menu, or a strip's add
//! button, gives a chat a tab of its own beside the open one. A tab dragged out of its strip goes
//! to another pane, or splits one (see `panes/`). Each tab keeps one size, open or not, busy or
//! not.
//!
//! An agent's chat takes no tab: it shows alone, over the pane that had the focus, with no strip.

// Rust guideline compliant 2026-11-27

use crate::prelude::*;
use patchwork_client::AppState;
use std::sync::Arc;
use patchwork_views_logos::agent_logo;
use gpui::{
    AnyElement, ClickEvent, DragMoveEvent, MouseButton, MouseDownEvent, Role, SharedString, Window,
    div, prelude::*, px,
};
use patchwork_client::local::core::chats::{ApprovalStatus, Chat};
use patchwork_design_icons::IconName;
use patchwork_design_theme::{Theme, fs_sm, radius};
use patchwork_design_components::{button, icon_button, spinner, tooltip};
use patchwork_design_text::display_title;
use patchwork_design_icons::icon;

use crate::panes::{PaneId, TabDrag, Zone};
use crate::window::MenuTarget as Target;
use patchwork_views_chat::chat_drag::TAB_WIDTH;
use crate::window::{is_busy};

/// The strip's height, the same as the file views' tabs.
pub const STRIP_HEIGHT: f32 = 40.;

/// The window's tabs methods, for any [`ChatWindow`].
pub trait TabsExt: ChatWindow {
    /// Opens chat `id` in a new tab beside the open chat's.
    fn open_in_new_tab(&mut self, id: &str, cx: &mut Context<Self>) {
        self.new_tab = true;
        self.open_chat(id, cx);
    }

    /// Starts a task in `project` in a new tab beside the open chat's.
    fn new_task_tab(
        &mut self,
        project: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_tab = true;
        self.new_chat_in(project, None, window, cx);
        self.new_tab = false;
    }

    /// Gives the open chat a tab in the focused pane when it has none. It takes over `replace`'s
    /// tab when that pane has it, or goes right after it when `new`, and else goes last.
    fn keep_tab(&mut self, replace: Option<&str>, new: bool, cx: &gpui::App) {
        let Some(open) = self.chat.as_ref() else {
            return;
        };
        if open.agent_id.is_some() {
            return;
        }
        let (id, project) = (open.id.clone(), open.project_id.clone());
        // Leaving an agent's chat, which had no tab, the tab it was shown over is the open one.
        let from_agent = replace.is_some_and(|r| self.is_agent_chat(r, cx));
        let replace = replace.map(str::to_owned);
        let layout = self.layout_mut(project.as_deref());
        if let Some(pane) = layout.pane_of(&id).and_then(|p| layout.pane_mut(p)) {
            pane.shown = Some(id);
            return;
        }
        let pane = layout.focused_pane();
        let replace = if from_agent {
            pane.shown.clone()
        } else {
            replace
        };
        let slot = replace.and_then(|r| pane.tabs.iter().position(|t| *t == r));
        match slot {
            Some(at) if new => pane.tabs.insert(at + 1, id.clone()),
            Some(at) => pane.tabs[at].clone_from(&id),
            None => pane.tabs.push(id.clone()),
        }
        pane.shown = Some(id);
    }

    /// Takes chat `id` out of the tabs. Its pane shows its neighbour instead, and a pane left with
    /// no tabs closes; the open chat's last tab closes the chat when no other pane is left.
    fn close_tab(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some((key, pane_id)) = self
            .layouts
            .iter()
            .find_map(|(k, l)| l.pane_of(id).map(|p| (k.clone(), p)))
        else {
            return;
        };
        let open = self.chat.as_ref().is_some_and(|c| c.id == id);
        let Some(layout) = self.layouts.get(&key) else {
            return;
        };
        let Some(tabs) = layout.pane(pane_id).map(|p| p.tabs.clone()) else {
            return;
        };
        let focused = layout.focused == pane_id;
        let at = tabs.iter().position(|t| t == id).unwrap_or(0);
        // The tab after it, or else the one before, that is still in the task list.
        let next = {
            let (before, after) = tabs.split_at(at);
            after
                .iter()
                .skip(1)
                .chain(before.iter().rev())
                .find(|t| self.listed(t, cx))
                .cloned()
        };
        let Some(layout) = self.layouts.get_mut(&key) else {
            return;
        };
        let Some(pane) = layout.pane_mut(pane_id) else {
            return;
        };
        pane.tabs.remove(at);
        let closed = id.to_owned();
        if pane.shown.as_deref() == Some(id) {
            pane.shown.clone_from(&next);
            if pane.view.as_ref().and_then(|v| v.chat_id()) == Some(id) {
                pane.view = None;
            }
        }
        let empty = pane.tabs.is_empty() && layout.panes.len() > 1;
        if empty {
            layout.remove(pane_id);
        }
        self.remember_closed(&closed);
        if focused && (open || empty) {
            match next.filter(|_| !empty) {
                Some(next) => self.open_chat(&next, cx),
                None => self.show_focused_pane(key.as_deref(), cx),
            }
        }
        cx.notify();
    }

    /// Shows the chat of the focused pane of `project`'s layout, as it was left, or no chat when
    /// it has none.
    fn show_focused_pane(&mut self, project: Option<&str>, cx: &mut Context<Self>) {
        let pane = self.layout_mut(project).focused_pane();
        let (shown, slot) = (pane.shown.clone(), pane.view.take());
        match shown {
            Some(shown) => {
                self.open_chat(&shown, cx);
                if let Some(slot) = slot {
                    self.restore_view(slot);
                }
            }
            None => self.close_chat(cx),
        }
    }

    /// Whether chat `id` is in the task list and not archived.
    fn listed(&self, id: &str, cx: &gpui::App) -> bool {
        AppState::get(cx)
            .chats
            .iter()
            .any(|c| c.id == id && c.archived_at.is_none())
    }

    /// Whether chat `id` is an agent's, which takes no tab.
    fn is_agent_chat(&self, id: &str, cx: &gpui::App) -> bool {
        AppState::get(cx)
            .chats
            .iter()
            .any(|c| c.id == id && c.agent_id.is_some())
    }

    /// The project whose tabs show: the open chat's, or else the one picked for a new task.
    fn tabs_project(&self, cx: &gpui::App) -> Option<String> {
        match &self.chat {
            Some(chat) => chat.project_id.clone(),
            None => AppState::get(cx).settings.window.active_project_id.clone(),
        }
    }

    /// The chats of `tabs` still in the task list, in order.
    fn tab_chats<'a>(
        &'a self,
        tabs: &'a [String],
        chats: &'a [Arc<Chat>],
    ) -> impl Iterator<Item = &'a Chat> + 'a {
        tabs.iter()
            .filter_map(|id| chats.iter().find(|c| c.id == *id))
            .map(AsRef::as_ref)
            .filter(|c| c.archived_at.is_none() && c.agent_id.is_none())
    }

    /// The tasks of `project` in the task list that have no tab, in its order.
    fn untabbed_tasks<'a>(&'a self, project: Option<&str>, cx: &'a gpui::App) -> Vec<&'a Chat> {
        AppState::get(cx)
            .chats
            .iter()
            .map(AsRef::as_ref)
            .filter(|c| {
                c.archived_at.is_none()
                    && c.parent.is_none()
                    && c.agent_id.is_none()
                    && c.project_id.as_deref() == project
                    && !self.has_tab(&c.id)
            })
            .collect()
    }
}

impl<W: ChatWindow> TabsExt for W {}

/// The strip of pane `pane`'s tabs, led by the button that adds one. A pane alone shows it while
/// a chat is open or it has tabs; one of several always does. The top-right pane's ends with the
/// layout button.
pub fn tab_strip<W: ChatWindow>(
    w: &W,
    pane: PaneId,
    t: &Theme,
    cx: &mut Context<W>,
) -> Option<AnyElement> {
    if w.chat.as_ref().is_some_and(|c| c.agent_id.is_some()) {
        return None;
    }
    let project = w.tabs_project(cx);
    let layout = w.layouts.get(&project)?;
    let (focused, alone) = (layout.focused == pane, layout.panes.len() == 1);
    let corner = layout.maximized.unwrap_or_else(|| layout.root.top_right()) == pane;
    let ids = &layout.pane(pane)?.tabs;
    let chats = Arc::clone(&AppState::get(cx).chats);
    let tabs: Vec<&Chat> = w.tab_chats(ids, &chats).collect();
    if alone && w.chat.is_none() && tabs.is_empty() {
        return None;
    }
    let open = if focused {
        w.chat.as_ref().map(|c| c.id.clone())
    } else {
        layout.pane(pane).and_then(|p| p.shown.clone())
    };
    let count = tabs.len();
    let drop_bg = t.bg_hover;
    let strip = div()
        .id(SharedString::from(format!("chat-tabs-{pane}")))
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
        .drag_over::<TabDrag>(move |s, _, _, _| s.bg(drop_bg))
        .on_drop(cx.listener(move |w, drag: &TabDrag, _, cx| {
            w.tab_drop_before = None;
            w.dock_tab(&drag.chat, pane, Zone::Middle, cx);
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |w, e: &MouseDownEvent, window, cx| {
                w.open_context_menu(Target::Pane(pane), e.position, window, cx);
            }),
        )
        .child(
            icon_button(
                SharedString::from(format!("chat-tab-add-{pane}")),
                "Add a tab",
                IconName::Plus,
                14.,
                cx,
            )
            .flex_none()
            .tooltip("Add a tab")
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |w, e: &MouseDownEvent, window, cx| {
                    let target = Target::Tabs(project.clone());
                    w.open_context_menu(target, e.position, window, cx);
                }),
            ),
        )
        .children(tabs.into_iter().enumerate().map(|(i, chat)| {
            let on = open.as_deref() == Some(chat.id.as_str());
            let drop_here =
                cx.has_active_drag() && w.tab_drop_before.as_deref() == Some(chat.id.as_str());
            tab(chat, (on, focused, drop_here), (i, count), t, cx)
        }))
        .when(focused && w.broadcasting && !alone, |s| {
            s.child(div().flex_1()).child(broadcast_pill(t, cx))
        })
        .when(corner, |s| {
            s.child(div().flex_1())
                .when(layout.maximized.is_some(), |s| s.child(restore_button(cx)))
                .child(layouts_button(cx))
        });
    Some(strip.into_any_element())
}

/// The button at the right of the top-right pane's tabs that lays the panes out as a preset.
fn layouts_button<W: ChatWindow>(cx: &mut Context<W>) -> impl IntoElement + use<W> {
    icon_button("chat-layouts", "Layouts", IconName::SquareGridFour, 14., cx)
        .flex_none()
        .tooltip("Layouts")
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|w, e: &MouseDownEvent, window, cx| {
                w.open_context_menu(Target::Layouts, e.position, window, cx);
            }),
        )
}

/// The mark on the focused pane's tabs while each message goes to every pane, that turns it off.
fn broadcast_pill<W: ChatWindow>(t: &Theme, cx: &mut Context<W>) -> impl IntoElement + use<W> {
    let hover = t.bg_hover;
    button("chat-broadcast", "Sending to every pane. Stop")
        .flex_none()
        .h(px(24.))
        .px(px(8.))
        .gap(px(6.))
        .rounded(px(radius()))
        .border_1()
        .border_color(t.accent)
        .text_size(px(fs_sm()))
        .text_color(t.accent_text)
        .hover(move |s| s.bg(hover))
        .child(icon(IconName::PaperPlaneRight, 12.))
        .child("To every pane")
        .child(icon(IconName::X, 10.))
        .tooltip(tooltip("Each message goes to every pane. Click to stop"))
        .on_click(cx.listener(|w, _, _, cx| w.toggle_broadcast(cx)))
}

/// The button beside the layout button while one pane shows alone, that shows every pane again.
fn restore_button<W: ChatWindow>(cx: &mut Context<W>) -> impl IntoElement + use<W> {
    icon_button(
        "chat-restore-panes",
        "Show all panes",
        IconName::ArrowsInSimple,
        14.,
        cx,
    )
    .flex_none()
    .tooltip("Show all panes")
    .on_click(cx.listener(|w, _, _, cx| w.toggle_maximized(cx)))
}

/// One tab: the agent's logo, a spinner while it replies, or a shield while it waits for an
/// answer, the title, and its close button.
/// Right-click opens the chat's menu, as on its row in the task list, and a double-click shows
/// its pane alone. A tab dropped on it goes before it; one held over it a moment shows it.
///
/// `on` is whether it is its pane's shown tab, `focused` whether that pane has the focus, and
/// `drop_here` whether a dragged tab would land before it; `i` of `count` is its place.
fn tab<W: ChatWindow>(
    chat: &Chat,
    (on, focused, drop_here): (bool, bool, bool),
    (i, count): (usize, usize),
    t: &Theme,
    cx: &mut Context<W>,
) -> impl IntoElement + use<W> {
    let (fg, muted, hover) = (t.text, t.text_muted, t.bg_hover);
    // The focused pane's open tab stands out; another pane's shown tab only a little.
    let selected = if focused {
        t.bg_selected_muted
    } else {
        t.bg_hover
    };
    let title = display_title(&chat.title);
    let title = if title.trim().is_empty() {
        "New chat".to_owned()
    } else {
        title
    };
    let (id, close_id, menu_id) = (chat.id.clone(), chat.id.clone(), chat.id.clone());
    let (onto, hover_id, accent) = (chat.id.clone(), chat.id.clone(), t.accent);
    let waiting = chat.messages.last().is_some_and(|m| {
        m.approvals
            .iter()
            .any(|a| a.status == ApprovalStatus::Pending)
    });
    let mark = if waiting {
        div()
            .id("tab-waiting")
            .role(Role::Status)
            .aria_label("Waiting for you")
            .child(icon(IconName::ShieldWarning, 13.).text_color(t.warning))
            .into_any_element()
    } else if is_busy(chat) {
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
    .relative()
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
    // Where a dragged tab lands: a straight line in the gap before this tab.
    .child(
        div()
            .absolute()
            .left(px(-3.))
            .top_0()
            .bottom_0()
            .w(px(2.))
            .when(drop_here, |l| l.bg(accent)),
    )
    .child(div().flex_1().min_w_0().truncate().child(title.clone()))
    .child(
        icon_button(
            SharedString::from(format!("chat-tab-close-{}", chat.id)),
            format!("Close tab {title}"),
            IconName::X,
            12.,
            cx,
        )
        .flex_none()
        .size(px(20.))
        .tooltip("Close tab")
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
            // The strip under it has a menu of its own.
            cx.stop_propagation();
            w.open_context_menu(Target::Chat(menu_id.clone()), e.position, window, cx);
        }),
    )
    .on_drop(cx.listener(move |w, drag: &TabDrag, _, cx| {
        w.tab_drop_before = None;
        w.drop_tab_before(&drag.chat, &onto, cx);
    }))
    .on_drag_move::<TabDrag>(
        cx.listener(move |w, e: &DragMoveEvent<TabDrag>, window, cx| {
            let over = e.bounds.contains(&e.event.position);
            let own = e.drag(cx).chat == hover_id;
            let here = w.tab_drop_before.as_deref() == Some(hover_id.as_str());
            if over && !own {
                if !here {
                    w.tab_drop_before = Some(hover_id.clone());
                    cx.notify();
                }
                w.hold_tab_over(&hover_id, e.bounds, window, cx);
            } else if here {
                w.tab_drop_before = None;
                cx.notify();
            }
        }),
    )
    .on_drag(
        TabDrag {
            chat: chat.id.clone(),
            title: title.clone().into(),
        },
        |drag, _, _, cx| {
            let title = drag.title.clone();
            cx.new(|_| TabGhost(title))
        },
    )
    .on_click(cx.listener(move |w, e: &ClickEvent, _, cx| {
        if e.click_count() == 2 {
            w.toggle_maximized(cx);
        } else {
            w.open_chat(&id, cx);
        }
    }))
}

/// What follows the pointer while a tab, or a task from the sidebar, is dragged: its title in a
/// tab's shape.
pub use patchwork_views_chat::chat_drag::ChatGhost as TabGhost;
