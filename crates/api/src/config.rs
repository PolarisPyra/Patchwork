//! The service's settings, read from the environment as systemd's `EnvironmentFile` sets it.
//! `docs/accounts.md` lists them.

use std::env;
use std::net::SocketAddr;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

/// Postgres over its local socket, as the `patchwork` role that peer auth maps the service's Linux
/// user to. The user is named because the static build can't look it up.
const DEFAULT_DATABASE: &str = "postgresql:///patchwork?host=/var/run/postgresql&user=patchwork";
/// Only Caddy on the same machine reaches it.
const DEFAULT_LISTEN: &str = "127.0.0.1:8080";

pub struct Config {
    pub database_url: String,
    pub listen: SocketAddr,
    /// The key the HMAC of each one-time code is made with, so the database alone can't check one.
    pub code_key: [u8; 32],
    pub mail: MailConfig,
}

pub enum MailConfig {
    /// Sends through the SMTP server at `url`, as `smtps://user:password@host:465`.
    Smtp { url: String, from: String },
    /// Writes each email to the log instead, for running the service on a developer's machine.
    Log,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE.into());
        let listen = env::var("PATCHWORK_API_LISTEN")
            .unwrap_or_else(|_| DEFAULT_LISTEN.into())
            .parse()
            .map_err(|e| format!("PATCHWORK_API_LISTEN: {e}"))?;
        let code_key = STANDARD
            .decode(required("PATCHWORK_API_CODE_KEY")?.trim())
            .ok()
            .and_then(|key| <[u8; 32]>::try_from(key).ok())
            .ok_or("PATCHWORK_API_CODE_KEY must be 32 bytes in base64: openssl rand -base64 32")?;
        let mail = match required("PATCHWORK_API_MAIL")?.as_str() {
            "smtp" => MailConfig::Smtp {
                url: required("PATCHWORK_API_SMTP_URL")?,
                from: required("PATCHWORK_API_MAIL_FROM")?,
            },
            "log" => MailConfig::Log,
            other => return Err(format!("PATCHWORK_API_MAIL is {other}, not smtp or log")),
        };
        Ok(Self {
            database_url,
            listen,
            code_key,
            mail,
        })
    }
}

fn required(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| format!("{name} is not set"))
}
