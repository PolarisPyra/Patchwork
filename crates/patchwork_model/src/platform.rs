//! The chat apps the gateway connects to, and the fields each one is set up with.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

/// A chat app Patchwork can be reached from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Platform {
    Telegram,
    Discord,
    Slack,
    Matrix,
    Mattermost,
    Signal,
    Qq,
}

/// Whether a field must be filled in before the platform connects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    Required,
    /// Optional, and this value is used when it is empty.
    Default(&'static str),
}

/// A value a platform is set up with: a setting, or a secret kept in the credential store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    /// Its name in the settings or the credential store, after the platform's.
    pub key: &'static str,
    pub label: &'static str,
    /// What the field shows while it is empty; for a `Need::Default` field, the default.
    pub placeholder: &'static str,
    pub need: Need,
}

/// A field that must be filled in.
const fn required(key: &'static str, label: &'static str, placeholder: &'static str) -> Field {
    Field {
        key,
        label,
        placeholder,
        need: Need::Required,
    }
}

/// A field that falls back to `default` when it is empty.
const fn default(key: &'static str, label: &'static str, default: &'static str) -> Field {
    Field {
        key,
        label,
        placeholder: default,
        need: Need::Default(default),
    }
}

impl Platform {
    /// Every platform, in the order Settings lists them.
    pub const ALL: [Self; 7] = [
        Self::Telegram,
        Self::Discord,
        Self::Slack,
        Self::Matrix,
        Self::Mattermost,
        Self::Signal,
        Self::Qq,
    ];

    /// The platform's name as people write it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Telegram => "Telegram",
            Self::Discord => "Discord",
            Self::Slack => "Slack",
            Self::Matrix => "Matrix",
            Self::Mattermost => "Mattermost",
            Self::Signal => "Signal",
            Self::Qq => "QQ",
        }
    }

    /// The platform's name in keys and logs.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Telegram => "telegram",
            Self::Discord => "discord",
            Self::Slack => "slack",
            Self::Matrix => "matrix",
            Self::Mattermost => "mattermost",
            Self::Signal => "signal",
            Self::Qq => "qq",
        }
    }

    /// The longest message Patchwork sends at once, in characters; longer replies are split.
    #[must_use]
    pub fn limit(self) -> usize {
        match self {
            Self::Discord | Self::Signal => 2000,
            Self::Slack => 3000,
            Self::Telegram | Self::Qq => 4000,
            Self::Matrix | Self::Mattermost => 16_000,
        }
    }

    /// The secrets it connects with, kept in the system credential store.
    #[must_use]
    pub fn credentials(self) -> &'static [Field] {
        match self {
            Self::Telegram => const { &[required("token", "Bot token", "123456:ABC-DEF…")] },
            Self::Discord => const { &[required("token", "Bot token", "Bot token")] },
            Self::Slack => {
                const {
                    &[
                        required("bot", "Bot token", "xoxb-…"),
                        required("app", "App token", "xapp-…"),
                    ]
                }
            }
            Self::Matrix | Self::Mattermost => {
                const { &[required("token", "Access token", "Access token")] }
            }
            Self::Signal => &[],
            Self::Qq => const { &[required("secret", "Client secret", "Client secret")] },
        }
    }

    /// The plain settings it connects with.
    #[must_use]
    pub fn settings(self) -> &'static [Field] {
        match self {
            Self::Telegram | Self::Discord | Self::Slack => &[],
            Self::Matrix => const { &[required("server", "Homeserver", "https://matrix.org")] },
            Self::Mattermost => {
                const { &[required("server", "Server", "https://chat.example.com")] }
            }
            Self::Signal => {
                const {
                    &[
                        default("server", "signal-cli daemon", "http://127.0.0.1:8080"),
                        required("account", "Phone number", "+15551234567"),
                    ]
                }
            }
            Self::Qq => const { &[required("app", "App ID", "App ID")] },
        }
    }
}
