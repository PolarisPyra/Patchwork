//! Patchwork's hosted service: accounts, signing in with a password and a code sent by email, the
//! devices an account is signed in on, and its free trial. `docs/accounts.md` describes the routes
//! and how to run it.

pub mod accounts;
pub mod auth;
pub mod config;
pub mod devices;
mod email;
mod events;
pub mod http;
pub mod trials;

use std::sync::Arc;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tokio::sync::Semaphore;

pub use config::Config;
use email::Mailer;

/// What every request reaches: the database, the mail sender, the key that seals codes, the slots
/// password hashing takes turns in, and the key free trials are signed with.
#[derive(Clone)]
pub struct Api {
    db: PgPool,
    mail: Mailer,
    code_key: [u8; 32],
    hashing: Arc<Semaphore>,
    dummy_hash: Arc<str>,
    license_key: Option<[u8; 32]>,
}

impl Api {
    pub async fn open(config: &Config) -> Result<Self, String> {
        let db = PgPoolOptions::new()
            .max_connections(10)
            .connect(&config.database_url)
            .await
            .map_err(|e| {
                format!(
                    "couldn't reach the database at {}: {e}",
                    config.database_url
                )
            })?;
        Ok(Self {
            db,
            mail: Mailer::new(&config.mail)?,
            code_key: config.code_key,
            hashing: Arc::new(Semaphore::new(auth::passwords::AT_ONCE)),
            dummy_hash: auth::passwords::dummy_hash()?.into(),
            license_key: config.license_key,
        })
    }

    /// Brings the database up to the newest migration in `migrations/`.
    pub async fn migrate(&self) -> Result<(), String> {
        sqlx::migrate!()
            .run(&self.db)
            .await
            .map_err(|e| format!("couldn't migrate the database: {e}"))
    }

    /// Deletes the codes, sessions and failed sign-ins nobody needs anymore.
    pub async fn prune(&self) -> Result<(), String> {
        auth::prune(self)
            .await
            .map_err(|e| format!("couldn't prune: {e}"))
    }
}
