//! An account: who someone is, by the email and password they sign in with. Registering makes it
//! (`auth`); here it is read, renamed, moved to a new email, given a new password, signed out
//! everywhere else, and deleted. Each change that could lock its owner out asks for the password.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgConnection};
use uuid::Uuid;

use crate::Api;
use crate::auth::{By, Purpose, Session, issue, passwords, redeem};
use crate::devices::Device;
use crate::events::{self, Event};
use crate::http::{ApiError, Body, ClientIp, Res, is_unique_violation};

/// The account, as its owner sees it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: Uuid,
    pub email: String,
    pub name: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// `PATCH /v1/accounts/me`. A null or empty name clears it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAccount {
    pub name: Option<String>,
}

/// `POST /v1/accounts/email`: send a code to the address the account should move to.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeEmail {
    pub new_email: String,
    pub password: String,
}

/// `POST /v1/accounts/email/verify`: the code sent to the new address.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeEntry {
    pub code: String,
}

/// `POST /v1/accounts/password`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePassword {
    pub current_password: String,
    pub new_password: String,
}

/// `DELETE /v1/accounts/me`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAccount {
    pub password: String,
}

/// A `users` row, with the security stamp the wire shape leaves out. The password hash is read
/// only where it is checked.
#[derive(FromRow)]
pub(crate) struct UserRow {
    pub id: Uuid,
    pub email: String,
    pub name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub security_stamp: Uuid,
}

/// The columns of a [`UserRow`], for `select` and `returning`. A macro, so each query stays one
/// string literal.
macro_rules! user_columns {
    () => {
        "id, email::text as email, name, created_at, security_stamp"
    };
}
pub(crate) use user_columns;

impl UserRow {
    pub(crate) fn account(&self) -> Account {
        Account {
            id: self.id,
            email: self.email.clone(),
            name: self.name.clone(),
            created_at: self.created_at,
        }
    }
}

impl Api {
    /// The account with this email, and the devices signed in on it.
    pub async fn find_account(&self, email: &str) -> Res<Option<(Account, Vec<Device>)>> {
        let email = normalize_email(email)?;
        let user: Option<UserRow> = sqlx::query_as(concat!(
            "select ",
            user_columns!(),
            " from users where email = $1::citext"
        ))
        .bind(&email)
        .fetch_optional(&self.db)
        .await?;
        let Some(user) = user else {
            return Ok(None);
        };
        let devices = sqlx::query_as(
            "select id, name, platform, client_version as version, last_seen_at, false as current
             from devices where user_id = $1 and active order by last_seen_at desc nulls last",
        )
        .bind(user.id)
        .fetch_all(&self.db)
        .await?;
        Ok(Some((user.account(), devices)))
    }

    /// Ends every session of the account, except `keep` when given.
    pub async fn sign_out_everywhere(
        &self,
        user: Uuid,
        keep: Option<Uuid>,
        ip: Option<&str>,
    ) -> Res<()> {
        let mut tx = self.db.begin().await?;
        end_sessions(&mut tx, user, keep).await?;
        events::record(&mut *tx, Event::SignedOutEverywhere, Some(user), None, ip).await?;
        tx.commit().await?;
        Ok(())
    }
}

/// Changes the account's security stamp, so no session made before can come back, and moves
/// `keep` to the new one. Every install has to enter an email code again, and sign-in codes
/// already sent stop working.
pub(crate) async fn end_sessions(
    conn: &mut PgConnection,
    user: Uuid,
    keep: Option<Uuid>,
) -> Res<()> {
    let stamp = Uuid::new_v4();
    sqlx::query("update users set security_stamp = $2, updated_at = now() where id = $1")
        .bind(user)
        .bind(stamp)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "update sessions set revoked_at = now()
         where user_id = $1 and revoked_at is null and id is distinct from $2",
    )
    .bind(user)
    .bind(keep)
    .execute(&mut *conn)
    .await?;
    sqlx::query("update sessions set security_stamp = $2 where id = $1")
        .bind(keep)
        .bind(stamp)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "update devices set remember_hash = null, remembered_until = null where user_id = $1",
    )
    .bind(user)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "update email_codes set consumed_at = now()
         where user_id = $1 and purpose = 'two_factor' and consumed_at is null",
    )
    .bind(user)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// `GET /v1/accounts/me`.
pub(crate) async fn me(State(api): State<Api>, session: Session) -> Res<Json<Account>> {
    Ok(Json(user(&api, session.user_id).await?.account()))
}

/// `PATCH /v1/accounts/me`.
pub(crate) async fn update(
    State(api): State<Api>,
    session: Session,
    Body(req): Body<UpdateAccount>,
) -> Res<Json<Account>> {
    let name = normalize_name(req.name.as_deref())?;
    let user: UserRow = sqlx::query_as(concat!(
        "update users set name = $2, updated_at = now() where id = $1 returning ",
        user_columns!()
    ))
    .bind(session.user_id)
    .bind(name)
    .fetch_one(&api.db)
    .await?;
    Ok(Json(user.account()))
}

/// `POST /v1/accounts/email`: sends a code to the new address, which [`verify_email`] takes.
pub(crate) async fn change_email(
    State(api): State<Api>,
    session: Session,
    Body(req): Body<ChangeEmail>,
) -> Res<StatusCode> {
    let email = normalize_email(&req.new_email)?;
    let current = user(&api, session.user_id).await?.email;
    confirm_password(&api, &current, &req.password, session.ip.as_deref()).await?;
    if current == email {
        return Err(ApiError::bad_request(
            "same_email",
            "That is already the account's email",
        ));
    }
    let (taken,): (bool,) =
        sqlx::query_as("select exists (select 1 from users where email = $1::citext)")
            .bind(&email)
            .fetch_one(&api.db)
            .await?;
    if taken {
        return Err(email_taken());
    }
    issue(
        &api,
        Purpose::ChangeEmail,
        &email,
        Some(session.user_id),
        session.ip.as_deref(),
    )
    .await?;
    Ok(StatusCode::ACCEPTED)
}

/// `POST /v1/accounts/email/verify`: moves the account to the address the code was sent to. Every
/// other session ends, and the old address is told.
pub(crate) async fn verify_email(
    State(api): State<Api>,
    session: Session,
    Body(req): Body<CodeEntry>,
) -> Res<Json<Account>> {
    let redeemed = redeem(
        &api,
        Purpose::ChangeEmail,
        By::User(session.user_id),
        &req.code,
    )
    .await?;
    let old = user(&api, session.user_id).await?.email;
    let mut tx = api.db.begin().await?;
    let moved: Result<UserRow, sqlx::Error> = sqlx::query_as(concat!(
        "update users set email = $2::citext, email_verified_at = now(), last_email_change_at = now(),
           updated_at = now()
         where id = $1 returning ",
        user_columns!()
    ))
    .bind(session.user_id)
    .bind(&redeemed.email)
    .fetch_one(&mut *tx)
    .await;
    let user = match moved {
        Ok(user) => user,
        Err(e) if is_unique_violation(&e) => return Err(email_taken()),
        Err(e) => return Err(e.into()),
    };
    end_sessions(&mut tx, user.id, Some(session.id)).await?;
    events::record(
        &mut *tx,
        Event::EmailChanged,
        Some(user.id),
        Some(session.device_id),
        session.ip.as_deref(),
    )
    .await?;
    tx.commit().await?;

    let notice = format!(
        "Your Patchwork account now signs in with {}. If you didn't make this change, reply to this \
         email.",
        user.email
    );
    if let Err(e) = api
        .mail
        .send(&old, "Your Patchwork email was changed", &notice)
        .await
    {
        log::warn!("{e}");
    }
    Ok(Json(user.account()))
}

/// `POST /v1/accounts/password`: sets a new password, keeps this session, and ends the others.
pub(crate) async fn change_password(
    State(api): State<Api>,
    session: Session,
    Body(req): Body<ChangePassword>,
) -> Res<StatusCode> {
    let email = user(&api, session.user_id).await?.email;
    confirm_password(&api, &email, &req.current_password, session.ip.as_deref()).await?;
    passwords::policy(&req.new_password)?;
    let hash = passwords::hash(&api, &req.new_password).await?;
    let mut tx = api.db.begin().await?;
    sqlx::query("update users set password_hash = $2, updated_at = now() where id = $1")
        .bind(session.user_id)
        .bind(hash)
        .execute(&mut *tx)
        .await?;
    end_sessions(&mut tx, session.user_id, Some(session.id)).await?;
    events::record(
        &mut *tx,
        Event::PasswordChanged,
        Some(session.user_id),
        Some(session.device_id),
        session.ip.as_deref(),
    )
    .await?;
    tx.commit().await?;

    let notice = "Your Patchwork password was changed, and every other device was signed out. If \
                  you didn't make this change, reset your password from Patchwork's sign-in screen.";
    if let Err(e) = api
        .mail
        .send(&email, "Your Patchwork password was changed", notice)
        .await
    {
        log::warn!("{e}");
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /v1/accounts/security-stamp`: signs out every other session.
pub(crate) async fn sign_out_everywhere(
    State(api): State<Api>,
    session: Session,
) -> Res<StatusCode> {
    api.sign_out_everywhere(session.user_id, Some(session.id), session.ip.as_deref())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /v1/accounts/me`: deletes the account, its devices and its sessions at once.
pub(crate) async fn delete(
    State(api): State<Api>,
    session: Session,
    ClientIp(ip): ClientIp,
    Body(req): Body<DeleteAccount>,
) -> Res<StatusCode> {
    let email = user(&api, session.user_id).await?.email;
    confirm_password(&api, &email, &req.password, ip.as_deref()).await?;
    let mut tx = api.db.begin().await?;
    sqlx::query("delete from users where id = $1")
        .bind(session.user_id)
        .execute(&mut *tx)
        .await?;
    events::record(&mut *tx, Event::AccountDeleted, None, None, ip.as_deref()).await?;
    tx.commit().await?;
    log::info!("account {} deleted", session.user_id);
    Ok(StatusCode::NO_CONTENT)
}

/// The address trimmed and in lower case, or the reason it isn't one.
pub(crate) fn normalize_email(raw: &str) -> Res<String> {
    let email = raw.trim().to_lowercase();
    let valid = email.len() <= 254
        && !email.chars().any(char::is_whitespace)
        && email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && !domain.contains('@')
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        });
    if valid {
        Ok(email)
    } else {
        Err(ApiError::bad_request(
            "invalid_email",
            "That is not an email address",
        ))
    }
}

/// The name trimmed, or none when it is empty, or the reason it can't be taken.
pub(crate) fn normalize_name(raw: Option<&str>) -> Res<Option<String>> {
    let name = raw.map(str::trim).filter(|n| !n.is_empty());
    if name.is_some_and(|n| n.chars().count() > 100) {
        return Err(ApiError::bad_request(
            "invalid_name",
            "A name can be up to 100 characters",
        ));
    }
    Ok(name.map(str::to_owned))
}

pub(crate) fn email_taken() -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "email_taken",
        "Another account already signs in with that email",
    )
}

/// Asks for the account's password again before a change that could lock its owner out.
async fn confirm_password(api: &Api, email: &str, password: &str, ip: Option<&str>) -> Res<()> {
    match passwords::check(api, email, password, ip).await? {
        Some(_) => Ok(()),
        None => Err(ApiError::bad_request(
            "wrong_password",
            "That password is wrong",
        )),
    }
}

async fn user(api: &Api, id: Uuid) -> Res<UserRow> {
    let user = sqlx::query_as(concat!(
        "select ",
        user_columns!(),
        " from users where id = $1"
    ))
    .bind(id)
    .fetch_one(&api.db)
    .await?;
    Ok(user)
}
