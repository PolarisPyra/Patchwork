//! The Security pane: what protects the saved secrets, the vault's password (set, changed,
//! removed and entered here) and its recovery key, the secrets that are saved, the copies
//! earlier versions left, and what the chat apps had Patchwork do.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui::{AnyElement, Div, Focusable, Role, SharedString, Toggled, Window, div, prelude::*, px};
use patchwork_core::gateway::ChatAppCall;
use patchwork_core::vault::{Method, Saved, Status, status};
use zeroize::{Zeroize, Zeroizing};

use self::rows::{activity_rows, idle_row, protected_row, recovery_row, saved_rows, screen_row};
use super::controls::{FieldKind, alert, token_row, vault_notice};
use super::dialog::{SettingsDialog, SettingsEvent};
use crate::ui::theme::{fs_sm, theme};
use crate::ui::{BtnKind, btn, icon, icon_button, spinner};

mod rows;

/// The width of a password field.
const FIELD_WIDTH: f32 = 260.;

/// What each password field of the pane is named. The names start alike, so they go together.
const FIELDS: &str = "security-";
const CURRENT: &str = "security-current";
const NEW: &str = "security-new";
const CONFIRM: &str = "security-confirm";
const RECOVERY: &str = "security-recovery";
const ASK: &str = "security-ask";
const ALL: [&str; 5] = [CURRENT, NEW, CONFIRM, RECOVERY, ASK];

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
    /// Opens the vault with its recovery key and sets a new password.
    Recover,
    /// Replaces the recovery key.
    NewKey,
}

/// What a vault that waits is opened with.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Entry {
    #[default]
    Password,
    RecoveryKey,
}

/// What the pane reads from the core when it opens, and again after each change.
#[derive(Default)]
struct Read {
    secrets: Vec<Saved>,
    /// How many copies earlier versions left outside the vault.
    old: usize,
    /// Whether the vault has a recovery key.
    has_key: bool,
    /// What the chat apps had Patchwork do, the latest first.
    activity: Vec<ChatAppCall>,
}

/// The pane's state: what is typed, and how the last change went. What is typed, and every
/// value shown, is wiped from memory when it is let go.
#[derive(Default)]
pub struct SecurityState {
    current: Zeroizing<String>,
    new: Zeroizing<String>,
    confirm: Zeroizing<String>,
    /// The recovery key typed for a vault whose password is lost.
    recovery: Zeroizing<String>,
    /// The password typed before a secret is shown.
    ask: Zeroizing<String>,
    entry: Entry,
    /// The fields that show what is typed.
    shown: HashSet<&'static str>,
    busy: bool,
    /// Whether the password field of a vault that waits has been given the focus.
    focused: bool,
    error: Option<String>,
    /// What the last change did, once it has.
    done: Option<&'static str>,
    /// What the core holds, once it has been read.
    read: Option<Read>,
    /// The values the pane shows, by the secret's name.
    revealed: HashMap<String, Zeroizing<String>>,
    /// The secret that waits for the password to be shown.
    asking: Option<String>,
    /// The recovery key just made, until it is put away.
    new_key: Option<Zeroizing<String>>,
}

impl SettingsDialog {
    /// Shows or hides what is typed in the field named `key`.
    fn toggle_password(&mut self, key: &'static str, cx: &mut Context<Self>) {
        let masked = !self.security.shown.insert(key);
        if masked {
            self.security.shown.remove(key);
        }
        if let Some(area) = self.field_area(key) {
            area.update(cx, |a, cx| {
                a.masked = masked;
                cx.notify();
            });
        }
        cx.notify();
    }

    /// Empties the pane's fields, wiping what was typed in them first.
    fn wipe_fields(&mut self, cx: &mut Context<Self>) {
        for key in ALL {
            if let Some(area) = self.field_area(key) {
                area.update(cx, |a, _| a.content.zeroize());
            }
        }
        self.drop_fields(FIELDS);
    }

    /// Reads which secrets are saved, how many old copies are left, and what the chat apps
    /// did, off the main thread.
    fn read_secrets(&mut self, cx: &mut Context<Self>) {
        // Empty until they are read, so they are asked for once.
        self.security.read = Some(Read::default());
        let core = Arc::clone(&self.core);
        self.run(
            move || Read {
                secrets: core.vault_secrets(),
                old: core.old_secret_copies(),
                has_key: core.vault_has_recovery_key(),
                activity: core.chat_app_activity(),
            },
            |d, read, _| d.security.read = Some(read),
            cx,
        );
    }

    /// Shows the saved secret `name`, or hides it when it shows. A vault with a password asks
    /// for it first, unless it was entered a few minutes ago.
    fn toggle_secret(&mut self, name: String, cx: &mut Context<Self>) {
        self.security.error = None;
        if self.security.revealed.remove(&name).is_some() {
            return cx.notify();
        }
        if self.core.secret_needs_password() {
            self.security.asking = Some(name);
            return cx.notify();
        }
        let core = Arc::clone(&self.core);
        let read = name.clone();
        self.run(
            move || core.reveal_secret(&read).map(Zeroizing::new),
            move |d, value, _| match value {
                Some(value) => {
                    d.security.revealed.insert(name, value);
                }
                None => d.security.error = Some("That secret could not be read".into()),
            },
            cx,
        );
    }

    /// Checks the password typed under a secret, then shows that secret.
    fn confirm_secret(&mut self, cx: &mut Context<Self>) {
        let Some(name) = self.security.asking.clone() else {
            return;
        };
        if self.security.busy {
            return;
        }
        self.security.busy = true;
        self.security.error = None;
        let core = Arc::clone(&self.core);
        let password = std::mem::take(&mut self.security.ask);
        let read = name.clone();
        self.run(
            move || {
                core.confirm_vault_password(&password)
                    .map(|()| core.reveal_secret(&read).map(Zeroizing::new))
            },
            move |d, out, cx| {
                d.security.busy = false;
                d.wipe_fields(cx);
                match out {
                    Ok(Some(value)) => {
                        d.security.asking = None;
                        d.security.revealed.insert(name, value);
                    }
                    Ok(None) => d.security.error = Some("That secret could not be read".into()),
                    Err(e) => d.security.error = Some(e.to_string()),
                }
            },
            cx,
        );
        cx.notify();
    }

    /// Opens or locks the vault, sets, changes or removes its password with what is typed,
    /// replaces its recovery key, or removes the old copies.
    fn change_password(&mut self, change: Change, cx: &mut Context<Self>) {
        if self.security.busy {
            return;
        }
        self.security.done = None;
        if matches!(change, Change::Set | Change::Recover)
            && *self.security.new != *self.security.confirm
        {
            self.security.error = Some("The two new passwords are not the same".into());
            return cx.notify();
        }
        self.security.busy = true;
        self.security.error = None;
        let core = Arc::clone(&self.core);
        let current = std::mem::take(&mut self.security.current);
        let new = std::mem::take(&mut self.security.new);
        let recovery = std::mem::take(&mut self.security.recovery);
        self.security.confirm = Zeroizing::default();
        self.run(
            move || match change {
                Change::Unlock => core.unlock_vault(&current).map(|()| ("Unlocked", None)),
                Change::Set => core
                    .set_vault_password(&current, &new)
                    .map(|key| ("Password saved", Some(Zeroizing::new(key)))),
                Change::Remove => core
                    .remove_vault_password(&current)
                    .map(|()| ("Password removed", None)),
                Change::Lock => core.lock_vault().map(|()| ("Locked", None)),
                Change::Clean => core
                    .remove_old_secret_copies()
                    .map(|_| ("Old copies removed", None)),
                Change::Recover => core
                    .recover_vault(&recovery, &new)
                    .map(|key| ("Password saved", Some(Zeroizing::new(key)))),
                Change::NewKey => core
                    .new_recovery_key(&current)
                    .map(|key| ("Recovery key replaced", Some(Zeroizing::new(key)))),
            },
            move |d, out, cx| {
                d.security.busy = false;
                // The fields start empty after a change, and after a wrong password.
                d.wipe_fields(cx);
                // Nothing shown outlives a change, and the list is read again.
                d.security.revealed.clear();
                d.security.asking = None;
                d.security.read = None;
                d.security.focused = false;
                match out {
                    Ok((done, key)) => {
                        d.security.done = Some(done);
                        d.security.entry = Entry::Password;
                        if key.is_some() || matches!(change, Change::Lock | Change::Remove) {
                            d.security.new_key = key;
                        }
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

/// One of the pane's password fields.
fn password_field(
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
    d.field(
        key,
        title,
        &value,
        "",
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
        icon_button(
            SharedString::from(format!("{key}-show")),
            format!("Show {}", title.to_lowercase()),
            if shown { "eye-slash" } else { "eye" },
            14.,
            cx,
        )
        .size(px(22.))
        .flex_none()
        .aria_toggled(if shown { Toggled::True } else { Toggled::False })
        .tooltip(crate::ui::tooltip(if shown { "Hide" } else { "Show" }))
        .on_click(cx.listener(move |d, _, _, cx| d.toggle_password(key, cx))),
    )
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
    let field = password_field(d, key, title, window, cx);
    d.on_submit(key, move |d, _, cx| d.change_password(submit, cx));
    token_row(title, desc, &t).child(field).into_any_element()
}

/// A plain button of the pane.
fn plain(
    id: &'static str,
    label: &'static str,
    d: &SettingsDialog,
    cx: &mut Context<SettingsDialog>,
    click: impl Fn(&mut SettingsDialog, &mut Context<SettingsDialog>) + 'static,
) -> AnyElement {
    let t = *theme(cx);
    let busy = d.security.busy;
    btn(id, label, BtnKind::Plain, busy, &t)
        .child(label)
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
    div()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(8.))
        .pt(px(10.))
        .children(working)
        .children(done)
        .children(others)
        .child(
            btn("security-save", label, BtnKind::Primary, busy, &t)
                .child(label)
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
            Some("Unlocks the saved keys, tokens and terminal variables"),
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
            rows.push(password_row(
                d,
                key,
                title,
                None,
                Change::Recover,
                window,
                cx,
            ));
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
    rows
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
    if has_password {
        rows.push(idle_row(d, cx));
        // Only Linux has the listener for the screen and for sleep.
        if cfg!(target_os = "linux") {
            rows.push(screen_row(d, cx));
        }
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
        (!has_password).then_some("Asked for each time Patchwork starts"),
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
    rows.extend(saved_rows(d, window, cx));
    rows.extend(activity_rows(d, cx));
    // Without a keychain or a password, nothing locks the secrets: the notice says so.
    rows.extend(vault_notice("security-vault", &t).map(IntoElement::into_any_element));
    rows
}

/// The Security pane: the fields the vault's state asks for, and the buttons under them.
pub(super) fn render(
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
        .children(rows)
        .children(
            d.security
                .error
                .clone()
                .map(|e| alert("security-error", e, t.danger)),
        )
        .into_any_element()
}
