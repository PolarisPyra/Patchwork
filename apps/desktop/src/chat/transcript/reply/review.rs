//! What the chat's reviewer made of the files a reply changed, under its checks. Shown only for a
//! reply a reviewer looked at. Concerns show open, an approval closed.

// Rust guideline compliant 2026-10-09

use patchwork_desktop_shared::agent_label;
use gpui::{AnyElement, Div, Hsla, Role, SharedString, div, prelude::*, px};
use patchwork_backend::chats::{Message, ReviewStatus, TurnReview};

use patchwork_ui_markdown::markdown::markdown;
use patchwork_ui_theme::{IconName, fs_sm, fs_xs, radius, Theme};
use patchwork_ui_components::{BtnKind, btn, button, spinner, tooltip};
use patchwork_ui_theme::icon;
use crate::workspace::Workspace;

fn status_icon(status: ReviewStatus, t: &Theme) -> AnyElement {
    let (name, color) = match status {
        ReviewStatus::Running => return spinner(14., t.text_muted).into_any_element(),
        ReviewStatus::Approved => (IconName::CheckCircle, t.success),
        ReviewStatus::Concerns => (IconName::Warning, t.warning),
        ReviewStatus::Failed => (IconName::XCircle, t.danger),
    };
    icon(name, 14.)
        .flex_none()
        .text_color(color)
        .into_any_element()
}

/// The row's words, and how they read.
fn say(review: &TurnReview, t: &Theme) -> (String, Hsla) {
    let who = match &review.model {
        Some(model) => format!("{} ({model})", agent_label(review.agent)),
        None => agent_label(review.agent).to_string(),
    };
    match review.status {
        ReviewStatus::Running => (format!("{who} is reviewing the changes…"), t.text_muted),
        ReviewStatus::Approved => (format!("{who} approved the changes"), t.success),
        ReviewStatus::Concerns => (format!("{who} raised concerns"), t.warning),
        ReviewStatus::Failed => (format!("{who} could not review the changes"), t.danger),
    }
}

pub fn review_card(
    w: &Workspace,
    m: &Message,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Option<Div> {
    let review = m.review.as_ref()?;
    let id = m.id.clone();
    // The set holds the cards turned from how they first show.
    let open = w.transcript.open_reviews.contains(&id) != (review.status == ReviewStatus::Concerns);
    let has_text = !review.text.is_empty() && review.status != ReviewStatus::Failed;
    let (title, tone) = say(review, t);
    let shown = if open { "Hide review" } else { "Show review" };

    let summary = button(SharedString::from(format!("review-{id}")), shown)
        .aria_expanded(open && has_text)
        .min_w_0()
        .gap_x(px(8.))
        .tooltip(tooltip(shown))
        .child(status_icon(review.status, t))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_color(tone)
                .child(title.clone()),
        )
        .when(has_text, |b| {
            b.child(
                icon(
                    if open {
                        IconName::CaretDown
                    } else {
                        IconName::CaretRight
                    },
                    12.,
                )
                .flex_none()
                .text_color(t.text_subtle),
            )
        })
        .on_click(cx.listener({
            let id = id.clone();
            move |w, _, _, cx| {
                if !w.transcript.open_reviews.remove(&id) {
                    w.transcript.open_reviews.insert(id.clone());
                }
                cx.notify();
            }
        }));

    let child = review.chat_id.clone();
    let open_chat = btn(
        SharedString::from(format!("review-open-{id}")),
        "Open the reviewer's chat",
        BtnKind::Plain,
        false,
        t,
    )
    .tooltip(tooltip("Open the reviewer's chat"))
    .child("Open")
    .on_click(cx.listener(move |w, _, _, cx| w.open_chat(&child, cx)));

    let details = (open && has_text).then(|| {
        div().pt(px(2.)).text_size(px(fs_sm())).child(markdown(
            &review.text,
            format!("review-text-{id}"),
            cx,
        ))
    });

    let error = (review.status == ReviewStatus::Failed && !review.text.is_empty()).then(|| {
        div()
            .id(SharedString::from(format!("review-error-{id}")))
            .role(Role::Alert)
            .text_size(px(fs_xs()))
            .text_color(t.danger)
            .child(review.text.clone())
    });

    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .rounded(px(radius()))
            .border_1()
            .border_color(t.border)
            .bg(t.bg_panel)
            .px(px(12.))
            .py(px(8.))
            .text_size(px(fs_sm()))
            .child(
                div()
                    .id(SharedString::from(format!("review-row-{id}")))
                    .role(Role::Status)
                    .aria_label(title)
                    .flex()
                    .items_center()
                    .gap_x(px(12.))
                    .child(summary)
                    .child(div().flex_1())
                    .child(open_chat),
            )
            .children(details)
            .children(error),
    )
}
