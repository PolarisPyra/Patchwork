//! A chat app's own page: how to set it up, its switch, status and credentials.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, Role, SharedString, Window, div, prelude::*, px};
use patchwork_core::gateway::{Platform, Status};

use super::paired::pairing_row;
use super::setup::setup;
use crate::settings::SettingsDialog;
use crate::settings::controls::{FieldKind, alert, detail_header, switch, token_row, vault_notice};
use crate::ui::theme::{fs_sm, theme};
use crate::ui::{BtnKind, btn, icon, platform_logo};

/// The width of a token or address field.
const FIELD_WIDTH: f32 = 260.;

/// What a platform's status says, and whether it is an error.
fn status_line(status: &Status) -> (String, bool) {
    match status {
        Status::Off => ("Off".to_string(), false),
        Status::Locked => ("Unlock the vault to connect".to_string(), false),
        Status::Connecting => ("Connecting…".to_string(), false),
        Status::Connected(name) => (format!("Connected as {name}"), false),
        Status::Error(error) => (error.clone(), true),
    }
}

/// One platform: its switch and status, its tokens, and its server and account.
fn platform(
    d: &mut SettingsDialog,
    platform: Platform,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> Vec<AnyElement> {
    let t = *theme(cx);
    let settings = d.settings.gateway.platform(platform);
    let id = platform.id();
    let label = platform.label();
    let enabled = settings.enabled;
    let status = if enabled {
        d.gateway
            .statuses
            .get(&platform)
            .cloned()
            .unwrap_or_default()
    } else {
        Status::Off
    };
    let (line, failed) = status_line(&status);
    let line = div()
        .id(SharedString::from(format!("gw-status-{id}")))
        .role(if failed { Role::Alert } else { Role::Status })
        .aria_label(format!("{label}: {line}"))
        .min_w_0()
        .max_w(px(320.))
        .truncate()
        .text_size(px(fs_sm()))
        .text_color(if failed { t.danger } else { t.text_muted })
        .child(line);
    let switch_label = format!("Enable {label}");
    let mut rows = vec![
        token_row("Enable", None, &t)
            .child(line)
            .child(
                switch(format!("gw-{id}-enabled"), switch_label, enabled, &t).on_click(
                    cx.listener(move |d, _, _, cx| {
                        d.set_platform(platform, |s| s.enabled = !enabled, cx);
                    }),
                ),
            )
            .into_any_element(),
    ];
    let kind = FieldKind {
        masked: true,
        mono: true,
        ..Default::default()
    };
    for credential in platform.credentials() {
        let key = credential.key;
        let saved = d
            .gateway
            .saved
            .as_ref()
            .is_some_and(|s| s.contains(&(platform, key)));
        let name = format!("{label} {}", credential.label.to_lowercase());
        let field = d
            .field(
                format!("gw-{id}-{key}"),
                name.clone(),
                "",
                if saved {
                    "Saved"
                } else {
                    credential.placeholder
                },
                kind,
                move |d, text, cx| {
                    if !text.trim().is_empty() {
                        d.save_credential(platform, key, text, cx);
                    }
                },
                window,
                cx,
            )
            .w_full();
        // The field takes its own line under the label, wide enough for a whole token.
        rows.push(
            token_row(credential.label, None, &t)
                .flex_wrap()
                .child(field)
                .child(credential_actions(d, platform, key, &name, saved, cx))
                .into_any_element(),
        );
    }
    for field in platform.settings() {
        let key = field.key;
        let input = d
            .field(
                format!("gw-{id}-{key}"),
                format!("{label} {}", field.label.to_lowercase()),
                settings.value(key),
                field.placeholder,
                FieldKind {
                    mono: true,
                    ..Default::default()
                },
                move |d, text, cx| {
                    d.set_platform(
                        platform,
                        |s| {
                            s.values.insert(key.to_string(), text.trim().to_string());
                        },
                        cx,
                    );
                },
                window,
                cx,
            )
            .w(px(FIELD_WIDTH))
            .flex_none();
        rows.push(
            token_row(field.label, None, &t)
                .child(input)
                .into_any_element(),
        );
    }
    rows
}

/// The buttons under a credential's field that clear it and save it, and whether it saved.
fn credential_actions(
    d: &SettingsDialog,
    platform: Platform,
    key: &'static str,
    name: &str,
    saved: bool,
    cx: &mut Context<SettingsDialog>,
) -> impl IntoElement {
    let t = *theme(cx);
    let id = platform.id();
    let field = SharedString::from(format!("gw-{id}-{key}"));
    let done = d.gateway.just_saved.contains(&(platform, key)).then(|| {
        div()
            .id(SharedString::from(format!("gw-{id}-{key}-saved")))
            .role(Role::Status)
            .aria_label(format!("{name} saved"))
            .flex()
            .items_center()
            .gap(px(4.))
            .text_size(px(fs_sm()))
            .text_color(t.success)
            .child(icon("check", 13.).text_color(t.success))
            .child("Saved")
    });
    let cleared = field.clone();
    div()
        .w_full()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(8.))
        .pt(px(6.))
        .children(done)
        .child(
            btn(
                SharedString::from(format!("gw-{id}-{key}-clear")),
                format!("Clear {name}"),
                BtnKind::Plain,
                false,
                &t,
            )
            .child("Clear")
            .on_click(cx.listener(move |d, _, _, cx| {
                if let Some(area) = d.field_area(&cleared) {
                    area.update(cx, |a, cx| a.set_text("", cx));
                }
                d.gateway.just_saved.remove(&(platform, key));
                if saved {
                    d.save_credential(platform, key, String::new(), cx);
                }
                cx.notify();
            })),
        )
        .child(
            btn(
                SharedString::from(format!("gw-{id}-{key}-save")),
                format!("Save {name}"),
                BtnKind::Primary,
                false,
                &t,
            )
            .child("Save")
            .on_click(cx.listener(move |d, _, _, cx| d.commit_field(&field, cx))),
        )
}

/// A chat app's own page: how to set it up, its switch and fields, and the pairing code.
pub(super) fn page(
    d: &mut SettingsDialog,
    each: Platform,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let label = each.label();
    let back = |d: &mut SettingsDialog, cx: &mut Context<SettingsDialog>| {
        d.close_gateway(cx);
    };
    let header = detail_header("gw", platform_logo(each, 24.), label, back, cx).pb(px(8.));
    let steps = setup(each, cx);
    let rows = platform(d, each, window, cx);
    let pairing = pairing_row(d, each, cx);
    div()
        .id("gw-platform")
        .role(Role::Group)
        .aria_label(label)
        .flex()
        .flex_col()
        .child(header)
        .child(steps)
        .children(rows)
        .child(pairing)
        .children(vault_notice("gw-vault", &t))
        .children(
            d.gateway
                .error
                .clone()
                .map(|e| alert("gw-error", e, t.danger)),
        )
        .into_any_element()
}
