//! The chat's token use, as the line under the composer shows it.

// Rust guideline compliant 2026-10-09

use gpui::{Div, div, prelude::*, px};
use patchwork_model::chats::Chat;
use patchwork_design_text::format_tokens;
use patchwork_design_icons::{IconName, icon};
use patchwork_design_theme::{Theme, fs_xs};

/// The chat's token use: totals over its replies, context from the last.
#[must_use]
pub fn usage_line(chat: &Chat, warn_above: u64, t: &Theme) -> Div {
    let line = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_wrap()
        .justify_end()
        .gap_x(px(12.))
        .text_size(px(fs_xs()))
        .line_height(px(16.));
    let Some(last) = chat.messages.iter().rev().find_map(|m| m.usage.as_ref()) else {
        return line.invisible().child("Context");
    };
    let (mut input, mut read, mut write, mut output) = (0, 0, 0, 0);
    for usage in chat.messages.iter().filter_map(|m| m.usage.as_ref()) {
        input += usage.input;
        read += usage.cache_read;
        write += usage.cache_write;
        output += usage.output;
    }
    let context = chat.current_context();
    let large = context.is_some_and(|c| c > warn_above);
    line.text_color(if large { t.warning } else { t.text_muted })
        .children(context.map(|c| {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .when(large, |d| d.child(icon(IconName::Warning, 13.)))
                .child(if large {
                    format!("Context over {}", format_tokens(warn_above))
                } else {
                    format!("Context {}", format_tokens(c))
                })
        }))
        .child(format!("Cache read {} tokens", format_tokens(read)))
        .child(format!("Cache write {} tokens", format_tokens(write)))
        .child(format!("Input {} tokens", format_tokens(input)))
        .child(format!("Output {} tokens", format_tokens(output)))
        .children(last.first_token_ms.map(|ms| {
            if ms < 1000 {
                format!("First token {ms} ms")
            } else {
                format!("First token {:.1} s", ms as f64 / 1000.)
            }
        }))
        .children(last.output_per_second.map(|n| format!("{n} tokens/s")))
}
