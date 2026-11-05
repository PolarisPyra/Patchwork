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
    /// The Ed25519 key license keys are signed with, for free trials; None turns trials off.
    pub license_key: Option<[u8; 32]>,
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
        let license_key = match env::var("PATCHWORK_API_LICENSE_KEY_FILE") {
            Ok(path) => Some(signing_key(&path)?),
            Err(_) => None,
        };
        Ok(Self {
            database_url,
            listen,
            code_key,
            mail,
            license_key,
        })
    }
}

fn required(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| format!("{name} is not set"))
}

/// The seed of the Ed25519 private key in the PEM file at `path`, as `openssl genpkey -algorithm
/// ed25519` writes it: PKCS#8, whose DER ends with the 32-byte seed after a fixed prefix.
fn signing_key(path: &str) -> Result<[u8; 32], String> {
    const PREFIX: [u8; 16] = [
        0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04,
        0x20,
    ];
    let pem = std::fs::read_to_string(path)
        .map_err(|e| format!("PATCHWORK_API_LICENSE_KEY_FILE: couldn't read {path}: {e}"))?;
    let body: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    let der = STANDARD.decode(body.trim()).unwrap_or_default();
    der.strip_prefix(&PREFIX)
        .and_then(|seed| <[u8; 32]>::try_from(seed).ok())
        .ok_or_else(|| {
            format!("PATCHWORK_API_LICENSE_KEY_FILE: {path} is not an Ed25519 private key")
        })
}
