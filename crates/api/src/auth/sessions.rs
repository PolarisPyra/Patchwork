//! Session tokens: 32 random bytes the device keeps, of which the service keeps only the SHA-256.
//! A session lasts 90 days from when it was last used, and ends at once when it is revoked or the
//! account's security stamp changes.

use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use sqlx::PgExecutor;
use uuid::Uuid;

use crate::Api;
use crate::http::{ApiError, Res, client_ip};

/// Marks a Patchwork session token, so one pasted somewhere it shouldn't be is easy to spot.
const PREFIX: &str = "pws_";

/// The session a request is signed in with. A route that takes one answers 401 without it.
pub(crate) struct Session {
    pub id: Uuid,
    pub user_id: Uuid,
    pub device_id: Uuid,
    pub ip: Option<String>,
}

impl FromRequestParts<Api> for Session {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, api: &Api) -> Result<Self, ApiError> {
        let token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(ApiError::unauthorized)?;
        let row: Option<(Uuid, Uuid, Uuid)> = sqlx::query_as(
            "update sessions s set last_used_at = now(), expires_at = now() + interval '90 days'
             from users u
             where s.token_hash = $1 and u.id = s.user_id and s.revoked_at is null
               and s.expires_at > now() and s.security_stamp = u.security_stamp
             returning s.id, s.user_id, s.device_id",
        )
        .bind(hash(token.trim()))
        .fetch_optional(&api.db)
        .await?;
        let (id, user_id, device_id) = row.ok_or_else(ApiError::unauthorized)?;
        sqlx::query("update devices set last_seen_at = now() where id = $1")
            .bind(device_id)
            .execute(&api.db)
            .await?;
        Ok(Session {
            id,
            user_id,
            device_id,
            ip: client_ip(&parts.headers),
        })
    }
}

/// Starts a session for `user` on `device`, and returns its token. This is the only time the
/// token exists outside the device.
pub(crate) async fn create<'e>(
    db: impl PgExecutor<'e>,
    user: Uuid,
    device: Uuid,
    stamp: Uuid,
) -> Res<String> {
    let token = new_token(PREFIX)?;
    sqlx::query(
        "insert into sessions (id, user_id, device_id, token_hash, security_stamp, expires_at)
         values ($1, $2, $3, $4, $5, now() + interval '90 days')",
    )
    .bind(Uuid::now_v7())
    .bind(user)
    .bind(device)
    .bind(hash(&token))
    .bind(stamp)
    .execute(db)
    .await?;
    Ok(token)
}

/// 32 random bytes after `prefix`, for a secret only its holder keeps.
pub(crate) fn new_token(prefix: &str) -> Res<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| {
        log::error!("no randomness: {e}");
        ApiError::internal()
    })?;
    Ok(format!("{prefix}{}", URL_SAFE_NO_PAD.encode(bytes)))
}

/// What is kept of a token: its SHA-256.
pub(crate) fn hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// Deletes codes that can no longer be used, failed sign-ins past their window, and sessions that
/// ended a month ago. Runs hourly.
pub(crate) async fn prune(api: &Api) -> sqlx::Result<()> {
    sqlx::query("delete from email_codes where created_at < now() - interval '1 day'")
        .execute(&api.db)
        .await?;
    sqlx::query("delete from login_failures where created_at < now() - interval '1 day'")
        .execute(&api.db)
        .await?;
    sqlx::query(
        "delete from sessions where coalesce(revoked_at, expires_at) < now() - interval '30 days'",
    )
    .execute(&api.db)
    .await?;
    Ok(())
}
