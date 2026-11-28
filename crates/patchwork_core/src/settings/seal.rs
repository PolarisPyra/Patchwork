//! Keeping the terminal variables' values out of the saved settings: the values go in the
//! vault, and the settings keep each variable's name.

// Rust guideline compliant 2026-10-09

use std::collections::BTreeMap;

use zeroize::Zeroizing;

use super::types::Settings;
use crate::Error;
use crate::persistence::Store;
use crate::vault;

/// The secret the terminal variables' values are saved as, by variable name.
const TERMINAL_ENV: &str = "terminal:env";

/// The settings as the database keeps them: every terminal variable's value is in the vault,
/// and empty here. Without an open vault, or when it cannot be written, they are `settings`.
pub(super) fn sealed(settings: &Settings) -> Settings {
    if !matches!(vault::status(), vault::Status::Open(_)) {
        return settings.clone();
    }
    let values: BTreeMap<&str, &str> = settings
        .terminal
        .env
        .iter()
        .filter_map(|(name, value)| Some((name.as_str(), value.as_deref()?)))
        .filter(|(_, value)| !value.is_empty())
        .collect();
    let saved = if values.is_empty() {
        vault::remove(TERMINAL_ENV)
    } else {
        serde_json::to_string(&values)
            .map(Zeroizing::new)
            .map_err(|e| e.to_string().into())
            .and_then(|text| {
                // Most saves change another setting, and leave the values as they are.
                if vault::get(TERMINAL_ENV).as_deref() == Some(text.as_str()) {
                    Ok(())
                } else {
                    vault::put(TERMINAL_ENV, &text)
                }
            })
    };
    if let Err(e) = saved {
        log::warn!(error:% = e; "terminal variables stay in the settings: {{error}}");
        return settings.clone();
    }
    let mut kept = settings.clone();
    for value in kept.terminal.env.values_mut().flatten() {
        value.clear();
    }
    kept
}

/// Fills the terminal variables' values in from the vault, as the app starts, and moves the
/// values an earlier version left in the saved settings into it.
///
/// # Errors
///
/// Returns an error when the settings cannot be written to the database.
pub(crate) fn unseal(store: &Store) -> Result<(), Error> {
    if !matches!(vault::status(), vault::Status::Open(_)) {
        return Ok(());
    }
    let values: BTreeMap<String, String> = vault::get(TERMINAL_ENV)
        .map(Zeroizing::new)
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let env = store.settings().terminal.env;
    if values.is_empty() && env.values().flatten().all(String::is_empty) {
        return Ok(());
    }
    store.update_settings_as(
        |settings| {
            for (name, value) in &mut settings.terminal.env {
                if let Some(value) = value
                    && value.is_empty()
                    && let Some(saved) = values.get(name)
                {
                    value.clone_from(saved);
                }
            }
        },
        sealed,
    )
}

/// Empties the terminal variables' values in memory, as the vault is locked. The vault keeps
/// them, and `unseal` fills them in when it opens.
///
/// # Errors
///
/// Returns an error when the settings cannot be written to the database.
pub(crate) fn forget(store: &Store) -> Result<(), Error> {
    store.update_settings_as(
        |settings| {
            for value in settings.terminal.env.values_mut().flatten() {
                value.clear();
            }
        },
        Settings::clone,
    )
}
