//! The Vault pane's password fields and rows, by whether the vault is open, waiting for its
//! password, or has none, and the pane drawn from them.

// Rust guideline compliant 2026-10-09

use gpui::component::button::Button;
use gpui::component::button::ButtonVariants as _;
use gpui::component::Disableable as _;
use gpui::component::Sizable as _;
use gpui::component::spinner::Spinner;
use gpui::{AnyElement, Div, Focusable, Role, SharedString, Window, div, prelude::*, px};
use patchwork_client::local::core::vault::{Method, Status, status};
use zeroize::Zeroizing;
use super::vault::{ASK, CONFIRM, CURRENT, Change, Entry, FIELD_WIDTH, NEW, RECOVERY};
use super::rows::{activity_rows, protected_row, recovery_row, saved_rows, timeout_row};
use crate::settings::controls::{FieldKind, Section, alert, token_row, vault_notice};
use crate::settings::dialog::SettingsDialog;
use patchwork_design_icons::IconName;
use patchwork_design_theme::{fs_sm, theme};

use patchwork_design_icons::icon;

/// One of the pane's password fields.
pub(super) fn password_field(
    d: &mut SettingsDialog,
    key: &'static str,
    title: &'static str,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> Div {
    let shown = d.security.shown.contains(key);
    let kind = FieldKind {
        masked: !shown,
        live: true,
        mono: true,
        ..Default::default()
    };
    let value = match key {
        CURRENT => d.security.current.clone(),
        NEW => d.security.new.clone(),
        CONFIRM => d.security.confirm.clone(),
        RECOVERY => d.security.recovery.clone(),
        _ => d.security.ask.clone(),
    };
    // The one field without a label beside it says what it takes.
    let placeholder = if key == ASK { title } else { "" };
    d.field(
        key,
        title,
        &value,
        placeholder,
        kind,
        move |d, text, _| {
            let text = Zeroizing::new(text);
            match key {
                CURRENT => d.security.current = text,
                NEW => d.security.new = text,
                CONFIRM => d.security.confirm = text,
                RECOVERY => d.security.recovery = text,
                _ => d.security.ask = text,
            }
        },
        window,
        cx,
    )
    .w(px(FIELD_WIDTH))
    .flex_none()
    .pr(px(3.))
    .child(
        reveal_button(
            format!("{key}-show"),
            format!("Show {}", title.to_lowercase()),
            shown,
        )
        .on_click(cx.listener(move |d, _, _, cx| d.toggle_password(key, cx))),
    )
}

/// The eye at the end of a secret's field that shows or hides what is typed.
pub(super) fn reveal_button(id: String, label: String, shown: bool) -> Button {
    Button::new(SharedString::from(id))
        .ghost()
        .small()
        .icon(if shown {
            IconName::EyeSlash
        } else {
            IconName::Eye
        })
        .accessibility_label(label)
        .size(px(22.))
        .flex_none()
        .toggled(shown)
}

/// One password field in its row. Enter in it does `submit`.
fn password_row(
    d: &mut SettingsDialog,
    key: &'static str,
    title: &'static str,
    submit: Change,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let field = password_field(d, key, title, window, cx);
    d.on_submit(key, move |d, _, cx| d.change_password(submit, cx));
    token_row(title, &t).child(field).into_any_element()
}

/// A plain button of the pane.
fn plain(
    id: &'static str,
    label: &'static str,
    d: &SettingsDialog,
    cx: &mut Context<SettingsDialog>,
    click: impl Fn(&mut SettingsDialog, &mut Context<SettingsDialog>) + 'static,
) -> AnyElement {
    let busy = d.security.busy;
    Button::new(id)
        .small()
        .disabled(busy)
        .label(label)
        .when(!busy, |b| {
            b.on_click(cx.listener(move |d, _, _, cx| click(d, cx)))
        })
        .into_any_element()
}

/// The buttons under the fields, with how the last change went: `others`, then the one that
/// does `change`.
fn actions(
    d: &SettingsDialog,
    label: &'static str,
    change: Change,
    others: Vec<AnyElement>,
    cx: &mut Context<SettingsDialog>,
) -> impl IntoElement {
    let t = *theme(cx);
    let busy = d.security.busy;
    let working = busy.then(|| {
        div()
            .id("security-working")
            .role(Role::Status)
            .aria_label("Working")
            .child(Spinner::new().with_size(gpui::px(14.)).color(t.text_muted))
    });
    let done = d.security.done.map(|done| {
        div()
            .id("security-done")
            .role(Role::Status)
            .aria_label(done)
            .flex()
            .items_center()
            .gap(px(4.))
            .text_size(px(fs_sm()))
            .text_color(t.success)
            .child(icon(IconName::Check, 13.).text_color(t.success))
            .child(done)
    });
    div()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(8.))
        .px(px(16.))
        .py(px(10.))
        .children(working)
        .children(done)
        .children(others)
        .child(
            Button::new("security-save")
                .primary()
                .small()
                .disabled(busy)
                .label(label)
                .when(!busy, |b| {
                    b.on_click(cx.listener(move |d, _, _, cx| d.change_password(change, cx)))
                }),
        )
}

/// Switches a vault that waits between its password and its recovery key.
fn enter_with(d: &mut SettingsDialog, entry: Entry, cx: &mut Context<SettingsDialog>) {
    d.security.entry = entry;
    d.security.error = None;
    d.security.focused = false;
    d.wipe_fields(cx);
    cx.notify();
}

/// The fields and buttons of a vault that waits for its password or its recovery key.
fn waiting_rows(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> Vec<AnyElement> {
    let mut rows = Vec::new();
    let first = if d.security.entry == Entry::Password {
        rows.push(password_row(
            d,
            CURRENT,
            "Password",
            Change::Unlock,
            window,
            cx,
        ));
        CURRENT
    } else {
        for (key, title) in [
            (RECOVERY, "Recovery key"),
            (NEW, "New password"),
            (CONFIRM, "Confirm password"),
        ] {
            rows.push(password_row(d, key, title, Change::Recover, window, cx));
        }
        RECOVERY
    };
    if !d.security.focused {
        d.security.focused = true;
        if let Some(area) = d.field_area(first) {
            let focus = area.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
        }
    }
    let actions = if d.security.entry == Entry::Password {
        let other = plain("security-use-key", "Use recovery key", d, cx, |d, cx| {
            enter_with(d, Entry::RecoveryKey, cx);
        });
        actions(d, "Unlock", Change::Unlock, vec![other], cx).into_any_element()
    } else {
        let other = plain("security-use-password", "Use password", d, cx, |d, cx| {
            enter_with(d, Entry::Password, cx);
        });
        actions(d, "Reset password", Change::Recover, vec![other], cx).into_any_element()
    };
    rows.push(actions);
    vec![Section::new("Unlock").children(rows).into_any_element()]
}

/// The rows of an open vault: what protects it, its password and recovery key, the saved
/// secrets and what the chat apps did. A vault that has no key yet (`method` is None) has
/// only the fields that set its password.
fn open_rows(
    d: &mut SettingsDialog,
    method: Option<Method>,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> Vec<AnyElement> {
    let t = *theme(cx);
    let has_password = method == Some(Method::Password);
    if method.is_some() && d.security.read.is_none() {
        d.read_secrets(cx);
    }
    let mut rows: Vec<AnyElement> = method
        .map(|method| protected_row(d, method, cx))
        .into_iter()
        .collect();
    // The notice goes on top, so a vault nothing locks says so before anything else.
    let notice =
        vault_notice("security-vault", &t).map(|n| n.pt_0().pb(px(12.)).into_any_element());
    if has_password {
        rows.push(timeout_row(d, cx));
        rows.push(password_row(
            d,
            CURRENT,
            "Current password",
            Change::Set,
            window,
            cx,
        ));
    }
    rows.push(password_row(
        d,
        NEW,
        "New password",
        Change::Set,
        window,
        cx,
    ));
    rows.push(password_row(
        d,
        CONFIRM,
        "Confirm password",
        Change::Set,
        window,
        cx,
    ));
    let (label, others) = if has_password {
        let remove = plain("security-remove", "Remove password", d, cx, |d, cx| {
            d.change_password(Change::Remove, cx);
        });
        ("Change password", vec![remove])
    } else {
        ("Set password", Vec::new())
    };
    rows.push(actions(d, label, Change::Set, others, cx).into_any_element());
    if has_password || d.security.new_key.is_some() {
        rows.push(recovery_row(d, cx));
    }
    let saved = saved_rows(d, window, cx);
    let activity = activity_rows(d, cx);
    let mut page: Vec<AnyElement> = notice.into_iter().collect();
    page.push(Section::new("Lock").children(rows).into_any_element());
    if !saved.is_empty() {
        page.push(
            Section::new("Saved secrets")
                .children(saved)
                .into_any_element(),
        );
    }
    page.extend(activity.map(|list| {
        Section::new("Chat app activity")
            .bare()
            .child(list)
            .into_any_element()
    }));
    page
}

/// The Vault pane: the fields the vault's state asks for, and the buttons under them.
pub fn render(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let rows = match status() {
        Status::Closed => return div().into_any_element(),
        Status::Locked(why) => {
            return alert(
                "security-locked",
                format!("The vault is locked for this run: {why}"),
                t.danger,
            )
            .pt_0()
            .into_any_element();
        }
        Status::Waiting => waiting_rows(d, window, cx),
        Status::Unset => open_rows(d, None, window, cx),
        Status::Open(method) => open_rows(d, Some(method), window, cx),
    };
    div()
        .id("security")
        .role(Role::Group)
        .aria_label("Vault password")
        .flex()
        .flex_col()
        .children(
            d.security
                .error
                .clone()
                .map(|e| alert("security-error", e, t.danger).pt_0().pb(px(12.))),
        )
        .children(rows)
        .into_any_element()
}
