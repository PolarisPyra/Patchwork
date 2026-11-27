//! The pairing code sent to a bot, and the pairings it has made.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, FontWeight, Role, SharedString, div, prelude::*, px};
use patchwork_backend::gateway::Platform;
use patchwork_platform::now_ms;

use crate::settings::SettingsDialog;
use crate::settings::controls::{list_box, token_row};
use patchwork_ui_theme::{font_mono, fs_sm, roomy, theme};
use patchwork_ui_components::{BtnKind, btn, icon_button};
use patchwork_ui_theme::icon;

/// The milliseconds in a minute, for the pairing code's time left.
const MINUTE_MS: u64 = 60_000;

/// The one-time code sent to a bot to make a pairing, the button that makes one, and who has
/// paired on `platform`.
pub(super) fn pairing_row(
    d: &SettingsDialog,
    platform: Platform,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let code = d
        .gateway
        .code
        .clone()
        .filter(|(_, until)| *until > now_ms());
    let shown = code.map(|(code, until)| {
        let minutes = until.saturating_sub(now_ms()).div_ceil(MINUTE_MS);
        div()
            .id("gw-code")
            .role(Role::Status)
            .aria_label(format!("Pairing code {code}, {minutes} min left"))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .font_family(font_mono())
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(code),
            )
            .child(
                div()
                    .text_size(px(fs_sm()))
                    .text_color(t.text_muted)
                    .child(format!("{minutes} min")),
            )
    });
    let pairing = token_row("Pairing code", &t).children(shown).child(
        btn("gw-new-code", "New code", BtnKind::Plain, false, &t)
            .child("New code")
            .on_click(cx.listener(|d, _, _, cx| {
                d.gateway.code = Some(d.core.new_pairing_code());
                cx.notify();
            })),
    );
    let names: Vec<&str> = d
        .gateway
        .pairings
        .iter()
        .filter(|u| u.platform == platform)
        .map(|u| u.name.as_str())
        .collect();
    let paired = (!names.is_empty()).then(|| {
        let text = format!("Paired with {}", names.join(", "));
        div()
            .id("gw-paired-now")
            .role(Role::Status)
            .aria_label(text.clone())
            .flex()
            .items_center()
            .gap(px(6.))
            .py(px(roomy(10.)))
            .text_color(t.success)
            .child(icon("check-circle", 15.).text_color(t.success))
            .child(text)
    });
    div()
        .flex()
        .flex_col()
        .child(pairing)
        .children(paired)
        .into_any_element()
}

/// The pairings, each with the button that unpairs it.
pub(super) fn paired(d: &SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    if d.gateway.pairings.is_empty() {
        return crate::settings::controls::empty("No pairings", &t).into_any_element();
    }
    let rows = d
        .gateway
        .pairings
        .iter()
        .map(|paired| {
            let (platform, id) = (paired.platform, paired.id.clone());
            let name = format!("{} on {}", paired.name, platform.label());
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .px(px(12.))
                .py(px(8.))
                .child(div().min_w_0().flex_1().truncate().child(name.clone()))
                .child(
                    icon_button(
                        SharedString::from(format!("gw-unpair-{}-{}", platform.id(), paired.id)),
                        format!("Remove {name}"),
                        "x",
                        14.,
                        cx,
                    )
                    .on_click(cx.listener(move |d, _, _, cx| {
                        d.remove_paired(platform, id.clone(), cx);
                    })),
                )
                .into_any_element()
        })
        .collect();
    div()
        .id("gw-paired")
        .role(Role::List)
        .aria_label("Pairings")
        .child(list_box(rows, &t))
        .into_any_element()
}
