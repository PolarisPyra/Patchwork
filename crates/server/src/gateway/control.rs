//! What Settings does with the gateway: starting it, its tokens, its status, and pairing.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::sync::mpsc::channel;

use super::discord::Discord;
use super::email::Email;
use super::hub::spawn;
use super::irc::{Irc, IrcSetup};
use super::matrix::Matrix;
use super::mattermost::Mattermost;
use super::outbound::forward;
use super::platform::{Need, Platform};
use super::qq::Qq;
use super::signal::Signal;
use super::slack::Slack;
use super::sms::Sms;
use super::telegram::Telegram;
use super::types::{Connector, PlatformSettings, Status};
use crate::provider::api::config::{delete_secret, read_secret, store_secret};
use crate::system::now_ms;
use crate::{Error, Server};

/// How long a pairing code works, in milliseconds.
const CODE_LIFETIME: u64 = 10 * 60 * 1000;

/// The letters a pairing code is made of, without ones that read alike.
const CODE_LETTERS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// How many letters a pairing code has.
const CODE_LENGTH: usize = 6;

impl Server {
    /// Where each platform's connection stands; platforms not listed are off.
    #[must_use]
    pub fn gateway_status(&self) -> Vec<(Platform, Status)> {
        let state = self.gateway.lock();
        let mut list: Vec<_> = state
            .statuses
            .iter()
            .map(|(p, s)| (*p, s.clone()))
            .collect();
        list.sort_by_key(|(p, _)| *p);
        list
    }

    /// The platform credentials that are saved, by platform and key. This reads the system
    /// credential store, which can block.
    #[must_use]
    pub fn gateway_credentials(&self) -> Vec<(Platform, &'static str)> {
        Platform::ALL
            .iter()
            .flat_map(|p| p.credentials().iter().map(move |c| (*p, c.key)))
            .filter(|(p, key)| token(*p, key).is_some())
            .collect()
    }

    /// Saves a platform's credential, or removes it when `value` is empty, and reconnects. This
    /// writes the system credential store, which can block.
    ///
    /// # Errors
    ///
    /// Returns an error when the credential cannot be saved or removed.
    pub fn set_gateway_credential(
        self: &Arc<Self>,
        platform: Platform,
        key: &str,
        value: &str,
    ) -> Result<(), Error> {
        let name = secret_name(platform, key);
        let value = value.trim();
        if value.is_empty() {
            delete_secret(&name);
        } else {
            store_secret(&name, value)?;
        }
        self.restart_gateway();
        Ok(())
    }

    /// A new one-time code that pairs whoever sends it to a connected bot, and when it stops
    /// working, in epoch milliseconds.
    #[must_use]
    pub fn new_gateway_code(&self) -> (String, u64) {
        let random = uuid::Uuid::new_v4();
        let code: String = random
            .as_bytes()
            .iter()
            .take(CODE_LENGTH)
            .map(|b| char::from(CODE_LETTERS[usize::from(*b) % CODE_LETTERS.len()]))
            .collect();
        let expires = now_ms() + CODE_LIFETIME;
        self.gateway.lock().code = Some((code.clone(), expires));
        (code, expires)
    }

    /// The pairing code that still works, and when it stops.
    #[must_use]
    pub fn gateway_code(&self) -> Option<(String, u64)> {
        let mut state = self.gateway.lock();
        if state.code.as_ref().is_some_and(|(_, at)| *at <= now_ms()) {
            state.code = None;
        }
        state.code.clone()
    }

    /// Takes the pairing code when `text` is it, so it works once.
    pub(super) fn take_gateway_code(&self, text: &str) -> bool {
        let text = text.trim();
        let text = text.strip_prefix("/pair").unwrap_or(text).trim();
        let mut state = self.gateway.lock();
        let matches = state
            .code
            .as_ref()
            .is_some_and(|(code, at)| *at > now_ms() && code.eq_ignore_ascii_case(text));
        if matches {
            state.code = None;
        }
        matches
    }

    /// Stops every connection and opens the ones the settings turn on. This reads the system
    /// credential store, which can block.
    pub fn restart_gateway(self: &Arc<Self>) {
        let settings = self.store.settings().gateway;
        let wanted: Vec<_> = Platform::ALL
            .into_iter()
            .filter(|p| self.store.owns_data() && settings.platform(*p).enabled)
            .map(|p| (p, connector(p, &settings.platform(p))))
            .collect();
        self.gateway.stop_all();
        if wanted.is_empty() {
            return;
        }
        if !self.gateway.forwarding() {
            let (sender, receiver) = channel();
            self.gateway.set_forwarder(sender);
            let weak = Arc::downgrade(self);
            let spawned = std::thread::Builder::new()
                .name("gateway-out".into())
                .spawn(move || forward(&weak, &receiver));
            if let Err(error) = spawned {
                log::error!(error:% = error; "could not start the gateway: {{error}}");
            }
        }
        for (platform, made) in wanted {
            match made {
                Ok(connector) => spawn(self, platform, connector),
                Err(error) => {
                    self.gateway
                        .lock()
                        .statuses
                        .insert(platform, Status::Error(error.to_string()));
                }
            }
        }
    }
}

/// The credential store name of a platform's credential.
pub(super) fn secret_name(platform: Platform, key: &str) -> String {
    format!("gateway:{}:{key}", platform.id())
}

/// A platform's saved credential named `key`.
fn token(platform: Platform, key: &str) -> Option<String> {
    read_secret(&secret_name(platform, key)).filter(|t| !t.trim().is_empty())
}

/// A platform's field `key`, from its settings or the credential store: its default when it is
/// empty and has one, or an error that says to add it when it is required.
pub(super) fn field(
    platform: Platform,
    settings: &PlatformSettings,
    key: &str,
) -> Result<String, Error> {
    let setting = platform.settings().iter().find(|f| f.key == key);
    let (field, value) = if let Some(field) = setting {
        (
            field,
            settings.value(key).trim().trim_end_matches('/').to_string(),
        )
    } else {
        let field = platform
            .credentials()
            .iter()
            .find(|f| f.key == key)
            .ok_or_else(|| format!("{} has no {key} field", platform.label()))?;
        (field, token(platform, key).unwrap_or_default())
    };
    if !value.is_empty() {
        return Ok(value);
    }
    match field.need {
        Need::Required => Err(format!("Add the {}", field.label.to_lowercase()).into()),
        Need::Optional => Ok(String::new()),
        Need::Default(value) => Ok(value.to_string()),
    }
}

/// A connection to `platform` made from its settings and saved credentials.
fn connector(platform: Platform, settings: &PlatformSettings) -> Result<Arc<dyn Connector>, Error> {
    let get = |key: &str| field(platform, settings, key);
    Ok(match platform {
        Platform::Telegram => Arc::new(Telegram::new(get("token")?)),
        Platform::Discord => Arc::new(Discord::new(get("token")?)),
        Platform::Slack => Arc::new(Slack::new(get("bot")?, get("app")?)),
        Platform::Matrix => Arc::new(Matrix::new(get("server")?, get("token")?)),
        Platform::Mattermost => Arc::new(Mattermost::new(get("server")?, get("token")?)),
        Platform::Signal => Arc::new(Signal::new(get("server")?, get("account")?)),
        Platform::Irc => Arc::new(Irc::new(IrcSetup {
            server: get("server")?,
            nick: get("nick")?,
            channels: get("channels")?,
            password: get("password")?,
            nickserv: get("nickserv")?,
        })?),
        Platform::Email => Arc::new(Email::new(
            get("address")?,
            &get("imap")?,
            &get("smtp")?,
            get("password")?,
        )?),
        Platform::Sms => Arc::new(Sms::new(&get("account")?, get("number")?, &get("token")?)),
        Platform::Qq => Arc::new(Qq::new(get("app")?, get("secret")?)),
    })
}
