//! What Settings does with the gateway: starting it, its tokens, its status, and pairing.

// Rust guideline compliant 2026-10-09

use patchwork_platform::Locked;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::{Duration, Instant};

use patchwork_core::gateway::{AFTER_UPDATE, AfterUpdate, secret_name, token};

use super::discord::Discord;
use super::hub::spawn;
use super::matrix::Matrix;
use super::mattermost::Mattermost;
use super::outbound::forward;
use super::platform::{Need, Platform};
use super::qq::Qq;
use super::signal::Signal;
use super::slack::Slack;
use super::telegram::Telegram;
use super::types::{Connector, PlatformSettings, Status};
use patchwork_core::provider::api::{delete_secret, store_secret};
use patchwork_platform::now_ms;
use patchwork_core::vault;
use patchwork_core::{Error, Core};
#[allow(
    unused_imports,
    reason = "each file takes the core's gateway methods it calls"
)]
use crate::prelude::*;
use crate::hub::HUB;

/// How long a pairing code works, in milliseconds.
const CODE_LIFETIME: u64 = 10 * 60 * 1000;

/// The letters a pairing code is made of, without ones that read alike.
const CODE_LETTERS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// How many letters a pairing code has.
const CODE_LENGTH: usize = 6;

/// How many wrong guesses end a pairing code, so it cannot be found by trying them all.
const MAX_WRONG_CODES: u32 = 5;

/// How long the conversations' chat apps are given to connect after an update.
const BACK_WAIT: Duration = Duration::from_secs(90);

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
        Platform::Qq => Arc::new(Qq::new(get("app")?, get("secret")?)),
    })
}

/// The core's control methods for the window.
pub trait CoreControl {
    /// Saves a platform's credential, or removes it when `value` is empty, and reconnects. This
    /// writes the vault, which can block.
    ///
    /// # Errors
    ///
    /// Returns an error when the credential cannot be saved or removed.
    fn set_chat_app_credential(
        self: &Arc<Self>,
        platform: Platform,
        key: &str,
        value: &str,
    ) -> Result<(), Error>;

    /// The pairing code that still works, and when it stops.
    #[must_use]
    fn pairing_code(&self) -> Option<(String, u64)>;
}

impl CoreControl for Core {
    fn set_chat_app_credential(
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

    fn pairing_code(&self) -> Option<(String, u64)> {
        let mut state = HUB.lock();
        if state.code.as_ref().is_some_and(|(_, at)| *at <= now_ms()) {
            state.code = None;
        }
        state.code.clone()
    }
}

/// Where each platform's connection stands; platforms not listed are off.
#[must_use]
pub(crate) fn status() -> Vec<(Platform, Status)> {
    let state = HUB.lock();
    let mut list: Vec<_> = state
        .statuses
        .iter()
        .map(|(p, s)| (*p, s.clone()))
        .collect();
    list.sort_by_key(|(p, _)| *p);
    list
}

/// A new one-time code that pairs whoever sends it to a connected bot, and when it stops
/// working, in epoch milliseconds.
#[must_use]
pub(crate) fn make_pairing_code() -> (String, u64) {
    let random = uuid::Uuid::new_v4();
    let code: String = random
        .as_bytes()
        .iter()
        .take(CODE_LENGTH)
        .map(|b| char::from(CODE_LETTERS[usize::from(*b) % CODE_LETTERS.len()]))
        .collect();
    let expires = now_ms() + CODE_LIFETIME;
    let mut state = HUB.lock();
    state.code = Some((code.clone(), expires));
    state.wrong_codes = 0;
    drop(state);
    (code, expires)
}

/// The core's control methods the gateway calls.
pub(crate) trait Control {
    /// Stops every connection and opens the ones the settings turn on. This reads the system
    /// credential store, which can block.
    fn restart_connections(self: &Arc<Self>);

    /// Takes the pairing code when `text` is it, so it works once. Text that could be a code
    /// and is not it counts as a wrong guess, and the code stops working after a few of them.
    fn take_gateway_code(&self, text: &str) -> bool;

    /// See `restart_gateway`.
    fn reopen_connections(self: &Arc<Self>);

    /// Tells the conversations that asked for an update that Patchwork is back, once, when
    /// their chat apps are connected.
    fn tell_back_from_update(self: &Arc<Self>);
}

impl Control for Core {
    fn restart_connections(self: &Arc<Self>) {
        self.reopen_connections();
        self.connection_changed();
    }

    fn take_gateway_code(&self, text: &str) -> bool {
        let text = text.trim();
        // In a group Telegram writes the command as `/pair@BotName CODE`.
        let text = match text.strip_prefix("/pair") {
            Some(rest) if rest.starts_with('@') => rest
                .split_once(char::is_whitespace)
                .map_or("", |(_, code)| code),
            Some(rest) => rest,
            None => text,
        }
        .trim();
        let mut state = HUB.lock();
        let matches = state
            .code
            .as_ref()
            .is_some_and(|(code, at)| *at > now_ms() && code.eq_ignore_ascii_case(text));
        // Only text that could be a code is a guess: a greeting or `/start` is not.
        let guess = state.code.is_some()
            && text.len() == CODE_LENGTH
            && text
                .bytes()
                .all(|b| CODE_LETTERS.contains(&b.to_ascii_uppercase()));
        if !matches && guess {
            state.wrong_codes += 1;
        }
        if matches || state.wrong_codes >= MAX_WRONG_CODES {
            state.code = None;
        }
        matches
    }

    fn reopen_connections(self: &Arc<Self>) {
        // One restart at a time, each reading the settings once it has its turn: the last one
        // to run opens what the newest settings ask for, and no platform is opened twice.
        let _turn = HUB.restarting.locked();
        let settings = self.store().settings().gateway;
        let wanted: Vec<_> = Platform::ALL
            .into_iter()
            .filter(|p| self.store().owns_data() && settings.platform(*p).enabled)
            .map(|p| (p, connector(p, &settings.platform(p))))
            .collect();
        HUB.stop_all();
        if wanted.is_empty() {
            return;
        }
        // A locked vault gives no credentials, and what it guards stays off until it is opened:
        // no chat app may reach Patchwork before then, with a credential or without one.
        if matches!(
            vault::status(),
            vault::Status::Waiting | vault::Status::Locked(_)
        ) {
            let mut state = HUB.lock();
            for (platform, _) in wanted {
                state.statuses.insert(platform, Status::Locked);
            }
            return;
        }
        if !HUB.forwarding() {
            let (sender, receiver) = channel();
            HUB.set_forwarder(sender);
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
                    HUB.lock()
                        .statuses
                        .insert(platform, Status::Error(error.to_string()));
                }
            }
        }
        self.tell_back_from_update();
    }

    fn tell_back_from_update(self: &Arc<Self>) {
        let Some(note) = self
            .store()
            .state(AFTER_UPDATE)
            .and_then(|json| serde_json::from_str::<AfterUpdate>(&json).ok())
        else {
            return;
        };
        // Cleared first, so a restart of the connections does not tell them twice.
        if let Err(error) = self.store().set_state(AFTER_UPDATE, "") {
            log::warn!(error:% = error; "could not clear the update note: {{error}}");
            return;
        }
        let now = env!("CARGO_PKG_VERSION");
        let text = if note.from == now {
            format!("Patchwork is back. It is still on {now}; the update did not install.")
        } else {
            format!("Patchwork is back, updated from {} to {now}.", note.from)
        };
        let core = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("gateway-back".into())
            .spawn(move || {
                let until = Instant::now() + BACK_WAIT;
                let mut waiting = note.conversations;
                while !waiting.is_empty() && Instant::now() < until {
                    let connected: Vec<Platform> = core
                        .gateway_status()
                        .into_iter()
                        .filter(|(_, status)| matches!(status, Status::Connected(_)))
                        .map(|(platform, _)| platform)
                        .collect();
                    waiting.retain(|conversation| {
                        let ready = connected.contains(&conversation.platform);
                        if ready {
                            core.say(conversation.platform, &conversation.conversation, &text);
                        }
                        !ready
                    });
                    std::thread::sleep(Duration::from_millis(500));
                }
                for conversation in waiting {
                    log::warn!(
                        platform = conversation.platform.id();
                        "{{platform}} did not connect in time to say Patchwork is back"
                    );
                }
            });
        if let Err(error) = spawned {
            log::warn!(error:% = error; "could not say Patchwork is back: {{error}}");
        }
    }
}
