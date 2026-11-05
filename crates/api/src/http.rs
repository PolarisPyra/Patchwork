//! The HTTP side: the routes, the one error shape every route answers with, a JSON body that
//! fails in that shape too, and the caller's address.

use std::convert::Infallible;
use std::net::IpAddr;

use axum::extract::{FromRequest, FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::{Api, accounts, auth, devices, trials};

pub type Res<T> = Result<T, ApiError>;

/// What a route answers when it can't do what was asked: a status, a code a client can match on,
/// and a message a person can read.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    pub(crate) fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    pub(crate) fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    pub(crate) fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", "Sign in again")
    }

    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    pub(crate) fn too_many(message: impl Into<String>) -> Self {
        Self::new(StatusCode::TOO_MANY_REQUESTS, "too_many_requests", message)
    }

    pub(crate) fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "Something went wrong on Patchwork's side",
        )
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = json!({ "error": { "code": self.code, "message": self.message } });
        (self.status, Json(body)).into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        log::error!("database: {e}");
        Self::internal()
    }
}

/// Whether the database turned a write down for a value another row already has.
pub(crate) fn is_unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(d) if d.is_unique_violation())
}

/// A JSON body, refused with an [`ApiError`] when it doesn't parse.
pub(crate) struct Body<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for Body<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, ApiError> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(Body(value)),
            Err(e) => Err(ApiError::new(e.status(), "bad_request", e.body_text())),
        }
    }
}

/// The caller's address, from the header Cloudflare adds. Only Cloudflare can reach the server's
/// HTTPS port, so nobody else can set it.
pub(crate) struct ClientIp(pub Option<String>);

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Infallible> {
        Ok(ClientIp(client_ip(&parts.headers)))
    }
}

pub(crate) fn client_ip(headers: &HeaderMap) -> Option<String> {
    let header = headers.get("cf-connecting-ip")?.to_str().ok()?;
    header
        .trim()
        .parse::<IpAddr>()
        .ok()
        .map(|ip| ip.to_string())
}

pub fn router(api: Api) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/auth/register", post(auth::register))
        .route("/v1/auth/register/finish", post(auth::register_finish))
        .route("/v1/auth/login", post(auth::login))
        .route("/v1/auth/login/verify", post(auth::login_verify))
        .route("/v1/auth/password-reset", post(auth::reset_password))
        .route(
            "/v1/auth/password-reset/finish",
            post(auth::reset_password_finish),
        )
        .route("/v1/auth/logout", post(auth::logout))
        .route(
            "/v1/accounts/me",
            get(accounts::me)
                .patch(accounts::update)
                .delete(accounts::delete),
        )
        .route("/v1/accounts/email", post(accounts::change_email))
        .route("/v1/accounts/email/verify", post(accounts::verify_email))
        .route(
            "/v1/accounts/security-stamp",
            post(accounts::sign_out_everywhere),
        )
        .route("/v1/accounts/password", post(accounts::change_password))
        .route("/v1/devices", get(devices::list))
        .route("/v1/devices/{id}", delete(devices::revoke))
        .route("/v1/license/trial", post(trials::start))
        .fallback(not_found)
        .with_state(api)
}

/// Up only while the database answers, so a monitor sees an outage either way.
async fn health(State(api): State<Api>) -> Res<Json<Value>> {
    sqlx::query("select 1").execute(&api.db).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn not_found() -> ApiError {
    ApiError::not_found("There is nothing here")
}
