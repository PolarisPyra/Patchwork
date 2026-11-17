//! The Security pane: what protects the saved secrets, the vault's password (set, changed,
//! removed and entered here), the secrets that are saved, and the copies earlier versions left.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{AnyElement, Focusable, Role, SharedString, Toggled, Window, div, prelude::*, px};
use patchwork_core::vault::{Method, Saved, Status, status};

use super::controls::{FieldKind, alert, section_heading, token_row, vault_notice};
use super::dialog::{SettingsDialog, SettingsEvent};
use crate::ui::theme::{Theme, font_mono, fs_sm, radius_sm, theme};
use crate::ui::{BtnKind, btn, icon, icon_button, spinner};

/// The width of a password field.
const FIELD_WIDTH: f32 = 260.;

/// What each password field of the pane is named. The names start alike, so they go together.
const FIELDS: &str = "security-";
const CURRENT: &str = "security-current";
const NEW: &str = "security-new";
const CONFIRM: &str = "security-confirm";

/// What the pane does with the passwords typed in it.
#[derive(Clone, Copy)]
enum Change {
    Unlock,
    /// Sets the first password, or changes it.
    Set,
    Remove,
    Lock,
    /// Removes the copies earlier versions left outside the vault.
    Clean,
}

/// The pane's state: what is typed, and how the last change went.
#[derive(Default)]
pub struct SecurityState {
    current: String,
    new: String,
    confirm: String,
    /// Whether the fields show what is typed.
    shown: bool,
    busy: bool,
    /// Whether the password field of a vault that waits has been given the focus.
    focused: bool,
    error: Option<String>,
    /// What the last change did, once it has.
    done: Option<&'static str>,
    /// The saved secrets, once they have been read.
    secrets: Option<Vec<Saved>>,
    /// How many copies earlier versions left outside the vault.
    old: usize,
    /// The values the pane shows, by the secret's name.
    revealed: HashMap<String, String>,
}

impl SettingsDialog {
    /// Shows or hides what is typed in the pane's fields.
    fn toggle_passwords(&mut self, cx: &mut Context<Self>) {
        self.security.shown = !self.security.shown;
        let masked = !self.security.shown;
        for key in [CURRENT, NEW, CONFIRM] {
            if let Some(area) = self.field_area(key) {
                area.update(cx, |a, cx| {
                    a.masked = masked;
                    cx.notify();
                });
            }
        }
        cx.notify();
    }

    /// Reads which secrets are saved, and how many old copies are left, off the main thread.
    fn read_secrets(&mut self, cx: &mut Context<Self>) {
        // Empty until they are read, so they are asked for once.
        self.security.secrets = Some(Vec::new());
        let core = Arc::clone(&self.core);
        self.run(
            move || (core.vault_secrets(), core.old_secret_copies()),
            |d, (secrets, old), _| {
                d.security.secrets = Some(secrets);
                d.security.old = old;
            },
            cx,
        );
    }

    /// Shows the saved secret `name`, or hides it when it shows.
    fn toggle_secret(&mut self, name: String, cx: &mut Context<Self>) {
        if self.security.revealed.remove(&name).is_some() {
            return cx.notify();
        }
        let core = Arc::clone(&self.core);
        let read = name.clone();
        self.run(
            move || core.reveal_secret(&read),
            move |d, value, _| match value {
                Some(value) => {
                    d.security.revealed.insert(name, value);
                }
                None => d.security.error = Some("That secret could not be read".into()),
            },
            cx,
        );
    }

    /// Opens or locks the vault, sets, changes or removes its password with what is typed, or
    /// removes the old copies.
    fn change_password(&mut self, change: Change, cx: &mut Context<Self>) {
        if self.security.busy {
            return;
        }
        self.security.done = None;
        if matches!(change, Change::Set) && self.security.new != self.security.confirm {
            self.security.error = Some("The two new passwords are not the same".into());
            return cx.notify();
        }
        self.security.busy = true;
        self.security.error = None;
        let core = Arc::clone(&self.core);
        let current = std::mem::take(&mut self.security.current);
        let new = std::mem::take(&mut self.security.new);
        self.security.confirm.clear();
        self.run(
            move || match change {
                Change::Unlock => core.unlock_vault(&current).map(|()| "Unlocked"),
                Change::Set => core
                    .set_vault_password(&current, &new)
                    .map(|()| "Password saved"),
                Change::Remove => core
                    .remove_vault_password(&current)
                    .map(|()| "Password removed"),
                Change::Lock => core.lock_vault().map(|()| "Locked"),
                Change::Clean => core
                    .remove_old_secret_copies()
                    .map(|_| "Old copies removed"),
            },
            |d, out, cx| {
                d.security.busy = false;
                // The fields start empty after a change, and after a wrong password.
                d.drop_fields(FIELDS);
                // Nothing shown outlives a change, and the list is read again.
                d.security.revealed.clear();
                d.security.secrets = None;
                d.security.focused = false;
                match out {
                    Ok(done) => {
                        d.security.done = Some(done);
                        // Opening the vault fills in the terminal variables.
                        d.settings = d.core.settings();
                        cx.emit(SettingsEvent::Changed);
                    }
                    Err(e) => d.security.error = Some(e.to_string()),
                }
            },
            cx,
        );
        cx.notify();
    }
}

/// One password field in its row. Enter in it does `submit`.
fn password_row(
    d: &mut SettingsDialog,
    key: &'static str,
    title: &'static str,
    desc: Option<&str>,
    submit: Change,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let kind = FieldKind {
        masked: !d.security.shown,
        live: true,
        mono: true,
        ..Default::default()
    };
    let value = match key {
        CURRENT => d.security.current.clone(),
        NEW => d.security.new.clone(),
        _ => d.security.confirm.clone(),
    };
    let field = d
        .field(
            key,
            title,
            &value,
            "",
            kind,
            move |d, text, _| match key {
                CURRENT => d.security.current = text,
                NEW => d.security.new = text,
                _ => d.security.confirm = text,
            },
            window,
            cx,
        )
        .w(px(FIELD_WIDTH))
        .flex_none();
    d.on_submit(key, move |d, _, cx| d.change_password(submit, cx));
    token_row(title, desc, &t).child(field).into_any_element()
}

/// The buttons under the fields, with how the last change went.
fn actions(
    d: &SettingsDialog,
    has_password: bool,
    waiting: bool,
    cx: &mut Context<SettingsDialog>,
) -> impl IntoElement {
    let t = *theme(cx);
    let busy = d.security.busy;
    let shown = d.security.shown;
    let working = busy.then(|| {
        div()
            .id("security-working")
            .role(Role::Status)
            .aria_label("Working")
            .child(spinner(14., t.text_muted))
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
            .child(icon("check", 13.).text_color(t.success))
            .child(done)
    });
    let (label, change) = match (waiting, has_password) {
        (true, _) => ("Unlock", Change::Unlock),
        (false, true) => ("Change password", Change::Set),
        (false, false) => ("Set password", Change::Set),
    };
    let remove = (has_password && !waiting).then(|| {
        btn(
            "security-remove",
            "Remove password",
            BtnKind::Plain,
            busy,
            &t,
        )
        .child("Remove password")
        .when(!busy, |b| {
            b.on_click(cx.listener(|d, _, _, cx| d.change_password(Change::Remove, cx)))
        })
    });
    div()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(8.))
        .pt(px(10.))
        .children(working)
        .children(done)
        .child(
            icon_button(
                "security-show",
                "Show passwords",
                if shown { "eye-slash" } else { "eye" },
                14.,
                cx,
            )
            .aria_toggled(if shown { Toggled::True } else { Toggled::False })
            .tooltip(crate::ui::tooltip(if shown {
                "Hide passwords"
            } else {
                "Show passwords"
            }))
            .on_click(cx.listener(|d, _, _, cx| d.toggle_passwords(cx))),
        )
        .children(remove)
        .child(
            btn("security-save", label, BtnKind::Primary, busy, &t)
                .child(label)
                .when(!busy, |b| {
                    b.on_click(cx.listener(move |d, _, _, cx| d.change_password(change, cx)))
                }),
        )
}

/// The Security pane: the fields the vault's state asks for, and the buttons under them.
pub(super) fn render(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let (has_password, waiting) = match status() {
        Status::Closed => return div().into_any_element(),
        Status::Locked(why) => {
            return alert(
                "security-locked",
                format!("The vault is locked for this run: {why}"),
                t.danger,
            )
            .into_any_element();
        }
        Status::Waiting => (true, true),
        Status::Open(method) => (method == Method::Password, false),
    };
    if !waiting && d.security.secrets.is_none() {
        d.read_secrets(cx);
    }
    let mut rows = Vec::new();
    if let Status::Open(method) = status() {
        rows.push(protected_row(d, method, cx));
    }
    if waiting {
        rows.push(password_row(
            d,
            CURRENT,
            "Password",
            Some("Unlocks the saved keys, tokens and terminal variables"),
            Change::Unlock,
            window,
            cx,
        ));
        if !d.security.focused {
            d.security.focused = true;
            if let Some(area) = d.field_area(CURRENT) {
                let focus = area.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
            }
        }
    } else {
        if has_password {
            rows.push(password_row(
                d,
                CURRENT,
                "Current password",
                None,
                Change::Set,
                window,
                cx,
            ));
        }
        rows.push(password_row(
            d,
            NEW,
            "New password",
            (!has_password)
                .then_some("Asked for each time Patchwork starts. It cannot be recovered."),
            Change::Set,
            window,
            cx,
        ));
        rows.push(password_row(
            d,
            CONFIRM,
            "Confirm password",
            None,
            Change::Set,
            window,
            cx,
        ));
    }
    let actions = actions(d, has_password, waiting, cx).into_any_element();
    let saved = if waiting {
        Vec::new()
    } else {
        saved_rows(d, cx)
    };
    div()
        .id("security")
        .role(Role::Group)
        .aria_label("Vault password")
        .flex()
        .flex_col()
        .children(rows)
        .child(actions)
        .children(saved)
        .children(
            (!waiting)
                .then(|| vault_notice("security-vault", &t))
                .flatten(),
        )
        .children(
            d.security
                .error
                .clone()
                .map(|e| alert("security-error", e, t.danger)),
        )
        .into_any_element()
}

/// What keeps the vault's key, how many secrets it holds, and the button that locks it.
fn protected_row(
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
    let count = d.security.secrets.as_ref().map_or(0, Vec::len);
    let line = match count {
        0 => by.to_string(),
        1 => format!("{by} · 1 secret"),
        n => format!("{by} · {n} secrets"),
    };
    let lock = (method == Method::Password).then(|| {
        btn("security-lock", "Lock now", BtnKind::Plain, busy, &t)
            .child("Lock now")
            .when(!busy, |b| {
                b.on_click(cx.listener(|d, _, _, cx| d.change_password(Change::Lock, cx)))
            })
    });
    token_row("Protected by", None, &t)
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

/// A saved secret's value, as the pane shows it.
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

/// The saved secrets, each with the button that shows it, and the old copies under them.
fn saved_rows(d: &SettingsDialog, cx: &mut Context<SettingsDialog>) -> Vec<AnyElement> {
    let t = *theme(cx);
    let secrets = d.security.secrets.as_deref().unwrap_or_default();
    let mut rows = Vec::new();
    if !secrets.is_empty() {
        rows.push(section_heading("Saved secrets", false, &t).into_any_element());
    }
    for secret in secrets {
        let shown = d.security.revealed.get(&secret.name);
        let name = secret.name.clone();
        let (glyph, action) = if shown.is_some() {
            ("eye-slash", "Hide")
        } else {
            ("eye", "Show")
        };
        rows.push(
            token_row(secret.label.clone(), None, &t)
                .flex_wrap()
                .child(
                    icon_button(
                        SharedString::from(format!("security-show-{}", secret.name)),
                        format!("Show {}", secret.label),
                        glyph,
                        14.,
                        cx,
                    )
                    .aria_toggled(if shown.is_some() {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
                    .tooltip(crate::ui::tooltip(action))
                    .on_click(cx.listener(move |d, _, _, cx| d.toggle_secret(name.clone(), cx))),
                )
                .children(shown.map(|value| value_box(&secret.label, value, &t)))
                .into_any_element(),
        );
    }
    let old = d.security.old;
    if old > 0 {
        let busy = d.security.busy;
        let desc = if old == 1 {
            "1 secret is also in the system keychain or api-keys.json".to_string()
        } else {
            format!("{old} secrets are also in the system keychain or api-keys.json")
        };
        rows.push(
            token_row("Old copies", Some(&desc), &t)
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
