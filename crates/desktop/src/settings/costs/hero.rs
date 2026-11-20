//! The overview's left side: the range's cost or tokens in full, then each provider's share.

// Rust guideline compliant 2026-10-09

use gpui::{Div, ElementId, FontWeight, Role, SharedString, div, prelude::*, px, relative};
use patchwork_core::costs::CostReport;

use super::chart::swatch;
use super::rows::{amount, dollars, grouped, series, value};
use crate::ui::theme::{Theme, fs_sm, fs_xs, fs_xxl};
use crate::ui::{agent_label, format_tokens};

pub(super) fn hero(report: &CostReport, by_cost: bool, t: &Theme) -> Div {
    let range = &report.in_range;
    let (label, figure, under) = if by_cost {
        (
            "Estimated cost",
            dollars(range.cost()),
            format!("{} tokens", format_tokens(range.tokens())),
        )
    } else {
        (
            "Tokens",
            grouped(range.tokens()),
            format!("{} read from cache", format_tokens(range.cache_read)),
        )
    };
    let whole = value(range, by_cost);
    let providers = report.providers.iter().enumerate().map(|(ix, p)| {
        let part = value(&p.in_range, by_cost);
        let share = if whole > 0. { part / whole } else { 0. };
        let name = agent_label(p.agent);
        let color = series(ix, t);
        div()
            .id(ElementId::Integer(ix as u64))
            .role(Role::ListItem)
            .aria_label(SharedString::from(format!(
                "{name}: {}, {:.0}%",
                amount(part, by_cost),
                share * 100.
            )))
            .flex()
            .flex_col()
            .gap(px(5.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(fs_sm()))
                    .child(swatch(name, color).flex_1())
                    .child(
                        div()
                            .flex_none()
                            .text_color(t.text_muted)
                            .child(amount(part, by_cost)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(34.))
                            .text_right()
                            .text_color(t.text_subtle)
                            .child(format!("{:.0}%", share * 100.)),
                    ),
            )
            .child(
                div()
                    .h(px(3.))
                    .w_full()
                    .rounded_full()
                    .bg(t.bg_hover)
                    .child(
                        div()
                            .h_full()
                            .rounded_full()
                            .bg(color)
                            .w(relative(share.clamp(0., 1.) as f32)),
                    ),
            )
    });
    div()
        .flex()
        .flex_col()
        .gap(px(14.))
        .w(px(220.))
        .flex_none()
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(fs_xs()))
                        .text_color(t.text_muted)
                        .child(label),
                )
                .child(
                    div()
                        .text_size(px(fs_xxl()))
                        .font_weight(FontWeight::SEMIBOLD)
                        .truncate()
                        .child(figure),
                )
                .child(
                    div()
                        .text_size(px(fs_sm()))
                        .text_color(t.text_subtle)
                        .child(under),
                ),
        )
        .child(
            div()
                .id("costs-shares")
                .role(Role::List)
                .aria_label("Share by provider")
                .flex()
                .flex_col()
                .gap(px(10.))
                .children(providers),
        )
}
