//! Synchronous HTTP RPC client over `POST /rpc` and `GET /health`.

// Rust guideline compliant 2026-10-09

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use patchwork_rpc_protocol::Method;
use patchwork_rpc_protocol::devices::{Claimed, Device, EventsSince, PairingCode, PairingInfo};
use patchwork_rpc_protocol::frames::{Answer, Failure, Health, Rejected};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use url::Url;

use crate::ClientError;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

/// Typed HTTP client for a Patchwork server.
pub struct RpcClient {
    base_url: String,
    token: Option<String>,
    agent: ureq::Agent,
    next_id: AtomicU64,
}

impl fmt::Debug for RpcClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RpcClient")
            .field("base_url", &self.base_url)
            .field("authenticated", &self.token.is_some())
            .finish_non_exhaustive()
    }
}

impl RpcClient {
    /// Creates a new HTTP client targeting `base_url` (e.g., `http://127.0.0.1:7703`).
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::InvalidUrl`] if `base_url` is not a valid `http` or `https` URL.
    pub fn new(base_url: impl AsRef<str>, token: Option<String>) -> Result<Self, ClientError> {
        let parsed = Url::parse(base_url.as_ref())
            .map_err(|e| ClientError::InvalidUrl(format!("{}: {e}", base_url.as_ref())))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(ClientError::InvalidUrl(format!(
                "unsupported scheme `{}` (expected http or https)",
                parsed.scheme()
            )));
        }
        let base_url = base_url.as_ref().trim_end_matches('/').to_string();
        Ok(Self {
            base_url,
            token,
            agent: build_agent(DEFAULT_TIMEOUT),
            next_id: AtomicU64::new(1),
        })
    }

    /// Configures the per-request timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.agent = build_agent(timeout);
        self
    }

    /// Returns a client with the given bearer token attached.
    #[must_use]
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Updates or clears the bearer token used for subsequent requests.
    pub fn set_token(&mut self, token: Option<String>) {
        self.token = token;
    }

    /// Returns the normalized base URL without a trailing slash.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Calls `GET /health`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, returns a non-200 status, or has invalid JSON.
    pub fn health(&self) -> Result<Health, ClientError> {
        let mut response = self
            .agent
            .get(&format!("{}/health", self.base_url))
            .call()?;
        let status = response.status().as_u16();
        let body = response.body_mut().read_to_string()?;
        if status != 200 {
            return Err(ClientError::HttpStatus { status, body });
        }
        Ok(serde_json::from_str(&body)?)
    }

    /// Calls an arbitrary RPC `method` with raw JSON `params`.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Unauthorized`] on HTTP 401, [`ClientError::Rpc`] on an RPC error
    /// frame, or a transport/serialization error.
    pub fn call_raw(&self, method: &str, params: &Value) -> Result<Value, ClientError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let payload = json!({
            "id": id,
            "method": method,
            "params": params,
        });
        let mut request = self
            .agent
            .post(&format!("{}/rpc", self.base_url))
            .header("Content-Type", "application/json");
        if let Some(token) = &self.token {
            request = request.header("Authorization", &format!("Bearer {token}"));
        }
        let mut response = request.send_json(&payload)?;
        let status = response.status().as_u16();
        let body = response.body_mut().read_to_string()?;
        if status == 401 {
            let message = serde_json::from_str::<Rejected>(&body)
                .map_or_else(|_| "Unauthorized".to_string(), |r| r.error.message);
            return Err(ClientError::Unauthorized(message));
        }
        if status != 200 {
            return Err(ClientError::HttpStatus { status, body });
        }
        decode_rpc_body(&body)
    }

    /// Calls a typed [`Method`] whose params serialize to JSON and output deserializes from JSON.
    ///
    /// # Errors
    ///
    /// Returns any transport, auth, RPC, or JSON decoding error.
    pub fn call<M>(&self, params: &M::Params) -> Result<M::Output, ClientError>
    where
        M: Method,
        M::Params: Serialize,
        M::Output: DeserializeOwned,
    {
        let value = serde_json::to_value(params)?;
        let result = self.call_raw(M::NAME, &value)?;
        Ok(serde_json::from_value(result)?)
    }

    /// Calls a typed [`Method`] that takes no parameters (empty object `{}`).
    ///
    /// # Errors
    ///
    /// Returns any transport, auth, RPC, or JSON decoding error.
    pub fn call_empty<M>(&self) -> Result<M::Output, ClientError>
    where
        M: Method,
        M::Output: DeserializeOwned,
    {
        let result = self.call_raw(M::NAME, &json!({}))?;
        Ok(serde_json::from_value(result)?)
    }

    /// Calls `auth.pair.start` to mint a single-use pairing code.
    ///
    /// # Errors
    ///
    /// Returns an error if the caller lacks admin scope or the RPC fails.
    pub fn pair_start(
        &self,
        name: Option<&str>,
        scopes: &[&str],
    ) -> Result<PairingCode, ClientError> {
        let mut params = serde_json::Map::new();
        if let Some(name) = name {
            params.insert("name".into(), Value::String(name.to_string()));
        }
        if !scopes.is_empty() {
            params.insert(
                "scopes".into(),
                Value::Array(scopes.iter().map(|s| Value::String((*s).into())).collect()),
            );
        }
        let result = self.call_raw("auth.pair.start", &Value::Object(params))?;
        Ok(serde_json::from_value(result)?)
    }

    /// Calls `auth.pair.claim` to exchange a pairing code for a device token.
    ///
    /// # Errors
    ///
    /// Returns an error if the code is invalid, expired, or rate-limited.
    pub fn pair_claim(
        &self,
        code: &str,
        name: &str,
        platform: &str,
    ) -> Result<Claimed, ClientError> {
        let result = self.call_raw(
            "auth.pair.claim",
            &json!({
                "code": code,
                "name": name,
                "platform": platform,
            }),
        )?;
        Ok(serde_json::from_value(result)?)
    }

    /// Calls `auth.devices` to list paired devices.
    ///
    /// # Errors
    ///
    /// Returns an error if the caller lacks admin scope or the RPC fails.
    pub fn devices(&self) -> Result<Vec<Device>, ClientError> {
        let result = self.call_raw("auth.devices", &json!({}))?;
        Ok(serde_json::from_value(result)?)
    }

    /// Calls `auth.devices.revoke` to revoke a paired device by id.
    ///
    /// # Errors
    ///
    /// Returns an error if the caller lacks admin scope or the RPC fails.
    pub fn revoke_device(&self, id: &str) -> Result<(), ClientError> {
        let _ = self.call_raw("auth.devices.revoke", &json!({ "id": id }))?;
        Ok(())
    }

    /// Calls `pairing.info` to inspect reachability and URLs.
    ///
    /// # Errors
    ///
    /// Returns an error if unauthorized or the RPC fails.
    pub fn pairing_info(&self) -> Result<PairingInfo, ClientError> {
        let result = self.call_raw("pairing.info", &json!({}))?;
        Ok(serde_json::from_value(result)?)
    }

    /// Calls `events.since` to read the current push sequence cursor.
    ///
    /// # Errors
    ///
    /// Returns an error if unauthorized or the RPC fails.
    pub fn events_since(&self) -> Result<EventsSince, ClientError> {
        let result = self.call_raw("events.since", &json!({}))?;
        Ok(serde_json::from_value(result)?)
    }
}

fn build_agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into()
}

pub(crate) fn decode_rpc_body(body: &str) -> Result<Value, ClientError> {
    let raw: Value = serde_json::from_str(body)?;
    if raw.get("error").is_some() {
        let failure: Failure = serde_json::from_value(raw)?;
        return Err(ClientError::Rpc(failure.error.message));
    }
    if raw.get("result").is_some() {
        let answer: Answer = serde_json::from_value(raw)?;
        return Ok(answer.result);
    }
    Err(ClientError::UnexpectedFrame(body.to_string()))
}
