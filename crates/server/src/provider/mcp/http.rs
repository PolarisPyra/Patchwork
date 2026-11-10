//! A server at a URL, spoken to over MCP's streamable HTTP.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::Value;

use super::{announce, Answer, CALL_TIMEOUT, Notices, PROTOCOL};

#[derive(Clone)]
pub(super) struct HttpServer {
    url: Arc<str>,
    headers: Arc<BTreeMap<String, String>>,
    session: Arc<Mutex<Option<String>>>,
    http: ureq::Agent,
    /// Cleared when the server cannot be reached or has dropped the session, so the next
    /// call connects again.
    pub(super) alive: Arc<AtomicBool>,
    notices: Notices,
}

impl HttpServer {
    pub(super) fn new(url: &str, headers: &BTreeMap<String, String>, notices: Notices) -> Self {
        let http = ureq::Agent::config_builder()
            // Generous for a remote server; a slower connect means it is unreachable.
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_global(Some(CALL_TIMEOUT))
            .http_status_as_error(false)
            .build()
            .into();
        HttpServer {
            url: url.into(),
            headers: Arc::new(headers.clone()),
            session: Arc::default(),
            http,
            alive: Arc::new(AtomicBool::new(true)),
            notices,
        }
    }

    pub(super) fn post(&self, message: &Value) -> Result<ureq::http::Response<ureq::Body>, Error> {
        let mut request = self
            .http
            .post(&*self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", PROTOCOL);
        if let Some(session) = self
            .session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_deref()
        {
            request = request.header("Mcp-Session-Id", session);
        }
        for (name, value) in self.headers.iter() {
            request = request.header(name, value);
        }
        let response = request.send(message.to_string()).map_err(|e| {
            self.alive.store(false, Ordering::Relaxed);
            format!("Could not reach {}: {e}", self.url)
        })?;
        if let Some(session) = response
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
        {
            *self.session.lock().unwrap_or_else(PoisonError::into_inner) = Some(session.into());
        }
        Ok(response)
    }

    /// Sends a request on its own thread, so the caller can stop waiting for it.
    pub(super) fn send_request(&self, id: u64, message: Value) -> Answer {
        let (tx, rx) = mpsc::channel();
        let server = self.clone();
        std::thread::spawn(move || {
            let _ = tx.send(server.request(id, &message));
        });
        rx
    }

    /// Sends a request; the answer comes as JSON, or as the matching event of an SSE stream.
    fn request(&self, id: u64, message: &Value) -> Result<Value, Error> {
        let mut response = self.post(message)?;
        let status = response.status().as_u16();
        // A server answers 404 to a session it has ended; a new one starts on the next call.
        if status == 404
            && self
                .session
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_some()
        {
            self.alive.store(false, Ordering::Relaxed);
            return Err("The MCP server ended its session".into());
        }
        if !response.status().is_success() {
            let text = response.body_mut().read_to_string().unwrap_or_default();
            let hint = if matches!(status, 401 | 403) {
                " Check the server's headers."
            } else {
                ""
            };
            return Err(format!(
                "The MCP server answered {status}: {}.{hint}",
                text.trim().chars().take(300).collect::<String>()
            )
            .into());
        }
        let sse = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|t| t.starts_with("text/event-stream"));
        let mut body = response.into_body().into_reader();
        if !sse {
            let mut text = String::new();
            body.read_to_string(&mut text).map_err(|e| e.to_string())?;
            return serde_json::from_str(&text)
                .map_err(|e| format!("The MCP server sent bad JSON: {e}").into());
        }
        let mut data = String::new();
        for line in BufReader::new(body).lines() {
            let line = line.map_err(|e| format!("The MCP server's reply broke off: {e}"))?;
            if let Some(chunk) = line.strip_prefix("data:") {
                data.push_str(chunk.trim_start());
            } else if line.is_empty()
                && !data.is_empty()
                && let Ok(v) = serde_json::from_str::<Value>(&std::mem::take(&mut data))
            {
                match (v.get("id"), v.get("method")) {
                    (Some(_), None) if v["id"].as_u64() == Some(id) => return Ok(v),
                    (None, Some(_)) => announce(&self.notices, &v),
                    _ => {}
                }
            }
        }
        Err("The MCP server ended its reply without an answer".into())
    }
}

// ---- Sharing servers with the CLIs ----------------------------------------------------------
//
// Claude Code and Codex would start their own copy of a server Patchwork already runs. Most
// servers run fine twice, and each copy keeps its client's session apart, but some fight over a
// port, a lock, or a program they drive. For a server the user sets to be shared, CLI turns
// instead start Patchwork's binary as a relay (`--mcp-proxy`), which passes the CLI's messages
// to the copy Patchwork runs. The relay reaches Patchwork over a local socket; the port and a token that
// proves the caller may use it are in a file only the user can read.
