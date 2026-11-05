//! The free trial: 14 days of Basic, once per account, as a license key the app keeps like a
//! bought one. The key is signed here with the license signing key, in the shape
//! `commercial/crates/licensing` reads (see `docs/licensing.md`).

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{Duration, Utc};
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::Api;
use crate::accounts::user;
use crate::auth::Session;
use crate::events::{self, Event};
use crate::http::{ApiError, Res};

const DAYS: i64 = 14;

/// `POST /v1/license/trial`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trial {
    /// The license key, for the account's email.
    pub key: String,
}

/// `POST /v1/license/trial`. An account that has had a trial, under this email or another, is
/// refused.
pub(crate) async fn start(State(api): State<Api>, session: Session) -> Res<Json<Trial>> {
    let seed = api.license_key.ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "trials_unavailable",
            "Free trials aren't available right now",
        )
    })?;
    let user = user(&api, session.user_id).await?;
    let expires = Utc::now() + Duration::days(DAYS);
    let id = format!("trial-{}", Uuid::new_v4());
    let mut tx = api.db.begin().await?;
    let started = sqlx::query(
        "insert into trials (user_id, email, license_id, expires_at)
         values ($1, $2::citext, $3, $4) on conflict do nothing",
    )
    .bind(user.id)
    .bind(&user.email)
    .bind(&id)
    .bind(expires)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if started == 0 {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "trial_used",
            "This account has already had its free trial",
        ));
    }
    events::record(
        &mut *tx,
        Event::TrialStarted,
        Some(user.id),
        Some(session.device_id),
        session.ip.as_deref(),
    )
    .await?;
    tx.commit().await?;
    let payload = json!({
        "id": id,
        "plan": "basic",
        "email": user.email,
        "expires": expires.timestamp(),
    })
    .to_string();
    let signature = SigningKey::from_bytes(&seed).sign(payload.as_bytes());
    let key = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(payload),
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    );
    log::info!("trial started for {}, as {id}", user.id);
    Ok(Json(Trial { key }))
}
