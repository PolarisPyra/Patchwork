//! The chat apps' credentials, kept in the vault.

// Rust guideline compliant 2026-10-09

use super::platform::Platform;
use crate::Core;
use crate::provider::api::read_secret;

/// The credential store name of a platform's credential.
#[must_use]
pub fn secret_name(platform: Platform, key: &str) -> String {
    format!("gateway:{}:{key}", platform.id())
}

/// A platform's saved credential named `key`.
#[must_use]
pub fn token(platform: Platform, key: &str) -> Option<String> {
    read_secret(&secret_name(platform, key)).filter(|t| !t.trim().is_empty())
}

impl Core {
    /// The platform credentials that are saved, by platform and key. This reads the vault, and
    /// the system credential store for one an earlier version saved, which can block.
    #[must_use]
    pub fn chat_app_credentials(&self) -> Vec<(Platform, &'static str)> {
        Platform::ALL
            .iter()
            .flat_map(|p| p.credentials().iter().map(move |c| (*p, c.key)))
            .filter(|(p, key)| token(*p, key).is_some())
            .collect()
    }
}
