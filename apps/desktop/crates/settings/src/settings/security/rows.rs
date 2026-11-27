//! The Vault pane's rows under the password fields: what protects the vault, when it locks,
//! its recovery key, the saved secrets, and what the chat apps had Patchwork do.

// Rust guideline compliant 2026-10-09

use patchwork_desktop_shared::platform_logo;
use gpui::{AnyElement, ClipboardItem, Focusable, Role, SharedString, Window, div, prelude::*, px};
use patchwork_backend::gateway::ChatAppCall;
use patchwork_backend::vault::{Method, VaultTimeout};

use crate::settings::security::{ASK, Change, FIELD_WIDTH, password_field, reveal_button};
use crate::settings::controls::{choices, list_box, select_box, token_row};
use crate::settings::dialog::SettingsDialog;
use patchwork_ui_theme::{IconName, Theme, font_mono, fs_sm, radius_sm, theme};
use patchwork_ui_components::{BtnKind, btn, icon_button, input_box, tooltip};
use patchwork_ui_markdown::format_relative;

/// When a vault with a password locks by itself.
const TIMEOUTS: [(VaultTimeout, &str); 5] = [
    (VaultTimeout::Minutes(60), "1 hour"),
    (VaultTimeout::Minutes(240), "4 hours"),
    (VaultTimeout::SystemLock, "On system lock"),
    (VaultTimeout::Restart, "On restart"),
    (VaultTimeout::Never, "Never"),
];

/// How many of the chat apps' calls the pane lists.
const CALLS_SHOWN: usize = 30;

/// What keeps the vault's key, how many secrets it holds, and the button that locks it.
pub(super) fn protected_row(
    d: &SettingsDialog,
    method: Method,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let busy = d.security.busy;
    let by = match method {
        Method::Keychain => "System keychain",
        Method::File => "Key file",
        Method::Password => "Your password",
    };
    let count = d.security.read.as_ref().map_or(0, |r| r.secrets.len());
    let line = match count {
        0 => by.to_string(),
        1 => format!("{by} · 1 secret"),
        n => format!("{by} · {n} secrets"),
    };
    let lock = (method == Method::Password).then(|| {
        btn("security-lock", "Lock now", BtnKind::Plain, busy, &t)
            .child("Lock now")
            .tooltip(tooltip("Locks the secrets and stops running agents"))
            .when(!busy, |b| {
                b.on_click(cx.listener(|d, _, _, cx| d.change_password(Change::Lock, cx)))
            })
    });
    token_row("Protected by", &t)
        .child(
            div()
                .id("security-protected")
                .role(Role::Status)
                .aria_label(line.clone())
                .text_size(px(fs_sm()))
                .text_color(t.text_muted)
                .child(line),
        )
        .children(lock)
        .into_any_element()
}

/// When a vault with a password locks by itself. An idle time saved by an earlier version
/// stays listed while it is the one picked.
pub(super) fn timeout_row(d: &SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    const TITLE: &str = "Vault timeout";
    let t = *theme(cx);
    let current = d.settings.vault.timeout;
    let mut options: Vec<(VaultTimeout, SharedString)> = TIMEOUTS
        .iter()
        // Only Linux has the listener for the screen and for sleep.
        .filter(|(at, _)| cfg!(target_os = "linux") || *at != VaultTimeout::SystemLock)
        .map(|&(at, label)| (at, label.into()))
        .collect();
    if let VaultTimeout::Minutes(mins) = current
        && !options.iter().any(|(at, _)| *at == current)
    {
        options.insert(0, (current, format!("{mins} min").into()));
    }
    let label = options
        .iter()
        .find(|(at, _)| *at == current)
        .map_or_else(SharedString::default, |(_, label)| label.clone());
    let open = d.menu_open("security-timeout");
    token_row(TITLE, &t)
        .child(
            select_box("security-timeout", TITLE, label, open, &t)
                .w(px(FIELD_WIDTH))
                .on_click(choices(
                    "security-timeout",
                    TITLE,
                    options,
                    current,
                    |d, timeout, cx| d.update(|s| s.vault.timeout = timeout, cx),
                    cx,
                )),
        )
        .into_any_element()
}

/// The recovery key: the one just made, shown until it is put away, or the button that makes
/// a new one with the current password.
pub(super) fn recovery_row(d: &SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let busy = d.security.busy;
    let Some(key) = d.security.new_key.as_ref() else {
        return token_row("Recovery key", &t)
            .child(
                btn(
                    "security-new-key",
                    "New recovery key",
                    BtnKind::Plain,
                    busy,
                    &t,
                )
                .child("New recovery key")
                .tooltip(tooltip("Needs the current password"))
                .when(!busy, |b| {
                    b.on_click(cx.listener(|d, _, _, cx| {
                        d.change_password(Change::NewKey, cx);
                    }))
                }),
            )
            .into_any_element();
    };
    let copied = SharedString::from(key.as_str().to_string());
    token_row("Recovery key", &t)
        .flex_wrap()
        .child(
            icon_button(
                "security-copy-key",
                "Copy recovery key",
                IconName::Copy,
                14.,
                cx,
            )
            .tooltip(tooltip("Copy"))
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copied.to_string()));
            })),
        )
        .child(
            btn(
                "security-download-key",
                "Download recovery key",
                BtnKind::Plain,
                false,
                &t,
            )
            .child("Download")
            .on_click(cx.listener(|d, _, _, cx| d.download_key(cx))),
        )
        .child(
            btn("security-key-done", "Done", BtnKind::Plain, false, &t)
                .child("Done")
                .on_click(cx.listener(|d, _, _, cx| {
                    d.security.new_key = None;
                    cx.notify();
                })),
        )
        .child(value_box("Recovery key", key, &t))
        .into_any_element()
}

/// The recovery key just made, as the pane shows it.
fn value_box(label: &str, value: &str, t: &Theme) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("security-value-{label}")))
        .role(Role::Status)
        .aria_label(format!("{label}: {value}"))
        .w_full()
        .min_w_0()
        .overflow_hidden()
        .rounded(px(radius_sm()))
        .border_1()
        .border_color(t.border_strong)
        .bg(t.bg_editor)
        .px(px(10.))
        .py(px(6.))
        .font_family(font_mono())
        .text_size(px(fs_sm()))
        .text_color(t.text_muted)
        .child(value.to_string())
}

/// The saved secrets, each in a field with the button that shows it, and the old copies under
/// them. A vault with a password asks for it in that field first.
pub(super) fn saved_rows(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> Vec<AnyElement> {
    let t = *theme(cx);
    let (secrets, old) = d
        .security
        .read
        .as_ref()
        .map_or_else(Default::default, |r| (r.secrets.clone(), r.old));
    let mut rows = Vec::new();
    for secret in &secrets {
        let row = token_row(secret.label.clone(), &t);
        let field = if d.security.asking.as_deref() == Some(secret.name.as_str()) {
            ask_field(d, window, cx)
        } else {
            secret_field(d, &secret.name, &secret.label, cx)
        };
        rows.push(row.child(field).into_any_element());
    }
    if old > 0 {
        let busy = d.security.busy;
        rows.push(
            token_row("Old copies", &t)
                .child(
                    btn(
                        "security-clean",
                        "Remove old copies",
                        BtnKind::Plain,
                        busy,
                        &t,
                    )
                    .child("Remove")
                    .when(!busy, |b| {
                        b.on_click(cx.listener(|d, _, _, cx| d.change_password(Change::Clean, cx)))
                    }),
                )
                .into_any_element(),
        );
    }
    rows
}

/// A saved secret in a field like the password ones: dots until its button shows the value.
fn secret_field(
    d: &SettingsDialog,
    name: &str,
    label: &str,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let value = d.security.revealed.get(name);
    let shown = value.is_some();
    let spoken = match value {
        Some(value) => format!("{label}: {}", value.as_str()),
        None => format!("{label}, hidden"),
    };
    let text = match value {
        Some(value) => value.as_str().to_string(),
        None => "\u{2022}".repeat(16),
    };
    let name = name.to_string();
    input_box(false, &t)
        .pr(px(3.))
        .w(px(FIELD_WIDTH))
        .flex_none()
        // A value is longer than the field, so it wraps inside it and the field grows down.
        .when(shown, |b| b.h_auto().items_start().py(px(3.)))
        .child(
            div()
                .id(SharedString::from(format!("security-value-{name}")))
                .role(Role::Status)
                .aria_label(spoken)
                .flex_1()
                .min_w_0()
                .font_family(font_mono())
                .text_color(t.text_muted)
                .child(text),
        )
        // A value can be copied only while it is shown, so the password is asked for first.
        .when(shown, |b| {
            let name = name.clone();
            b.child(
                icon_button(
                    SharedString::from(format!("security-copy-{name}")),
                    format!("Copy {label}"),
                    IconName::Copy,
                    14.,
                    cx,
                )
                .size(px(22.))
                .flex_none()
                .tooltip(tooltip("Copy"))
                .on_click(cx.listener(move |d, _, _, cx| {
                    if let Some(value) = d.security.revealed.get(&name) {
                        cx.write_to_clipboard(ClipboardItem::new_string(
                            value.as_str().to_string(),
                        ));
                    }
                })),
            )
        })
        .child(
            reveal_button(
                format!("security-show-{name}"),
                format!("Show {label}"),
                shown,
                cx,
            )
            .on_click(cx.listener(move |d, _, _, cx| d.toggle_secret(name.clone(), cx))),
        )
        .into_any_element()
}

/// The field that asks for the password in place of the secret it shows. Enter in it shows the
/// secret.
fn ask_field(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let field = password_field(d, ASK, "Your password", window, cx);
    d.on_submit(ASK, |d, _, cx| d.confirm_secret(cx));
    if !d.security.focused {
        d.security.focused = true;
        if let Some(area) = d.field_area(ASK) {
            let focus = area.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
        }
    }
    field.into_any_element()
}

/// What the chat apps had Patchwork do, the latest first. None when they did nothing.
pub(super) fn activity_rows(
    d: &SettingsDialog,
    cx: &mut Context<SettingsDialog>,
) -> Option<AnyElement> {
    let t = *theme(cx);
    let calls = d
        .security
        .read
        .as_ref()
        .map_or(&[][..], |r| r.activity.as_slice());
    if calls.is_empty() {
        return None;
    }
    let rows = calls
        .iter()
        .take(CALLS_SHOWN)
        .enumerate()
        .map(|(i, call)| call_row(i, call, &t))
        .collect();
    Some(
        div()
            .id("security-activity")
            .role(Role::List)
            .aria_label("Chat app activity")
            .child(list_box(rows, &t))
            .into_any_element(),
    )
}

/// One thing a chat app had Patchwork do: what, for whom, when, and whether it failed.
fn call_row(i: usize, call: &ChatAppCall, t: &Theme) -> AnyElement {
    let when = format_relative(call.at);
    let app = call.platform.map_or("a chat app", |p| p.label());
    let who = if call.who.is_empty() {
        app.to_string()
    } else {
        call.who.clone()
    };
    let outcome = if call.ok { "" } else { ", failed" };
    let spoken = format!(
        "{} ({}) from {who} on {app}, {when}{outcome}",
        call.capability, call.permission
    );
    div()
        .id(("security-call", i))
        .role(Role::ListItem)
        .aria_label(spoken)
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(12.))
        .py(px(6.))
        .text_size(px(fs_sm()))
        .children(call.platform.map(|p| platform_logo(p, 12.)))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .truncate()
                .font_family(font_mono())
                .child(call.capability.clone()),
        )
        .children((!call.ok).then(|| div().text_color(t.danger).child("Failed")))
        .child(div().text_color(t.text_muted).child(who))
        .child(div().text_color(t.text_subtle).child(when))
        .into_any_element()
}
