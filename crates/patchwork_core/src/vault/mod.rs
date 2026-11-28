//! Vault integration with `Core`.
//!
//! Delegates encryption, key management, and password/recovery flows to `patchwork-vault` while
//! wiring log scrubbing, settings unsealing, gateways, and agent secret requests (`requests`).

// Rust guideline compliant 2026-10-09

pub mod requests;

use std::path::Path;
use std::sync::Arc;

pub use patchwork_vault::{Method, Saved, Status, VaultSettings, VaultTimeout, status};
pub(crate) use patchwork_vault::{get, keep_open, names, put, remove};

use crate::gateway::Platform;
use crate::persistence::DB_FILE;
use crate::{Core, Error, logging, provider, settings};

/// Opens this instance's vault and connects it to the log scrubber.
pub(crate) fn open(data: &Path, config: &Path, keep: bool) {
    patchwork_vault::set_known_sink(logging::set_known);
    patchwork_vault::open(&data.join(DB_FILE), config, keep);
}

/// Copies every secret an earlier version left in its stores into the vault, so the vault
/// holds them all, and not only the ones read so far. The credential store's copies stay where
/// they were until `remove_old_secret_copies`.
pub(crate) fn take_old() {
    if !matches!(status(), Status::Open(_)) {
        return;
    }
    let mut old: Vec<String> = Vec::new();
    for provider in provider::api::providers() {
        old.push(format!("{}:headers", provider.id));
        old.push(provider.id);
    }
    for platform in Platform::ALL {
        let id = platform.id();
        old.extend(
            platform
                .credentials()
                .iter()
                .map(|field| format!("gateway:{id}:{}", field.key)),
        );
    }
    let before = names().len();
    for name in &old {
        let _ = provider::api::read_secret(name);
    }
    // Listing the MCP servers reads each one's saved values.
    let _ = provider::mcp::list();
    let taken = names().len().saturating_sub(before);
    if taken > 0 {
        log::info!(taken; "copied {{taken}} secrets from the old stores into the vault");
    }
}

/// What the secret `name` is, in words.
fn label(name: &str) -> String {
    if name == "terminal:env" {
        return "Terminal variables".into();
    }
    if let Some(server) = name.strip_prefix("mcp/") {
        return format!("MCP server {server}");
    }
    if let Some((id, key)) = name
        .strip_prefix("gateway:")
        .and_then(|rest| rest.split_once(':'))
        && let Some(platform) = Platform::ALL.into_iter().find(|p| p.id() == id)
    {
        let field = platform
            .credentials()
            .iter()
            .find(|f| f.key == key)
            .map_or(key, |f| f.label);
        return format!("{} {}", platform.label(), field.to_lowercase());
    }
    let (id, what) = name
        .strip_suffix(":headers")
        .map_or((name, "API key"), |id| (id, "headers"));
    // A secret whose provider is gone has only an id, which says nothing to the user.
    match provider::api::provider(id) {
        Some(provider) => format!("{} {what}", provider.name),
        None if what == "headers" => "API headers".into(),
        None => what.into(),
    }
}

impl Core {
    /// Starts what waited for the vault's secrets.
    fn vault_opened(self: &Arc<Self>) -> Result<(), Error> {
        settings::unseal(&self.store)?;
        take_old();
        provider::mcp::restart_changed();
        self.restart_gateway();
        Ok(())
    }

    /// Opens the vault with its password, then starts what waited for its secrets: the
    /// terminal variables, the MCP servers and the chat apps. This blocks while the key is
    /// derived and the chat apps connect.
    ///
    /// # Errors
    ///
    /// Returns an error when the password is not the vault's, or the settings cannot be saved.
    pub fn unlock_vault(self: &Arc<Self>, password: &str) -> Result<(), Error> {
        patchwork_vault::unlock_with(password)?;
        self.vault_opened()
    }

    /// Whether showing a saved secret takes the vault's password first.
    #[must_use]
    pub fn secret_needs_password(&self) -> bool {
        patchwork_vault::needs_password()
    }

    /// Takes the vault's password, so saved secrets are shown for the next few minutes. This
    /// blocks while the key is derived.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault is not open, or the password is not the vault's.
    pub fn confirm_vault_password(&self, password: &str) -> Result<(), Error> {
        patchwork_vault::confirm_password(password)
    }

    /// Shuts a vault that has a password, and takes its secrets from the terminal variables,
    /// the MCP servers and the chat apps. It opens when the password is entered. The agents
    /// that run keep what they started with: `lock_vault` stops those too.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault has no password, or the settings cannot be saved.
    pub(crate) fn shut_vault(self: &Arc<Self>) -> Result<(), Error> {
        patchwork_vault::lock()?;
        provider::api::forget_secrets();
        settings::forget(&self.store)?;
        provider::mcp::restart_changed();
        self.restart_gateway();
        Ok(())
    }

    /// Sets the vault's password, or changes it from `current`, and gives the new recovery
    /// key, which opens the vault when the password is lost. It is kept nowhere: the user
    /// writes it down now. From then on the vault opens only once the password is entered.
    /// A vault that had no key yet is made by this, and what waited for it starts. This blocks
    /// while the key is derived, and for that vault while the chat apps connect.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault is not open, `current` is not its password, `new` is
    /// too short, or the database cannot be written.
    pub fn set_vault_password(self: &Arc<Self>, current: &str, new: &str) -> Result<String, Error> {
        let made = status() == Status::Unset;
        let shown = patchwork_vault::set_password(current, new)?;
        // The password is set by now, and the recovery key has to reach the user.
        if made && let Err(e) = self.vault_opened() {
            log::warn!(error:% = e; "the vault was made, and its settings were not read: {{error}}");
        }
        Ok(shown)
    }

    /// Removes the vault's password: the system keychain keeps its key.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault is not open, `current` is not its password, the system
    /// has no keychain, or the database cannot be written.
    pub fn remove_vault_password(&self, current: &str) -> Result<(), Error> {
        patchwork_vault::remove_password(current)
    }

    /// Opens a vault whose password is lost with its recovery key, makes `new` its password,
    /// and gives the recovery key that replaces the one used. Then starts what waited for the
    /// vault's secrets. This blocks while the key is derived and the chat apps connect.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault has no recovery key, `recovery` is not it, `new` is
    /// too short, or the database cannot be written.
    pub fn recover_vault(self: &Arc<Self>, recovery: &str, new: &str) -> Result<String, Error> {
        let shown = patchwork_vault::recover(recovery, new)?;
        // The password is changed by now, and the new recovery key has to reach the user.
        if let Err(e) = self.vault_opened() {
            log::warn!(error:% = e; "the vault opened, and its settings were not read: {{error}}");
        }
        Ok(shown)
    }

    /// Replaces the vault's recovery key, or makes one for a vault whose password is older
    /// than recovery keys. The one before opens nothing. This blocks while the key is derived.
    ///
    /// # Errors
    ///
    /// Returns an error when `current` is not the vault's password, or the database cannot be
    /// written.
    pub fn new_recovery_key(&self, current: &str) -> Result<String, Error> {
        patchwork_vault::new_recovery_key(current)
    }

    /// Saves the recovery key `key` to `path`, in a file only the user can read.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be written.
    pub fn save_recovery_key(&self, path: &Path, key: &str) -> Result<(), Error> {
        patchwork_vault::save_recovery_key(path, key)
    }

    /// Whether the vault has a password and a recovery key for it.
    #[must_use]
    pub fn vault_has_recovery_key(&self) -> bool {
        patchwork_vault::has_recovery_key()
    }

    /// The secrets saved in the vault, by what they are. None of their values.
    #[must_use]
    pub fn vault_secrets(&self) -> Vec<Saved> {
        let mut saved: Vec<Saved> = names()
            .into_iter()
            .map(|name| Saved {
                label: label(&name),
                name,
            })
            .collect();
        saved.sort_by_key(|s| s.label.to_lowercase());
        saved
    }

    /// The value of the saved secret `name`, for Settings to show when asked. No tool an agent
    /// or a chat app has reaches this. None while the vault's password has to be entered
    /// first (see `secret_needs_password`).
    #[must_use]
    pub fn reveal_secret(&self, name: &str) -> Option<String> {
        if patchwork_vault::needs_password() {
            return None;
        }
        get(name)
    }

    /// How many saved secrets the system keychain still holds a copy of, from an earlier
    /// version. A debug build shares the keychain with the released app, and counts none.
    #[must_use]
    pub fn old_secret_copies(&self) -> usize {
        if cfg!(debug_assertions) {
            return 0;
        }
        provider::api::old_copies(&names())
    }

    /// Removes those copies, and says how many.
    ///
    /// # Errors
    ///
    /// Returns an error in a debug build, when the vault is not open, or when a copy cannot be
    /// moved or removed.
    pub fn remove_old_secret_copies(&self) -> Result<usize, Error> {
        if cfg!(debug_assertions) {
            return Err("A debug build leaves the released app's stores alone".into());
        }
        provider::api::remove_old_copies(&names())
    }
}
