//! Passwords: Argon2id with OWASP's parameters (19 MiB, two passes, one lane) and a random salt for
//! each, kept as a PHC string so the parameters travel with the hash. Hashing runs on the blocking
//! pool, a few at a time, so a burst of sign-ins can't use up the server's memory.

use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use uuid::Uuid;

use crate::Api;
use crate::http::{ApiError, Res};

const MIN_CHARS: usize = 12;
const MAX_CHARS: usize = 128;
/// Wrong passwords allowed in 15 minutes for one address, and from one IP across every address.
const FAILURES_PER_ADDRESS: i64 = 10;
const FAILURES_PER_IP: i64 = 50;

/// How many hashes may run at once.
pub(crate) const AT_ONCE: usize = 4;

/// The reason `password` can't be used, if there is one. Length is the only rule.
pub(crate) fn policy(password: &str) -> Res<()> {
    let chars = password.chars().count();
    if chars < MIN_CHARS {
        return Err(ApiError::bad_request(
            "weak_password",
            format!("A password needs at least {MIN_CHARS} characters"),
        ));
    }
    if chars > MAX_CHARS {
        return Err(ApiError::bad_request(
            "invalid_password",
            format!("A password can be up to {MAX_CHARS} characters"),
        ));
    }
    Ok(())
}

/// The PHC string to keep for `password`.
pub(crate) async fn hash(api: &Api, password: &str) -> Res<String> {
    let password = password.to_owned();
    hashing(api, move || {
        Argon2::default()
            .hash_password(password.as_bytes())
            .map(|h| h.to_string())
            .map_err(|e| {
                log::error!("couldn't hash a password: {e}");
                ApiError::internal()
            })
    })
    .await
}

/// A PHC string for a random password, made once at startup, which [`check`] verifies against
/// when no account has the address, so the answer takes as long either way.
pub(crate) fn dummy_hash() -> Result<String, String> {
    Argon2::default()
        .hash_password(Uuid::new_v4().as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| format!("couldn't hash a password: {e}"))
}

/// The account `email` signs in to, when `password` is its password. A wrong one counts against
/// the address and the IP, and past their limits the answer is 429 whatever the password.
pub(crate) async fn check(
    api: &Api,
    email: &str,
    password: &str,
    ip: Option<&str>,
) -> Res<Option<Uuid>> {
    let (by_address, by_ip): (i64, i64) = sqlx::query_as(
        "select count(*) filter (where email = $1::citext),
                count(*) filter (where $2::inet is not null and ip = $2::inet)
         from login_failures where created_at > now() - interval '15 minutes'",
    )
    .bind(email)
    .bind(ip)
    .fetch_one(&api.db)
    .await?;
    if by_address >= FAILURES_PER_ADDRESS || by_ip >= FAILURES_PER_IP {
        return Err(ApiError::too_many(
            "Too many wrong passwords. Wait 15 minutes and try again",
        ));
    }

    let row: Option<(Uuid, String)> =
        sqlx::query_as("select id, password_hash from users where email = $1::citext")
            .bind(email)
            .fetch_optional(&api.db)
            .await?;
    let (user, stored) = match row {
        Some((id, stored)) => (Some(id), stored),
        None => (None, api.dummy_hash.to_string()),
    };
    let password = password.to_owned();
    let matches = hashing(api, move || {
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), stored.as_str())
            .is_ok())
    })
    .await?;
    if matches && user.is_some() {
        return Ok(user);
    }
    sqlx::query("insert into login_failures (email, ip) values ($1::citext, $2::inet)")
        .bind(email)
        .bind(ip)
        .execute(&api.db)
        .await?;
    Ok(None)
}

/// Runs `work` on the blocking pool once one of the [`AT_ONCE`] hashing slots is free.
async fn hashing<T: Send + 'static>(
    api: &Api,
    work: impl FnOnce() -> Res<T> + Send + 'static,
) -> Res<T> {
    let _slot = api
        .hashing
        .acquire()
        .await
        .map_err(|_| ApiError::internal())?;
    tokio::task::spawn_blocking(work).await.map_err(|e| {
        log::error!("password hashing stopped: {e}");
        ApiError::internal()
    })?
}
