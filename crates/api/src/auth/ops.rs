//! The sign-in routes: register, sign in with a password and an emailed code, reset a forgotten
//! password, and sign out.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use sqlx::PgConnection;
use uuid::Uuid;

use super::codes::{self, By, Purpose};
use super::{
    EmailRequest, Login, LoginRequest, LoginVerify, RegisterFinish, ResetFinish, Session, SignedIn,
    passwords, sessions,
};
use crate::accounts::{
    UserRow, email_taken, end_sessions, normalize_email, normalize_name, user_columns,
};
use crate::devices::{self, DeviceInfo};
use crate::events::{self, Event};
use crate::http::{ApiError, Body, ClientIp, Res, is_unique_violation};
use crate::Api;

/// `POST /v1/auth/register`. Sends the code that finishes the account, or, when the address
/// already has one, an email saying so. Answers 202 either way.
pub(crate) async fn register(
    State(api): State<Api>,
    ClientIp(ip): ClientIp,
    Body(req): Body<EmailRequest>,
) -> Res<StatusCode> {
    let email = normalize_email(&req.email)?;
    if account_id(&api, &email).await?.is_some() {
        let body = "Someone tried to make a Patchwork account with this email, which already has \
                    one. Sign in instead, or reset your password from the sign-in screen.\n\nIf it \
                    wasn't you, you can ignore this email.";
        let instead = Some(("You already have a Patchwork account", body));
        codes::decoy(&api, Purpose::Register, &email, ip.as_deref(), instead).await?;
    } else {
        codes::issue(&api, Purpose::Register, &email, None, ip.as_deref()).await?;
    }
    Ok(StatusCode::ACCEPTED)
}

/// `POST /v1/auth/register/finish`: makes the account and signs this install in. The code has
/// just proved the address, so this sign-in asks for no second one.
pub(crate) async fn register_finish(
    State(api): State<Api>,
    ClientIp(ip): ClientIp,
    Body(req): Body<RegisterFinish>,
) -> Res<Json<SignedIn>> {
    let email = normalize_email(&req.email)?;
    let device = req.device.checked()?;
    let name = normalize_name(req.name.as_deref())?;
    passwords::policy(&req.password)?;
    codes::redeem(&api, Purpose::Register, By::Email(&email), &req.code).await?;
    let hash = passwords::hash(&api, &req.password).await?;

    let mut tx = api.db.begin().await?;
    let made: Result<UserRow, sqlx::Error> = sqlx::query_as(concat!(
        "insert into users
           (id, email, email_verified_at, name, password_hash, security_stamp, last_login_at)
         values ($1, $2::citext, now(), $3, $4, $5, now())
         returning ",
        user_columns!()
    ))
    .bind(Uuid::now_v7())
    .bind(&email)
    .bind(name)
    .bind(hash)
    .bind(Uuid::new_v4())
    .fetch_one(&mut *tx)
    .await;
    let user = match made {
        Ok(user) => user,
        Err(e) if is_unique_violation(&e) => return Err(email_taken()),
        Err(e) => return Err(e.into()),
    };
    events::record(
        &mut *tx,
        Event::Registered,
        Some(user.id),
        None,
        ip.as_deref(),
    )
    .await?;
    let signed_in = start(&mut tx, &user, &device, false, ip.as_deref()).await?;
    tx.commit().await?;
    Ok(Json(signed_in))
}

/// `POST /v1/auth/login`. With the right password, signs in at once on an install that is
/// remembered, and otherwise emails a code for [`login_verify`].
pub(crate) async fn login(
    State(api): State<Api>,
    ClientIp(ip): ClientIp,
    Body(req): Body<LoginRequest>,
) -> Res<Json<Login>> {
    let email = normalize_email(&req.email)?;
    let device = req.device.checked()?;
    let Some(user) = passwords::check(&api, &email, &req.password, ip.as_deref()).await? else {
        return Err(ApiError::bad_request(
            "invalid_credentials",
            "The email or password is wrong",
        ));
    };
    if let Some(token) = &req.remember_token
        && devices::remembered(&api.db, user, &device.identifier, token).await?
    {
        let mut tx = api.db.begin().await?;
        let user = touch(&mut tx, user).await?;
        let signed_in = start(&mut tx, &user, &device, false, ip.as_deref()).await?;
        tx.commit().await?;
        return Ok(Json(Login::SignedIn(Box::new(signed_in))));
    }
    let challenge =
        codes::issue(&api, Purpose::TwoFactor, &email, Some(user), ip.as_deref()).await?;
    Ok(Json(Login::CodeSent { challenge }))
}

/// `POST /v1/auth/login/verify`: the emailed code for a challenge from [`login`].
pub(crate) async fn login_verify(
    State(api): State<Api>,
    ClientIp(ip): ClientIp,
    Body(req): Body<LoginVerify>,
) -> Res<Json<SignedIn>> {
    let device = req.device.checked()?;
    let redeemed =
        codes::redeem(&api, Purpose::TwoFactor, By::Id(req.challenge), &req.code).await?;
    let user = redeemed.user_id.ok_or_else(ApiError::internal)?;
    let mut tx = api.db.begin().await?;
    let user = touch(&mut tx, user).await?;
    let signed_in = start(&mut tx, &user, &device, req.remember, ip.as_deref()).await?;
    tx.commit().await?;
    Ok(Json(signed_in))
}

/// `POST /v1/auth/password-reset`. Emails a code when the address has an account. Answers 202
/// either way, within the same limits.
pub(crate) async fn reset_password(
    State(api): State<Api>,
    ClientIp(ip): ClientIp,
    Body(req): Body<EmailRequest>,
) -> Res<StatusCode> {
    let email = normalize_email(&req.email)?;
    match account_id(&api, &email).await? {
        Some(user) => {
            codes::issue(
                &api,
                Purpose::ResetPassword,
                &email,
                Some(user),
                ip.as_deref(),
            )
            .await?;
        }
        None => codes::decoy(&api, Purpose::ResetPassword, &email, ip.as_deref(), None).await?,
    }
    Ok(StatusCode::ACCEPTED)
}

/// `POST /v1/auth/password-reset/finish`: sets the new password and ends every session.
pub(crate) async fn reset_password_finish(
    State(api): State<Api>,
    ClientIp(ip): ClientIp,
    Body(req): Body<ResetFinish>,
) -> Res<StatusCode> {
    let email = normalize_email(&req.email)?;
    passwords::policy(&req.password)?;
    let redeemed =
        codes::redeem(&api, Purpose::ResetPassword, By::Email(&email), &req.code).await?;
    let user = redeemed.user_id.ok_or_else(ApiError::internal)?;
    let hash = passwords::hash(&api, &req.password).await?;
    let mut tx = api.db.begin().await?;
    sqlx::query("update users set password_hash = $2, updated_at = now() where id = $1")
        .bind(user)
        .bind(hash)
        .execute(&mut *tx)
        .await?;
    end_sessions(&mut tx, user, None).await?;
    events::record(
        &mut *tx,
        Event::PasswordReset,
        Some(user),
        None,
        ip.as_deref(),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /v1/auth/logout`: ends the session the request is signed in with.
pub(crate) async fn logout(State(api): State<Api>, session: Session) -> Res<StatusCode> {
    sqlx::query("update sessions set revoked_at = now() where id = $1")
        .bind(session.id)
        .execute(&api.db)
        .await?;
    events::record(
        &api.db,
        Event::SignedOut,
        Some(session.user_id),
        Some(session.device_id),
        session.ip.as_deref(),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Signs `user` in on `device`: adds or brings back the device, remembers it when asked, and
/// starts the session.
async fn start(
    conn: &mut PgConnection,
    user: &UserRow,
    device: &DeviceInfo,
    remember: bool,
    ip: Option<&str>,
) -> Res<SignedIn> {
    let device = devices::upsert(&mut *conn, user.id, device).await?;
    let remember_token = match remember {
        true => Some(devices::remember(&mut *conn, device.id).await?),
        false => None,
    };
    let token = sessions::create(&mut *conn, user.id, device.id, user.security_stamp).await?;
    events::record(
        &mut *conn,
        Event::SignedIn,
        Some(user.id),
        Some(device.id),
        ip,
    )
    .await?;
    log::info!("signed in: account {} on device {}", user.id, device.id);
    Ok(SignedIn {
        token,
        account: user.account(),
        device,
        remember_token,
    })
}

/// Notes the sign-in on the account, and reads it back.
async fn touch(conn: &mut PgConnection, user: Uuid) -> Res<UserRow> {
    let user = sqlx::query_as(concat!(
        "update users set last_login_at = now() where id = $1 returning ",
        user_columns!()
    ))
    .bind(user)
    .fetch_one(conn)
    .await?;
    Ok(user)
}

async fn account_id(api: &Api, email: &str) -> Res<Option<Uuid>> {
    let id = sqlx::query_scalar("select id from users where email = $1::citext")
        .bind(email)
        .fetch_optional(&api.db)
        .await?;
    Ok(id)
}
