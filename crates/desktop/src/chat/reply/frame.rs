//! The top and bottom of a reply: who sent it, and its actions, files and usage.

// Rust guideline compliant 2026-10-09

use super::pieces::is_open;
use super::pieces::action_button;
use gpui::{AnyElement, ClipboardItem, Div, FontWeight, Role, SharedString, div, prelude::*, px};
use patchwork_server::threads::{Message, MessageStatus, Part, Role as MessageRole, Thread};
use crate::ui::theme::{FS_MD, FS_SM, FS_XS, RADIUS, Theme};
use crate::ui::{button, format_clock, full_elapsed, icon, now_ms};
use crate::workspace::{ReplyNotice, Workspace};
use crate::chat::timeline::{fold_head_of, fold_layout_of, steps_of};
use crate::chat::{approvals, checks, files, interrupted, review};

/// What sits under a reply's steps: approvals from before they had a place in the timeline, its
/// files, checks and review, a refusal or error, and its actions.
pub(super) fn foot(
    w: &Workspace,
    thread: &Thread,
    m: &Message,
    previous: Option<&Message>,
    actions: bool,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Vec<AnyElement> {
    let mut out = Vec::new();
    // Replies saved before approvals had their place in the timeline show them here.
    let loose: Vec<&str> = m
        .approvals
        .iter()
        .filter(|a| {
            !m.parts
                .iter()
                .any(|p| matches!(p, Part::Approval { id } if id == &a.id))
        })
        .map(|a| a.id.as_str())
        .collect();
    if !loose.is_empty() {
        out.push(approvals::approvals(w, m, &loose, t, cx).into_any_element());
    }
    out.extend(files::reply_files(w, m, t, cx).map(IntoElement::into_any_element));
    out.extend(checks::checks_card(w, m, t, cx).map(IntoElement::into_any_element));
    out.extend(review::review_card(w, m, t, cx).map(IntoElement::into_any_element));
    if let Some(refusal) = &m.refusal {
        out.push(
            div()
                .id(SharedString::from(format!("refusal-{}", m.id)))
                .role(Role::Alert)
                .rounded(px(RADIUS))
                .bg(t.warning_soft)
                .px(px(12.))
                .py(px(8.))
                .text_size(px(FS_SM))
                .flex()
                .flex_col()
                .gap(px(4.))
                .border_1()
                .border_color(t.warning)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.warning)
                        .child(icon("shield-warning", 15.))
                        .child(match &refusal.category {
                            Some(category) => {
                                format!("Blocked by a safety classifier · {category}")
                            }
                            None => "Blocked by a safety classifier".into(),
                        }),
                )
                .child(div().text_color(t.text).child(refusal.explanation.clone()))
                .into_any_element(),
        );
    }
    if let Some(error) = &m.error {
        out.push(
            div()
                .id(SharedString::from(format!("error-{}", m.id)))
                .role(Role::Alert)
                .rounded(px(RADIUS))
                .bg(t.danger_soft)
                .px(px(12.))
                .py(px(8.))
                .text_size(px(FS_SM))
                .text_color(t.danger)
                .child(error.clone())
                .into_any_element(),
        );
    }
    if let Some(row) = interrupted::resume_row(&w.server, thread, m, t, cx) {
        out.push(row.into_any_element());
    }
    if actions && !m.cut_off && !m.text.trim().is_empty() {
        let streaming = m.status == MessageStatus::Streaming;
        // The buttons find their message when clicked, so drawing does not copy its text.
        let retry = previous
            .filter(|p| p.role == MessageRole::User)
            .map(|p| p.id.clone());
        let (thread_id, message_id) = (thread.id.clone(), m.id.clone());
        let copy_id = m.id.clone();
        out.push(
            div()
                .flex()
                .items_center()
                .when(streaming, gpui::Styled::invisible)
                .child(
                    action_button(format!("copy-{}", m.id), "Copy", "copy", t).on_click(
                        cx.listener(move |w, _, _, cx| {
                            if let Some(m) = w.message(&copy_id) {
                                cx.write_to_clipboard(ClipboardItem::new_string(m.text.clone()));
                            }
                        }),
                    ),
                )
                .children(retry.map(|retry_id| {
                    action_button(format!("retry-{}", m.id), "Retry", "arrow-clockwise", t)
                        .on_click(cx.listener(move |w, _, _, cx| {
                            if let Some(p) = w.message(&retry_id) {
                                let (text, attachments) = (p.text.clone(), p.attachments.clone());
                                w.send(text, attachments, None, None, cx);
                            }
                        }))
                }))
                .child(
                    action_button(format!("fork-{}", m.id), "Fork", "git-fork", t).on_click(
                        cx.listener(move |w, _, _, cx| w.fork(&thread_id, &message_id, cx)),
                    ),
                )
                .child(
                    div()
                        .ml(px(6.))
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .text_size(px(FS_XS))
                        .text_color(t.text_muted)
                        .child(format_clock(m.created_at)),
                )
                .into_any_element(),
        );
    }
    out
}

/// What a running reply's head line says after "Working for 12s".
fn notice_text(notice: &ReplyNotice) -> String {
    match notice {
        ReplyNotice::Retrying(reason, Some(attempt), Some(max)) => {
            format!("· Retrying, {reason} ({attempt}/{max})")
        }
        ReplyNotice::Retrying(reason, _, _) => format!("· Retrying, {reason}"),
        ReplyNotice::Quiet(since) => format!(
            "· No output for {}",
            full_elapsed(now_ms().saturating_sub(*since))
        ),
    }
}

/// The reply's head line: "Working for 12s" while it streams, else its fold.
pub(super) fn head(w: &Workspace, m: &Message, t: &Theme, cx: &mut Context<Workspace>) -> Div {
    let streaming = m.status == MessageStatus::Streaming;
    let steps = steps_of(m);
    let layout = fold_layout_of(m, &steps, streaming);
    let open = is_open(w, m);
    let head_box = |row: AnyElement| {
        div()
            .mb(px(6.))
            .border_b_1()
            .border_color(t.border)
            .pt(px(4.))
            .pb(px(8.))
            .child(row)
    };
    if streaming {
        head_box(
            div()
                .id(SharedString::from(format!("status-{}", m.id)))
                .role(Role::Status)
                .h(px(24.))
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(FS_MD))
                .text_color(t.text_muted)
                .child(format!(
                    "Working for {}",
                    full_elapsed(now_ms().saturating_sub(m.created_at))
                ))
                .children(
                    w.notices.get(&m.id).map(|(_, notice)| {
                        div().text_color(t.text_subtle).child(notice_text(notice))
                    }),
                )
                .into_any_element(),
        )
    } else {
        let head = fold_head_of(m, layout.actions, open);
        let id = m.id.clone();
        let hover = t.text;
        head_box(
            button(
                SharedString::from(format!("fold-{}", m.id)),
                head.label.clone(),
            )
            .aria_expanded(open)
            .h(px(24.))
            .gap(px(6.))
            .text_size(px(FS_MD))
            .text_color(t.text_muted)
            .hover(move |s| s.text_color(hover))
            .child(head.label)
            .children(
                head.action
                    .map(|a| div().text_color(t.text_subtle).child(a)),
            )
            .when(layout.actions > 0, |b| {
                b.child(icon(if open { "caret-down" } else { "caret-right" }, 13.))
            })
            .on_click(cx.listener(move |w, _, _, cx| {
                if !w.open_folds.remove(&id) {
                    w.open_folds.insert(id.clone());
                }
                cx.notify();
            }))
            .into_any_element(),
        )
    }
}
