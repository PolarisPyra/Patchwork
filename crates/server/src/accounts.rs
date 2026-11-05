//! The optional Patchwork account, from Settings → Account: signing in to the hosted account
//! service (`crates/api`, see `docs/accounts.md`) and looking after the account. Nothing else in
//! the app needs one. The session and the token that skips the email code live in the system
//! keychain, never in the database.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;

use crate::{Res, Server};

/// The account service, unless `PATCHWORK_API_URL` points a dev build at a local one.
const API_URL: &str = "https://api.patchworkeditor.com";
const KEYCHAIN_SERVICE: &str = "dev.patchwork.ade";
/// The keychain entries: the session, the email it is signed in as, and the token that lets this
/// install skip the email code.
const SESSION: &str = "account:session";
const EMAIL: &str = "account:email";
const REMEMBER: &str = "account:remember";
/// The `app_state` key of the identifier this install signs in as.
const DEVICE_KEY: &str = "account-device";

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "accounts.ts")]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
}

/// An install the account is signed in on.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "accounts.ts")]
#[serde(rename_all = "camelCase")]
pub struct AccountDevice {
    pub id: String,
    pub name: String,
    /// `linux`, `macos` or `windows`.
    pub platform: String,
    pub version: Option<String>,
    /// RFC 3339.
    pub last_seen_at: Option<String>,
    /// This install.
    pub current: bool,
}

/// What a password sign-in leads to: a code to enter, or, on a remembered install, the account.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "accounts.ts")]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum LoginStep {
    CodeSent { challenge: String },
    SignedIn { account: Account },
}

/// A new session, as the service answers it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Session {
    token: String,
    account: Account,
    remember_token: Option<String>,
}

impl Server {
    /// The account this install is signed in to, or None. A session the service has ended (signed
    /// out elsewhere, a new password) is dropped here.
    pub fn account(&self) -> Res<Option<Account>> {
        if secret(SESSION).is_none() {
            return Ok(None);
        }
        match authed(Method::Get, "/v1/accounts/me", Value::Null) {
            Ok(body) => {
                let account: Account = parse(body)?;
                keep(EMAIL, &account.email)?;
                Ok(Some(account))
            }
            Err(_) if secret(SESSION).is_none() => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Emails the code that proves the address, to finish registering with.
    pub fn account_register(&self, email: &str) -> Res<()> {
        let body = json!({ "email": email.trim() });
        send(Method::Post, "/v1/auth/register", body, None).map(drop)
    }

    pub fn account_register_finish(
        &self,
        email: &str,
        code: &str,
        password: &str,
        name: Option<String>,
    ) -> Res<Account> {
        let body = json!({
            "email": email.trim(),
            "code": code.trim(),
            "password": password,
            "name": name.filter(|n| !n.trim().is_empty()),
            "device": self.device()?,
        });
        let session = start(send(Method::Post, "/v1/auth/register/finish", body, None)?)?;
        log::info!("account registered and signed in");
        Ok(session.account)
    }

    pub fn account_login(&self, email: &str, password: &str) -> Res<LoginStep> {
        let body = json!({
            "email": email.trim(),
            "password": password,
            "device": self.device()?,
            "rememberToken": secret(REMEMBER),
        });
        let answer = send(Method::Post, "/v1/auth/login", body, None)?;
        if answer["status"] == "codeSent" {
            let challenge = answer["challenge"]
                .as_str()
                .ok_or("The account service sent no sign-in challenge")?;
            return Ok(LoginStep::CodeSent {
                challenge: challenge.to_string(),
            });
        }
        let session = start(answer)?;
        log::info!("account signed in on a remembered install");
        Ok(LoginStep::SignedIn {
            account: session.account,
        })
    }

    /// Finishes a sign-in with the emailed code. `remember` skips the code on this install for
    /// 30 days.
    pub fn account_login_verify(
        &self,
        challenge: &str,
        code: &str,
        remember: bool,
    ) -> Res<Account> {
        let body = json!({
            "challenge": challenge,
            "code": code.trim(),
            "device": self.device()?,
            "remember": remember,
        });
        let session = start(send(Method::Post, "/v1/auth/login/verify", body, None)?)?;
        match session.remember_token.as_deref() {
            Some(token) if remember => keep(REMEMBER, token)?,
            _ => forget(REMEMBER),
        }
        log::info!("account signed in");
        Ok(session.account)
    }

    /// Emails a code to set a new password with.
    pub fn account_reset_password(&self, email: &str) -> Res<()> {
        let body = json!({ "email": email.trim() });
        send(Method::Post, "/v1/auth/password-reset", body, None).map(drop)
    }

    /// Sets the new password. Every session ends, this install's too if it was signed in.
    pub fn account_reset_password_finish(
        &self,
        email: &str,
        code: &str,
        password: &str,
    ) -> Res<()> {
        let body = json!({ "email": email.trim(), "code": code.trim(), "password": password });
        send(Method::Post, "/v1/auth/password-reset/finish", body, None)?;
        end_session();
        log::info!("account password reset");
        Ok(())
    }

    /// Names the account; None or an empty name clears it.
    pub fn account_rename(&self, name: Option<String>) -> Res<Account> {
        let name = name.filter(|n| !n.trim().is_empty());
        parse(authed(
            Method::Patch,
            "/v1/accounts/me",
            json!({ "name": name }),
        )?)
    }

    pub fn account_devices(&self) -> Res<Vec<AccountDevice>> {
        parse(authed(Method::Get, "/v1/devices", Value::Null)?)
    }

    /// Ends the sessions of another install signed in to the account.
    pub fn account_remove_device(&self, id: &str) -> Res<()> {
        let id = uuid::Uuid::parse_str(id).map_err(|_| "That device is not valid")?;
        authed(Method::Delete, &format!("/v1/devices/{id}"), json!({})).map(drop)
    }

    /// Signs this install out. It is signed out here even when the service can't be reached.
    pub fn account_sign_out(&self) -> Res<()> {
        if let Err(e) = authed(Method::Post, "/v1/auth/logout", json!({})) {
            log::warn!("account sign-out: the service did not end the session: {e}");
        }
        end_session();
        log::info!("account signed out");
        Ok(())
    }

    /// Ends every session but this one, and forgets every remembered install.
    pub fn account_sign_out_others(&self) -> Res<()> {
        authed(Method::Post, "/v1/accounts/security-stamp", json!({}))?;
        forget(REMEMBER);
        log::info!("account signed out on every other device");
        Ok(())
    }

    pub fn account_delete(&self, password: &str) -> Res<()> {
        let body = json!({ "password": password });
        authed(Method::Delete, "/v1/accounts/me", body)?;
        end_session();
        forget(REMEMBER);
        log::info!("account deleted");
        Ok(())
    }

    /// This install, as it describes itself when signing in. Its identifier is made once and kept,
    /// so signing in again reuses the same device.
    fn device(&self) -> Res<Value> {
        let identifier = match self.store.state(DEVICE_KEY) {
            Some(id) => id,
            None => {
                let id = crate::system::new_id();
                self.store.set_state(DEVICE_KEY, &id)?;
                id
            }
        };
        Ok(json!({
            "identifier": identifier,
            "name": device_name(),
            "platform": std::env::consts::OS,
            "version": crate::VERSION,
        }))
    }
}

#[derive(Clone, Copy)]
enum Method {
    Get,
    Post,
    Patch,
    Delete,
}

/// A request with this install's session, which fails when it is signed out.
fn authed(method: Method, path: &str, body: Value) -> Res<Value> {
    let token = secret(SESSION).ok_or("Sign in first")?;
    send(method, path, body, Some(&token))
}

/// Sends a request to the account service and answers its JSON body, or the service's message for
/// an error. A 401 to a request with a session means the session has ended, so it is dropped.
fn send(method: Method, path: &str, body: Value, token: Option<&str>) -> Res<Value> {
    let base = std::env::var("PATCHWORK_API_URL").unwrap_or_else(|_| API_URL.to_string());
    let url = format!("{}{path}", base.trim_end_matches('/'));
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .http_status_as_error(false)
        .build()
        .into();
    let sent = match method {
        Method::Get => bearer(agent.get(&url), token).call(),
        Method::Post => bearer(agent.post(&url), token).send_json(&body),
        Method::Patch => bearer(agent.patch(&url), token).send_json(&body),
        Method::Delete => bearer(agent.delete(&url).force_send_body(), token).send_json(&body),
    };
    let mut response =
        sent.map_err(|e| format!("Couldn't reach Patchwork's account service: {e}"))?;
    let status = response.status().as_u16();
    let text = response.body_mut().read_to_string().unwrap_or_default();
    let answer: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if (200..300).contains(&status) {
        return Ok(answer);
    }
    if status == 401 && token.is_some() {
        end_session();
        log::info!("account session ended by the service");
    }
    Err(answer["error"]["message"].as_str().map_or_else(
        || format!("The account service answered HTTP {status}"),
        String::from,
    ))
}

fn bearer<B>(request: ureq::RequestBuilder<B>, token: Option<&str>) -> ureq::RequestBuilder<B> {
    match token {
        Some(token) => request.header("Authorization", &format!("Bearer {token}")),
        None => request,
    }
}

fn parse<T: serde::de::DeserializeOwned>(body: Value) -> Res<T> {
    serde_json::from_value(body)
        .map_err(|e| format!("The account service sent an answer Patchwork can't read: {e}"))
}

/// Keeps a new session's token.
fn start(answer: Value) -> Res<Session> {
    let session: Session = parse(answer)?;
    keep(SESSION, &session.token)?;
    keep(EMAIL, &session.account.email)?;
    Ok(session)
}

fn end_session() {
    forget(SESSION);
    forget(EMAIL);
}

/// A 14-day Basic license key for the account signed in, which the service signs once per account.
pub(crate) fn trial_key() -> Res<String> {
    let answer = authed(Method::Post, "/v1/license/trial", json!({}))?;
    answer["key"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| "The account service sent no license key".to_string())
}

/// The email of the account this install is signed in to, read from the keychain without asking
/// the service.
pub(crate) fn signed_in_email() -> Option<String> {
    secret(SESSION)?;
    secret(EMAIL)
}

fn entry(name: &str) -> Res<keyring::Entry> {
    keyring::Entry::new(KEYCHAIN_SERVICE, name)
        .map_err(|e| format!("Couldn't open the system keychain: {e}"))
}

fn secret(name: &str) -> Option<String> {
    entry(name).ok()?.get_password().ok()
}

fn keep(name: &str, value: &str) -> Res<()> {
    entry(name)?
        .set_password(value)
        .map_err(|e| format!("Couldn't save the sign-in in the system keychain: {e}"))
}

fn forget(name: &str) {
    if let Ok(entry) = entry(name) {
        let _ = entry.delete_credential();
    }
}

/// The machine's name, shown in the account's list of devices.
fn device_name() -> String {
    #[cfg(unix)]
    let name = {
        let mut buf = [0u8; 256];
        // SAFETY: buf is valid for the length passed, and gethostname writes only within it.
        let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        match rc {
            0 => String::from_utf8_lossy(&buf[..end]).into_owned(),
            _ => String::new(),
        }
    };
    #[cfg(windows)]
    let name = std::env::var("COMPUTERNAME").unwrap_or_default();
    match name.trim() {
        "" => "Patchwork".to_string(),
        name => name.chars().take(100).collect(),
    }
}
