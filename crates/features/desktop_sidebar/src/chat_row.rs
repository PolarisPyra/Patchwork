//! The rows of the sidebar's tree: a chat's row and its indent guides.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, ClickEvent, DragMoveEvent, Div, Hsla, MouseButton, MouseDownEvent, Role,
    SharedString, Stateful, div, prelude::*, px,
};
use patchwork_model::chats::{ApprovalStatus, Chat, MessageStatus};
use patchwork_client::local::core::gateway::{ChatApps, Platform, Presence};
use patchwork_client::local::core::worktree::WorktreeMark;
use patchwork_client::AppState;
use patchwork_platform::now_ms;
use patchwork_design_components::{avatar::Mood, avatar::avatar};
use patchwork_views_chat::{chat_drag::ChatDrag, chat_drag::ChatGhost};
use patchwork_design_components::{button, spinner};
use patchwork_design_text::{display_title, format_relative};
use patchwork_views_logos::{agent_label, agent_logo};
use patchwork_design_icons::{IconName, icon};
use patchwork_design_theme::{Theme, fs_sm, radius_sm, theme};

use crate::{Row, Sidebar};

/// The scrollbar's gutter at the tree's right edge, which its thumb sits in.
pub(crate) const TREE_GUTTER: f32 = 10.;

/// The line from a project's or folder's chevron down through its rows, shown while the tree is
/// hovered.
pub(crate) fn indent_guide(left: f32) -> Div {
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left(px(left))
        .w(px(1.))
        .invisible()
        .group_hover("tree", gpui::Styled::visible)
        .bg(gpui::rgba(0x5858_5866))
}

/// A project or folder row.
pub(crate) fn tree_row(
    id: SharedString,
    name: String,
    level: usize,
    open: bool,
    picked: bool,
    t: &Theme,
) -> Stateful<Div> {
    let (fill, hover) = if picked {
        (t.bg_accent_soft, t.tree_selected)
    } else {
        (gpui::transparent_black(), t.tree_hover)
    };
    div()
        .id(id)
        .role(Role::TreeItem)
        .aria_level(level)
        .aria_label(name)
        .aria_expanded(open)
        .aria_selected(picked)
        .tab_index(0)
        .mx(px(4.))
        .mb(px(2.))
        .h(px(patchwork_design_theme::roomy(28.)))
        .flex_none()
        .flex()
        .items_center()
        .cursor_pointer()
        .rounded(px(radius_sm()))
        .pr(px(6.))
        .text_color(t.text)
        .bg(fill)
        .hover(move |s| s.bg(hover))
}

/// A 24px icon button a row shows while hovered.
pub(crate) fn reveal_button(id: &str, label: String, name: IconName, t: &Theme) -> Stateful<Div> {
    let (hover, fg) = (t.bg_hover, t.text);
    button(SharedString::from(format!("reveal:{id}")), label)
        .size(px(24.))
        .flex_none()
        .justify_center()
        .rounded(px(radius_sm()))
        .text_color(t.text_muted)
        .hover(move |s| s.bg(hover).text_color(fg))
        .child(icon(name, 17.))
}

/// The sidebar's mark on a task's worktree, and what it says. None for a worktree with nothing
/// to do.
fn worktree_mark(mark: WorktreeMark, t: &Theme) -> Option<(AnyElement, &'static str)> {
    let (label, color, name) = match mark {
        WorktreeMark::Unmerged => ("Unmerged changes", t.accent_text, IconName::GitPullRequest),
        WorktreeMark::OffBranch => ("Worktree on another branch", t.warning, IconName::Warning),
        WorktreeMark::Clean => return None,
    };
    let element = div()
        .id("worktree-mark")
        .role(Role::Image)
        .aria_label(label)
        .flex_none()
        .flex()
        .items_center()
        .text_color(color)
        .child(icon(name, 13.))
        .into_any_element();
    Some((element, label))
}

/// A chat or task with its time, or what it is waiting on, and its agent's mark.
pub(crate) fn chat_row(
    w: &Sidebar,
    th: &Chat,
    depth: usize,
    selected: Option<&str>,
    cx: &mut Context<Sidebar>,
) -> impl IntoElement {
    let t = *theme(cx);
    let target = Row::Chat(th.id.clone());
    let picked = selected == Some(th.id.as_str()) || w.is_picked(&target);
    let last = th.messages.last();
    let needs_approval = last.is_some_and(|m| {
        m.approvals
            .iter()
            .any(|a| a.status == ApprovalStatus::Pending)
    });
    let busy = th.is_busy();
    let title = display_title(&th.title);
    let (ml, mr) = match depth {
        0 => (-2., -4.),
        1 => (30., 4.),
        _ => (38., 4.),
    };
    let group = SharedString::from(format!("row:{}", th.id));
    let id = th.id.clone();
    let archive_id = th.id.clone();
    let (hovered, hover_id) = (
        w.hovered_row.as_deref() == Some(th.id.as_str()),
        th.id.clone(),
    );
    let (fill, hover) = if picked {
        (t.bg_accent_soft, t.tree_selected)
    } else {
        (gpui::transparent_black(), t.tree_hover)
    };
    let agent = th
        .agent_id
        .as_ref()
        .and_then(|id| AppState::get(cx).agents.iter().find(|b| &b.id == id));
    let mark: Option<AnyElement> = if let Some(agent) = agent {
        let mood = if needs_approval {
            Mood::Waiting
        } else if busy {
            Mood::Working
        } else if last.is_some_and(|m| m.status == MessageStatus::Error) {
            Mood::Error
        } else {
            Mood::Idle
        };
        Some(avatar(&agent.id, &agent.name, mood, 16.).into_any_element())
    } else if th.favorite {
        Some(
            icon(IconName::StarFill, 13.)
                .text_color(t.text_muted)
                .into_any_element(),
        )
    } else if th.pinned {
        Some(
            icon(IconName::PushPinFill, 13.)
                .text_color(t.text_muted)
                .into_any_element(),
        )
    } else {
        None
    };
    // The spinner and shield sit in the archive button's 20px box, so hovering swaps one for the
    // other in place. The spinner shows a task working in a project whose tabs are not open.
    let state: AnyElement = if needs_approval || busy {
        let mark = if needs_approval {
            icon(IconName::ShieldWarning, 15.)
                .text_color(t.warning)
                .into_any_element()
        } else {
            spinner(12., t.accent_text).into_any_element()
        };
        // At the box's right edge, in line with the times of the rows around it.
        div()
            .size(px(20.))
            .flex()
            .items_center()
            .justify_end()
            .child(mark)
            .into_any_element()
    } else {
        let at = last.map_or(th.created_at, |m| m.created_at);
        let color: Hsla = if picked { t.text_muted } else { t.text_subtle };
        div()
            .text_size(px(fs_sm()))
            .text_color(color)
            .child(format_relative(at))
            .into_any_element()
    };
    let pick = target.clone();
    let renaming = w.rename_field(&target, cx);
    let drag = renaming.is_none().then(|| ChatDrag {
        chat: th.id.clone(),
        title: title.clone().into(),
    });
    let name = renaming.unwrap_or_else(|| {
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .child(title.clone())
            .into_any_element()
    });
    let (btn_hover, fg) = (t.border_strong, t.text);
    let drop_here = cx.has_active_drag() && w.task_drop_before.as_deref() == Some(th.id.as_str());
    let (drop_onto, line_id) = (th.id.clone(), th.id.clone());
    let worktree = w
        .host
        .worktree_mark(&th.id, cx)
        .and_then(|mark| worktree_mark(mark, &t));
    let label: SharedString = match &worktree {
        Some((_, says)) => format!("{title}, {}", says.to_lowercase()).into(),
        None => title.clone().into(),
    };
    div()
        .id(SharedString::from(format!("chat:{}", th.id)))
        .group(group.clone())
        .role(Role::TreeItem)
        .aria_level(depth + 1)
        .aria_selected(picked)
        .aria_label(label)
        .tab_index(0)
        .h(px(patchwork_design_theme::roomy(28.)))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.))
        .ml(px(ml))
        .mr(px(mr))
        .mb(px(2.))
        .pl(px(8.))
        .pr(px(8.))
        .rounded(px(radius_sm()))
        .cursor_pointer()
        .text_color(t.text)
        .bg(fill)
        .hover(move |s| s.bg(hover))
        .relative()
        // Where a task dragged in the tree lands: a straight line in the gap above this row.
        .when(drop_here, |row| {
            row.child(
                div()
                    .absolute()
                    .top(px(-2.))
                    .left_0()
                    .right_0()
                    .h(px(2.))
                    .bg(t.accent),
            )
        })
        .on_drop(cx.listener(move |w, drag: &ChatDrag, _, cx| {
            w.drop_task_before(&drag.chat, &drop_onto, cx);
        }))
        .on_drag_move::<ChatDrag>(cx.listener(move |w, e: &DragMoveEvent<ChatDrag>, _, cx| {
            let over = e.bounds.contains(&e.event.position);
            let fits = over
                && AppState::get(cx)
                    .chats
                    .iter()
                    .find(|c| c.id == line_id)
                    .is_some_and(|c| w.can_drop_task(&e.drag(cx).chat, c, cx));
            let here = w.task_drop_before.as_deref() == Some(line_id.as_str());
            if fits && !here {
                w.task_drop_before = Some(line_id.clone());
                cx.notify();
            } else if !fits && here {
                w.task_drop_before = None;
                cx.notify();
            }
        }))
        .children(mark)
        .child(name)
        .child(
            div()
                .w(px(32.))
                .flex_none()
                .flex()
                .items_center()
                .justify_end()
                .when(!hovered, |s| {
                    s.child(div().flex().items_center().child(state))
                })
                .child(
                    button(
                        SharedString::from(format!("archive:{}", th.id)),
                        format!("Archive {title}"),
                    )
                    .absolute()
                    .size(px(20.))
                    .justify_center()
                    .rounded(px(radius_sm()))
                    .text_color(t.text_muted)
                    .hover(move |s| s.bg(btn_hover).text_color(fg))
                    .opacity(0.)
                    .group_hover(group, |s| s.opacity(1.))
                    .focus(|s| s.opacity(1.))
                    .child(icon(IconName::Archive, 15.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.archive(&archive_id, cx);
                    })),
                ),
        )
        .children(worktree.map(|(mark, _)| div().w(px(13.)).flex_none().child(mark)))
        // Dragged onto a pane, the task opens there as a tab, or in a new pane at its edge.
        .when_some(drag, |row, drag| {
            row.on_drag(drag, |drag, _, _, cx| {
                let title = drag.title.clone();
                cx.new(|_| ChatGhost(title))
            })
        })
        .children(chat_app_mark(
            AppState::get(cx).chat_apps.get(&th.id),
            &t,
            cx,
        ))
        .child(
            div()
                .id("agent")
                .role(Role::Image)
                .size(px(16.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .aria_label(agent_label(th.agent))
                .child(agent_logo(th.agent, th.model.as_deref(), 14.)),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                let row = this.menu_row(target.clone());
                this.open_menu(row, e, window, cx);
            }),
        )
        .on_hover(cx.listener(move |this, over: &bool, _, cx| {
            let now = over.then(|| hover_id.clone());
            if *over || this.hovered_row.as_ref() == Some(&hover_id) {
                this.hovered_row = now;
                cx.notify();
            }
        }))
        .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
            if !this.pick_row(&pick, e.modifiers(), cx) {
                this.open_chat(&id, cx);
            }
        }))
}

/// The logo of the chat app that is on a chat now, dimmed once it has gone unused a while.
fn chat_app_mark(apps: Option<&ChatApps>, t: &Theme, cx: &gpui::App) -> Option<AnyElement> {
    let apps = apps?;
    let platform = apps.conversations.first()?.platform;
    let mut names: Vec<&str> = apps
        .conversations
        .iter()
        .map(|b| b.platform.label())
        .collect();
    names.dedup();
    let presence = chat_app_presence(platform, apps.in_use(now_ms()), cx);
    let label = format!(
        "{} is on this chat, {}",
        names.join(", "),
        presence_word(presence)
    );
    Some(phone_mark("chat-app", label, presence, t))
}

/// How chat app `platform` shows: its connection, and whether a conversation used it lately.
pub(crate) fn chat_app_presence(platform: Platform, in_use: bool, cx: &gpui::App) -> Presence {
    AppState::get(cx)
        .chat_apps
        .status
        .iter()
        .find(|(p, _)| *p == platform)
        .map_or(Presence::Offline, |(_, status)| status.presence(in_use))
}

/// The word a screen reader says for `presence`.
pub(crate) fn presence_word(presence: Presence) -> &'static str {
    match presence {
        Presence::Online => "online",
        Presence::Away => "away",
        Presence::Offline => "offline",
    }
}

/// A mobile device in green when online, yellow when away and red when offline, named `label`.
pub(crate) fn phone_mark(
    id: &'static str,
    label: String,
    presence: Presence,
    t: &Theme,
) -> AnyElement {
    let colour = match presence {
        Presence::Online => t.success,
        Presence::Away => t.warning,
        Presence::Offline => t.danger,
    };
    div()
        .id(id)
        .role(Role::Image)
        .aria_label(label)
        .size(px(16.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .text_color(colour)
        .child(icon(IconName::DeviceMobile, 14.))
        .into_any_element()
}
