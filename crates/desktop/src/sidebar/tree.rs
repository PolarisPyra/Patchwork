//! The rows of the sidebar's tree: a chat's row and its indent guides.

// Rust guideline compliant 2026-10-09

use super::menu::Target;
use gpui::{
    AnyElement, ClickEvent, Div, Hsla, MouseButton, MouseDownEvent, Role, SharedString, Stateful,
    div, prelude::*, px,
};
use patchwork_server::threads::{ApprovalStatus, Thread};
use crate::ui::theme::{RADIUS_SM, Theme, theme};
use crate::ui::{agent_label, agent_logo, button, display_title, format_relative, icon, spinner};
use crate::workspace::{Workspace, is_busy};
use super::render::FS_XS;

/// The scrollbar's gutter at the tree's right edge, which its thumb sits in.
pub(super) const TREE_GUTTER: f32 = 10.;

/// The line from a project's or folder's chevron down through its rows, shown while the tree is
/// hovered.
pub(super) fn indent_guide(left: f32) -> Div {
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

/// A project or folder row, which notes when the pointer is over it so it can show its buttons.
pub(super) fn tree_row(
    id: SharedString,
    name: String,
    level: usize,
    open: bool,
    picked: bool,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let (fill, hover) = if picked {
        (t.bg_accent_soft, t.tree_selected)
    } else {
        (gpui::transparent_black(), t.tree_hover)
    };
    let row = id.clone();
    div()
        .id(id)
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            if *hovered {
                this.hovered_row = Some(row.clone());
            } else if this.hovered_row.as_ref() == Some(&row) {
                this.hovered_row = None;
            }
            cx.notify();
        }))
        .role(Role::TreeItem)
        .aria_level(level)
        .aria_label(name)
        .aria_expanded(open)
        .aria_selected(picked)
        .tab_index(0)
        .mx(px(4.))
        .mb(px(2.))
        .h(px(28.))
        .flex_none()
        .flex()
        .items_center()
        .cursor_pointer()
        .rounded(px(RADIUS_SM))
        .pr(px(6.))
        .text_color(t.text)
        .bg(fill)
        .hover(move |s| s.bg(hover))
}

/// A 20px icon button a row shows while hovered.
pub(super) fn reveal_button(id: &str, label: String, name: &str, t: &Theme) -> Stateful<Div> {
    let (hover, fg) = (t.bg_hover, t.text);
    button(SharedString::from(format!("reveal:{id}")), label)
        .size(px(20.))
        .flex_none()
        .justify_center()
        .rounded(px(RADIUS_SM))
        .text_color(t.text_muted)
        .hover(move |s| s.bg(hover).text_color(fg))
        .child(icon(name, 17.))
}

/// `ThreadRow`: a chat or task with its time, or what it is waiting on, and its agent's mark.
pub(super) fn thread_row(
    w: &Workspace,
    th: &Thread,
    depth: usize,
    selected: Option<&str>,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let t = *theme(cx);
    let target = Target::Thread(th.id.clone());
    let picked = selected == Some(th.id.as_str()) || w.is_picked(&target);
    let last = th.messages.last();
    let needs_approval = last.is_some_and(|m| {
        m.approvals
            .iter()
            .any(|a| a.status == ApprovalStatus::Pending)
    });
    let busy = is_busy(th);
    let title = display_title(&th.title);
    let (ml, mr) = match depth {
        0 => (-2., -4.),
        1 => (30., 4.),
        _ => (38., 4.),
    };
    let group = SharedString::from(format!("row:{}", th.id));
    let id = th.id.clone();
    let archive_id = th.id.clone();
    let (fill, hover) = if picked {
        (t.bg_accent_soft, t.tree_selected)
    } else {
        (gpui::transparent_black(), t.tree_hover)
    };
    let mark: Option<AnyElement> = if th.favorite {
        Some(
            icon("star-fill", 13.)
                .text_color(t.text_muted)
                .into_any_element(),
        )
    } else if th.pinned {
        Some(
            icon("push-pin-fill", 13.)
                .text_color(t.text_muted)
                .into_any_element(),
        )
    } else {
        None
    };
    let state: AnyElement = if needs_approval {
        icon("shield-warning", 15.)
            .text_color(t.warning)
            .into_any_element()
    } else if busy {
        spinner(12., t.accent_text).into_any_element()
    } else {
        let at = last.map_or(th.created_at, |m| m.created_at);
        let color: Hsla = if picked { t.text_muted } else { t.text_subtle };
        div()
            .text_size(px(FS_XS))
            .text_color(color)
            .child(format_relative(at))
            .into_any_element()
    };
    let pick = target.clone();
    let name = w.rename_field(&target, cx).unwrap_or_else(|| {
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .child(title.clone())
            .into_any_element()
    });
    let (btn_hover, fg) = (t.border_strong, t.text);
    div()
        .id(SharedString::from(format!("thread:{}", th.id)))
        .group(group.clone())
        .role(Role::TreeItem)
        .aria_level(depth + 1)
        .aria_selected(picked)
        .aria_label(title.clone())
        .tab_index(0)
        .h(px(28.))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.))
        .ml(px(ml))
        .mr(px(mr))
        .mb(px(2.))
        .pl(px(8.))
        .pr(px(8.))
        .rounded(px(RADIUS_SM))
        .cursor_pointer()
        .text_color(t.text)
        .bg(fill)
        .hover(move |s| s.bg(hover))
        .children(mark)
        .child(name)
        .child(
            div()
                .w(px(32.))
                .flex_none()
                .flex()
                .items_center()
                .justify_end()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .group_hover(group.clone(), gpui::Styled::invisible)
                        .child(state),
                )
                .child(
                    button(
                        SharedString::from(format!("archive:{}", th.id)),
                        format!("Archive {title}"),
                    )
                    .absolute()
                    .size(px(20.))
                    .justify_center()
                    .rounded(px(RADIUS_SM))
                    .text_color(t.text_muted)
                    .hover(move |s| s.bg(btn_hover).text_color(fg))
                    .invisible()
                    .group_hover(group, gpui::Styled::visible)
                    .child(icon("archive", 15.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.archive(&archive_id, cx);
                    })),
                ),
        )
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
                let target = this.menu_target(target.clone());
                this.open_context_menu(target, e, window, cx);
            }),
        )
        .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
            if !this.pick_row(&pick, e.modifiers(), cx) {
                this.open_thread(&id, cx);
            }
        }))
}
