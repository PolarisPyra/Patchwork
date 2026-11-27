//! The OpenCode server a turn starts and stops, and the HTTP calls made to it.

// Rust guideline compliant 2026-10-09

use std::io::{BufReader, Read};
use std::process::{Child, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use base64::Engine;
use patchwork_provider_core::command_size;
use patchwork_platform::Error;
use serde_json::Value;

use crate::bridge::{Request, START_TIMEOUT, log_error};

/// The private server, stopped when the turn ends.
pub(crate) struct Server {
    child: Child,
    pub(crate) url: String,
    pub(crate) password: String,
}

impl Server {
    pub(crate) fn start(request: &Request) -> Result<Self, Error> {
        let password = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let mut cmd = patchwork_platform::host_command(&request.bin);
        cmd.args(["serve", "--hostname", "127.0.0.1", "--port", "0"])
            .current_dir(&request.root)
            .env("PWD", &request.root)
            .env("OPENCODE_PASSWORD", &password)
            .env_remove("OPENCODE_SERVER_PASSWORD")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command_size::spawn(&mut cmd, "opencode serve")
            .map_err(|e| format!("Failed to start OpenCode: {e}"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or("OpenCode's output is unavailable")?;
        let stderr = child
            .stderr
            .take()
            .ok_or("OpenCode's output is unavailable")?;
        std::thread::spawn(move || {
            for line in patchwork_platform::lossy_lines(BufReader::new(stderr)) {
                log_error(&line);
            }
        });
        let (found, url) = mpsc::channel();
        std::thread::spawn(move || {
            let mut found = Some(found);
            for line in patchwork_platform::lossy_lines(BufReader::new(stdout)) {
                if let Some(url) = line.split("server listening on").nth(1)
                    && let Some(found) = found.take()
                {
                    let _ = found.send(url.trim().to_string());
                }
            }
        });
        let mut server = Server {
            child,
            url: String::new(),
            password,
        };
        server.url = url
            .recv_timeout(START_TIMEOUT)
            .map_err(|_discarded| "OpenCode's server did not start".to_string())?;
        Ok(server)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Requests to the server, with its Basic credentials.
#[derive(Clone)]
pub(crate) struct Api {
    agent: ureq::Agent,
    stream_agent: ureq::Agent,
    base: String,
    auth: String,
}

impl Api {
    pub(crate) fn new(base: &str, password: &str) -> Self {
        let config = |timeout| {
            ureq::Agent::config_builder()
                .timeout_global(timeout)
                .http_status_as_error(false)
                .build()
                .into()
        };
        let credentials =
            base64::engine::general_purpose::STANDARD.encode(format!("opencode:{password}"));
        Api {
            agent: config(Some(Duration::from_secs(30))),
            stream_agent: config(None),
            base: base.trim_end_matches('/').to_string(),
            auth: format!("Basic {credentials}"),
        }
    }

    pub(crate) fn get(&self, path: &str) -> Result<Value, Error> {
        let response = self
            .agent
            .get(format!("{}{path}", self.base))
            .header("Authorization", &self.auth)
            .call();
        read(path, response)
    }

    pub(crate) fn post(&self, path: &str, body: &Value) -> Result<Value, Error> {
        let response = self
            .agent
            .post(format!("{}{path}", self.base))
            .header("Authorization", &self.auth)
            .send_json(body);
        read(path, response)
    }

    pub(crate) fn put(&self, path: &str, body: &Value) -> Result<Value, Error> {
        let response = self
            .agent
            .put(format!("{}{path}", self.base))
            .header("Authorization", &self.auth)
            .send_json(body);
        read(path, response)
    }

    pub(crate) fn patch(&self, path: &str, body: &Value) -> Result<Value, Error> {
        let response = self
            .agent
            .patch(format!("{}{path}", self.base))
            .header("Authorization", &self.auth)
            .send_json(body);
        read(path, response)
    }

    pub(crate) fn delete(&self, path: &str) -> Result<Value, Error> {
        let response = self
            .agent
            .delete(format!("{}{path}", self.base))
            .header("Authorization", &self.auth)
            .call();
        read(path, response)
    }

    pub(crate) fn stream(&self, path: &str) -> Result<impl Read + Send + 'static, Error> {
        let response = self
            .stream_agent
            .get(format!("{}{path}", self.base))
            .header("Authorization", &self.auth)
            .call()
            .map_err(|e| format!("OpenCode's event stream failed: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("OpenCode's event stream answered {}", response.status()).into());
        }
        Ok(response.into_body().into_reader())
    }
}

pub(crate) fn read(
    path: &str,
    response: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<Value, Error> {
    let mut response = response.map_err(|e| format!("OpenCode request {path} failed: {e}"))?;
    let status = response.status();
    let text = response.body_mut().read_to_string().unwrap_or_default();
    let body = serde_json::from_str(&text).unwrap_or(Value::Null);
    if status.is_success() {
        return Ok(body);
    }
    let message = body["message"].as_str().map_or(text, String::from);
    Err(format!("OpenCode refused {path} ({status}): {message}").into())
}
