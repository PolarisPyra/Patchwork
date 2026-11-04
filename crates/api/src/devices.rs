//! The installs an account is signed in on: one per copy of the app, by the random identifier it
//! makes once, so signing in again from the same install updates its row instead of adding one.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

use crate::Api;
use crate::auth::{Session, hash, new_token};
use crate::events::{self, Event};
use crate::http::{ApiError, Res};

const PLATFORMS: [&str; 3] = ["linux", "macos", "windows"];
/// Marks a token that lets an install skip the email code, so one pasted somewhere is easy to spot.
const REMEMBER_PREFIX: &str = "pwr_";

/// The install signing in, as it describes itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    /// A random identifier the install makes once and keeps.
    pub identifier: String,
    /// The machine's name, shown in the list of devices.
    pub name: String,
    /// `linux`, `macos` or `windows`.
    pub platform: String,
    /// The app's version.
    pub version: Option<String>,
}

/// An install the account is signed in on.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: Uuid,
    pub name: String,
    pub platform: String,
    pub version: Option<String>,
    pub last_seen_at: Option<DateTime<Utc>>,
    /// The device the request came from.
    pub current: bool,
}

impl DeviceInfo {
    /// The same, trimmed, or the reason it can't be taken.
    pub(crate) fn checked(&self) -> Res<DeviceInfo> {
        let invalid = |what: &str| {
            ApiError::bad_request(
                "invalid_device",
                format!("The device's {what} is not valid"),
            )
        };
        let identifier = self.identifier.trim();
        if identifier.is_empty() || identifier.len() > 64 {
            return Err(invalid("identifier"));
        }
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > 100 {
            return Err(invalid("name"));
        }
        if !PLATFORMS.contains(&self.platform.as_str()) {
            return Err(invalid("platform"));
        }
        let version = self
            .version
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty());
        if version.is_some_and(|v| v.len() > 32) {
            return Err(invalid("version"));
        }
        Ok(DeviceInfo {
            identifier: identifier.to_owned(),
            name: name.to_owned(),
            platform: self.platform.clone(),
            version: version.map(str::to_owned),
        })
    }
}

/// Adds the install to the account, or brings back and updates the row it already has.
pub(crate) async fn upsert<'e>(
    db: impl PgExecutor<'e>,
    user: Uuid,
    info: &DeviceInfo,
) -> Res<Device> {
    let device = sqlx::query_as(
        "insert into devices (id, user_id, identifier, name, platform, client_version, last_seen_at)
         values ($1, $2, $3, $4, $5, $6, now())
         on conflict (user_id, identifier) do update set name = excluded.name,
           platform = excluded.platform, client_version = excluded.client_version,
           active = true, last_seen_at = now()
         returning id, name, platform, client_version as version, last_seen_at, true as current",
    )
    .bind(Uuid::now_v7())
    .bind(user)
    .bind(&info.identifier)
    .bind(&info.name)
    .bind(&info.platform)
    .bind(&info.version)
    .fetch_one(db)
    .await?;
    Ok(device)
}

/// Lets `device` sign in without an email code for 30 days, and returns the token it shows to do so.
pub(crate) async fn remember<'e>(db: impl PgExecutor<'e>, device: Uuid) -> Res<String> {
    let token = new_token(REMEMBER_PREFIX)?;
    sqlx::query(
        "update devices set remember_hash = $2, remembered_until = now() + interval '30 days'
         where id = $1",
    )
    .bind(device)
    .bind(hash(&token))
    .execute(db)
    .await?;
    Ok(token)
}

/// Whether the install `identifier` of `user` was remembered with `token`, and still is.
pub(crate) async fn remembered<'e>(
    db: impl PgExecutor<'e>,
    user: Uuid,
    identifier: &str,
    token: &str,
) -> Res<bool> {
    let (found,): (bool,) = sqlx::query_as(
        "select exists (select 1 from devices where user_id = $1 and identifier = $2 and active
           and remember_hash = $3 and remembered_until > now())",
    )
    .bind(user)
    .bind(identifier)
    .bind(hash(token.trim()))
    .fetch_one(db)
    .await?;
    Ok(found)
}

/// `GET /v1/devices`: the installs signed in, most recently seen first.
pub(crate) async fn list(State(api): State<Api>, session: Session) -> Res<Json<Vec<Device>>> {
    let devices = sqlx::query_as(
        "select id, name, platform, client_version as version, last_seen_at, id = $2 as current
         from devices where user_id = $1 and active order by last_seen_at desc nulls last",
    )
    .bind(session.user_id)
    .bind(session.device_id)
    .fetch_all(&api.db)
    .await?;
    Ok(Json(devices))
}

/// `DELETE /v1/devices/{id}`: signs the install out and takes it off the list. It may be the
/// device the request came from.
pub(crate) async fn revoke(
    State(api): State<Api>,
    session: Session,
    Path(id): Path<String>,
) -> Res<StatusCode> {
    let gone = || ApiError::not_found("That device is not signed in to this account");
    let id: Uuid = id.parse().map_err(|_| gone())?;
    let mut tx = api.db.begin().await?;
    let revoked = sqlx::query(
        "update devices set active = false, remember_hash = null, remembered_until = null
             where id = $1 and user_id = $2 and active",
    )
    .bind(id)
    .bind(session.user_id)
    .execute(&mut *tx)
    .await?;
    if revoked.rows_affected() == 0 {
        return Err(gone());
    }
    sqlx::query(
        "update sessions set revoked_at = now() where device_id = $1 and revoked_at is null",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    events::record(
        &mut *tx,
        Event::DeviceRevoked,
        Some(session.user_id),
        Some(id),
        session.ip.as_deref(),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
