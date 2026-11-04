//! Signing in: an email and password, then a one-time code sent to that email, traded for a
//! session token on the device that asked. An install can skip the code for 30 days once it has
//! entered one. Registering and resetting a password answer the same whether the address has an
//! account or not, and a wrong password reads the same as an address with no account.

mod codes;
mod ops;
pub(crate) mod passwords;
mod sessions;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::accounts::Account;
use crate::devices::{Device, DeviceInfo};

pub(crate) use codes::{By, Purpose, issue, redeem};
pub(crate) use ops::{
    login, login_verify, logout, register, register_finish, reset_password, reset_password_finish,
};
pub(crate) use sessions::{Session, hash, new_token, prune};

/// `POST /v1/auth/register`, `POST /v1/auth/password-reset`: the address to send a code to.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailRequest {
    pub email: String,
}

/// `POST /v1/auth/register/finish`: the code from the email, and the new account's password.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterFinish {
    pub email: String,
    pub code: String,
    pub password: String,
    pub name: Option<String>,
    pub device: DeviceInfo,
}

/// `POST /v1/auth/login`. `rememberToken` is the one a sign-in on this install was given, if any.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
    pub device: DeviceInfo,
    pub remember_token: Option<String>,
}

/// What `POST /v1/auth/login` answers with the right password: a session when the install is
/// remembered, or else the challenge to send back with the code just emailed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum Login {
    SignedIn(Box<SignedIn>),
    #[serde(rename_all = "camelCase")]
    CodeSent {
        challenge: Uuid,
    },
}

/// `POST /v1/auth/login/verify`: the code for a challenge. `remember` skips the code on this
/// install for 30 days.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginVerify {
    pub challenge: Uuid,
    pub code: String,
    pub device: DeviceInfo,
    #[serde(default)]
    pub remember: bool,
}

/// `POST /v1/auth/password-reset/finish`: the code from the email, and the new password.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetFinish {
    pub email: String,
    pub code: String,
    pub password: String,
}

/// A new session. `token` goes in `Authorization: Bearer …` on every later request.
/// `rememberToken`, when the sign-in asked for it, goes in the next `POST /v1/auth/login`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedIn {
    pub token: String,
    pub account: Account,
    pub device: Device,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remember_token: Option<String>,
}
