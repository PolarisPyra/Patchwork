//! The Vault pane's state, and what it does with the vault: reading and showing secrets,
//! downloading the recovery key, and setting, changing or removing the password.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use gpui::prelude::*;
use patchwork_backend::gateway::ChatAppCall;
use patchwork_backend::vault::Saved;
use zeroize::{Zeroize, Zeroizing};
use crate::settings::dialog::{SettingsDialog, SettingsEvent};

/// The width of a password field.
pub(super) const FIELD_WIDTH: f32 = 260.;

/// What each password field of the pane is named. The names start alike, so they go together.
pub(super) const FIELDS: &str = "security-";
pub(super) const CURRENT: &str = "security-current";
pub(super) const NEW: &str = "security-new";
pub(super) const CONFIRM: &str = "security-confirm";
pub(super) const RECOVERY: &str = "security-recovery";
pub(super) const ASK: &str = "security-ask";
pub(super) const ALL: [&str; 5] = [CURRENT, NEW, CONFIRM, RECOVERY, ASK];

/// What the pane does with the passwords typed in it.
#[derive(Clone, Copy)]
pub(super) enum Change {
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
pub(super) enum Entry {
    #[default]
    Password,
    RecoveryKey,
}

/// What the pane reads from the core when it opens, and again after each change.
#[derive(Default)]
pub(super) struct Read {
    pub(super) secrets: Vec<Saved>,
    /// How many copies earlier versions left outside the vault.
    pub(super) old: usize,
    /// What the chat apps had Patchwork do, the latest first.
    pub(super) activity: Vec<ChatAppCall>,
}

/// The pane's state: what is typed, and how the last change went. What is typed, and every
/// value shown, is wiped from memory when it is let go.
#[derive(Default)]
pub struct SecurityState {
    pub(super) current: Zeroizing<String>,
    pub(super) new: Zeroizing<String>,
    pub(super) confirm: Zeroizing<String>,
    /// The recovery key typed for a vault whose password is lost.
    pub(super) recovery: Zeroizing<String>,
    /// The password typed before a secret is shown.
    pub(super) ask: Zeroizing<String>,
    pub(super) entry: Entry,
    /// The fields that show what is typed.
    pub(super) shown: HashSet<&'static str>,
    pub(super) busy: bool,
    /// Whether the password field of a vault that waits has been given the focus.
    pub(super) focused: bool,
    pub(super) error: Option<String>,
    /// What the last change did, once it has.
    pub(super) done: Option<&'static str>,
    /// What the core holds, once it has been read.
    pub(super) read: Option<Read>,
    /// The values the pane shows, by the secret's name.
    pub(super) revealed: HashMap<String, Zeroizing<String>>,
    /// The secret that waits for the password to be shown.
    pub(super) asking: Option<String>,
    /// The recovery key just made, until it is put away.
    pub(super) new_key: Option<Zeroizing<String>>,
}

impl SettingsDialog {
    /// Shows or hides what is typed in the field named `key`.
    pub(super) fn toggle_password(&mut self, key: &'static str, cx: &mut Context<Self>) {
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
    pub(super) fn wipe_fields(&mut self, cx: &mut Context<Self>) {
        for key in ALL {
            if let Some(area) = self.field_area(key) {
                area.update(cx, |a, _| a.content.zeroize());
            }
        }
        self.drop_fields(FIELDS);
    }

    /// Reads which secrets are saved, how many old copies are left, and what the chat apps
    /// did, off the main thread.
    pub(super) fn read_secrets(&mut self, cx: &mut Context<Self>) {
        // Empty until they are read, so they are asked for once.
        self.security.read = Some(Read::default());
        let core = Arc::clone(&self.core);
        self.run(
            move || Read {
                secrets: core.vault_secrets(),
                old: core.old_secret_copies(),
                activity: core.chat_app_activity(),
            },
            |d, read, _| d.security.read = Some(read),
            cx,
        );
    }

    /// Shows the saved secret `name`, or hides it when it shows. A vault with a password asks
    /// for it first, unless it was entered a few minutes ago.
    pub(super) fn toggle_secret(&mut self, name: String, cx: &mut Context<Self>) {
        self.security.error = None;
        if self.security.revealed.remove(&name).is_some() {
            return cx.notify();
        }
        if self.core.secret_needs_password() {
            self.security.asking = Some(name);
            // The field that asks takes the focus when it is drawn.
            self.security.focused = false;
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
    pub(super) fn confirm_secret(&mut self, cx: &mut Context<Self>) {
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

    /// Saves the recovery key just made to the Downloads folder, or to the home folder
    /// when there is none.
    ///
    /// It asks nothing: a save dialog that fails to open would leave the key unsaved.
    pub(super) fn download_key(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.security.new_key.clone() else {
            return;
        };
        let (folder, done) = match dirs::download_dir().filter(|d| d.is_dir()) {
            Some(folder) => (folder, "Saved to Downloads as patchwork_recoverykey.txt"),
            None => (
                patchwork_platform::files::paths::home_dir().unwrap_or_default(),
                "Saved to your home folder as patchwork_recoverykey.txt",
            ),
        };
        let path = folder.join("patchwork_recoverykey.txt");
        self.security.error = None;
        self.security.done = None;
        let core = Arc::clone(&self.core);
        cx.spawn(async move |this, cx| {
            let saved = cx
                .background_spawn(async move { core.save_recovery_key(&path, &key) })
                .await;
            let _ = this.update(cx, |d, cx| {
                match saved {
                    Ok(()) => d.security.done = Some(done),
                    Err(e) => {
                        log::warn!(error:% = e; "could not save the recovery key: {{error}}");
                        d.security.error = Some(e.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Opens or locks the vault, sets, changes or removes its password with what is typed,
    /// replaces its recovery key, or removes the old copies.
    pub(super) fn change_password(&mut self, change: Change, cx: &mut Context<Self>) {
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
