//! Official Antigravity ACP sign-in (`agy acp --login`) and sign-out (`logout`).

// Rust guideline compliant 2026-11-22

use std::io::{BufReader, Read, Write};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use patchwork_platform::processes::running::{SIGKILL, signal_process_group};
use patchwork_platform::{Error, Locked};
use serde_json::{Value, json};

use crate::cli::agy_command;
use crate::probe::{client_info, invalidate_probe_cache};

const LOGIN_URL_TIMEOUT: Duration = Duration::from_secs(30);
const LOGIN_FINISH_TIMEOUT: Duration = Duration::from_secs(45);
const LOGOUT_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_AUTH_URL_LEN: usize = 16_384;
const MAX_CODE_LEN: usize = 4_096;
const CODE_REDIRECT_URI: &str = "https://antigravity.google/oauth-callback";

/// An active `agy acp --login` flow waiting for the user to paste their code or callback URL.
#[derive(Debug)]
pub struct LoginSession {
    url: String,
    expected_state: Option<String>,
    state: Arc<Mutex<LoginProcess>>,
}

#[derive(Debug)]
struct LoginProcess {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stderr: Arc<Mutex<String>>,
}

impl LoginProcess {
    fn kill(&mut self) {
        self.stdin.take();
        if let Some(mut child) = self.child.take() {
            signal_process_group(child.id(), SIGKILL);
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for LoginSession {
    fn drop(&mut self) {
        self.state.locked().kill();
    }
}

impl LoginSession {
    /// The Google sign-in URL to open in the browser.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Sends the user's pasted code or callback URL to `agy acp --login` and waits for completion.
    ///
    /// # Errors
    ///
    /// Returns an error when the input is empty or invalid, the sign-in process already ended,
    /// or `agy acp --login` rejects the code or times out.
    pub fn submit(&self, response: &str) -> Result<(), Error> {
        let answer = validate_login_response(response, self.expected_state.as_deref())?;
        let mut proc = self.state.locked();
        let mut stdin = proc
            .stdin
            .take()
            .ok_or("Antigravity sign-in is no longer waiting for a code")?;
        writeln!(stdin, "{answer}")
            .and_then(|()| stdin.flush())
            .map_err(|e| format!("Could not send the sign-in code to Antigravity: {e}"))?;
        drop(stdin);
        let mut child = proc
            .child
            .take()
            .ok_or("Antigravity sign-in already finished")?;
        let stderr = Arc::clone(&proc.stderr);
        drop(proc);

        let deadline = Instant::now() + LOGIN_FINISH_TIMEOUT;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Ok(None) => {
                    signal_process_group(child.id(), SIGKILL);
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("Antigravity sign-in timed out waiting for Google".into());
                }
                Err(e) => {
                    return Err(format!("Could not wait for Antigravity sign-in: {e}").into());
                }
            }
        };
        invalidate_probe_cache();
        if status.success() {
            return Ok(());
        }
        let err = stderr.locked().trim().to_string();
        if err.is_empty() {
            Err("Google sign-in failed. Check the code and start sign-in again.".into())
        } else {
            Err(format!("Google sign-in failed: {err}").into())
        }
    }

    /// Cancels the pending sign-in and stops `agy acp --login`.
    pub fn cancel(&self) {
        self.state.locked().kill();
    }
}

/// Starts `agy acp --login` and waits for it to emit the Google OAuth URL.
///
/// # Errors
///
/// Returns an error when `agy` is not installed, fails to start, or does not print a valid
/// Google sign-in link before timing out.
pub fn start_login() -> Result<LoginSession, Error> {
    let mut cmd = agy_command()?;
    cmd.args(["acp", "--login"])
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Could not start Antigravity sign-in: {e}"))?;
    let stdin = child
        .stdin
        .take()
        .ok_or("Antigravity sign-in stdin is unavailable")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("Antigravity sign-in stdout is unavailable")?;
    let mut stderr_pipe = child
        .stderr
        .take()
        .ok_or("Antigravity sign-in stderr is unavailable")?;

    let stderr = Arc::new(Mutex::new(String::new()));
    let stderr_sink = Arc::clone(&stderr);
    std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr_pipe.read_to_string(&mut buf);
        *stderr_sink.locked() = buf;
    });

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in patchwork_platform::lossy_lines(BufReader::new(stdout)) {
            let trimmed = line.trim();
            if trimmed.starts_with("https://accounts.google.com/") {
                let _ = tx.send(trimmed.to_string());
                return;
            }
        }
    });

    let Ok(url) = rx.recv_timeout(LOGIN_URL_TIMEOUT) else {
        signal_process_group(child.id(), SIGKILL);
        let _ = child.kill();
        let _ = child.wait();
        let err = stderr.locked().trim().to_string();
        return Err(if err.is_empty() {
            "Antigravity did not provide a Google sign-in link. Check your network and try again."
                .into()
        } else {
            format!("Antigravity sign-in failed: {err}").into()
        });
    };

    let expected_state = validate_auth_url(&url).inspect_err(|_| {
        signal_process_group(child.id(), SIGKILL);
        let _ = child.kill();
        let _ = child.wait();
    })?;

    Ok(LoginSession {
        url,
        expected_state,
        state: Arc::new(Mutex::new(LoginProcess {
            child: Some(child),
            stdin: Some(stdin),
            stderr,
        })),
    })
}

/// Signs out of Antigravity via the official ACP `logout` RPC.
///
/// # Errors
///
/// Returns an error when `agy` is unavailable, does not advertise logout, or rejects sign-out.
pub fn sign_out() -> Result<(), Error> {
    let mut cmd = agy_command()?;
    cmd.args(["acp", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Could not start Antigravity ACP server: {e}"))?;
    let pid = child.id();
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        signal_process_group(pid, SIGKILL);
        let _ = child.kill();
        let _ = child.wait();
        return Err("Antigravity ACP stdio is unavailable".into());
    };
    let init = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": 1,
            "clientInfo": client_info(),
            "clientCapabilities": {},
        },
    });
    let logout = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "logout",
        "params": {},
    });
    let wrote = writeln!(stdin, "{init}")
        .and_then(|()| writeln!(stdin, "{logout}"))
        .and_then(|()| stdin.flush());
    if let Err(e) = wrote {
        signal_process_group(pid, SIGKILL);
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("Could not send sign-out request to Antigravity: {e}").into());
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in patchwork_platform::lossy_lines(BufReader::new(stdout)) {
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if v["id"] == 2 {
                let _ = tx.send(v);
                return;
            }
        }
    });
    let response = rx.recv_timeout(LOGOUT_TIMEOUT).ok();
    drop(stdin);
    signal_process_group(pid, SIGKILL);
    let _ = child.kill();
    let _ = child.wait();
    invalidate_probe_cache();

    let Some(response) = response else {
        return Err("Antigravity sign-out timed out".into());
    };
    if let Some(message) = response["error"]["message"].as_str() {
        return Err(format!("Antigravity sign-out failed: {message}").into());
    }
    Ok(())
}

const AUTH_URL_PREFIX: &str = "https://accounts.google.com/o/oauth2/auth?";

fn validate_auth_url(url: &str) -> Result<Option<String>, Error> {
    if url.len() > MAX_AUTH_URL_LEN || url.chars().any(char::is_whitespace) {
        return Err("Antigravity returned an invalid Google sign-in URL".into());
    }
    let Some(query) = url.strip_prefix(AUTH_URL_PREFIX) else {
        return Err("Antigravity returned an unexpected sign-in URL".into());
    };
    let query = query.split('#').next().unwrap_or(query);
    let state = query_param(query, "state");
    let redirect_uri = query_param(query, "redirect_uri");
    let response_type = query_param(query, "response_type");
    if response_type.as_deref() != Some("code")
        || redirect_uri.as_deref() != Some(CODE_REDIRECT_URI)
    {
        return Err("Antigravity returned an unexpected OAuth sign-in URL".into());
    }
    Ok(state.filter(|s| !s.is_empty()))
}

fn validate_login_response(raw: &str, expected_state: Option<&str>) -> Result<String, Error> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Paste the authorization code or callback URL from Google".into());
    }
    if trimmed.len() > MAX_CODE_LEN || trimmed.contains(['\r', '\n']) {
        return Err("The sign-in code must be a single line".into());
    }
    if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
        let Some((_, query)) = trimmed.split_once('?') else {
            return Err("That callback URL does not contain a sign-in code".into());
        };
        let query = query.split('#').next().unwrap_or(query);
        let code = query_param(query, "code");
        let state = query_param(query, "state");
        if code.as_deref().is_none_or(str::is_empty) {
            return Err("That callback URL does not contain a sign-in code".into());
        }
        if let Some(expected) = expected_state
            && state.as_deref() != Some(expected)
        {
            return Err("That callback URL belongs to a different sign-in attempt".into());
        }
    }
    Ok(trimmed.to_string())
}

fn query_param(query: &str, name: &str) -> Option<String> {
    for pair in query.split('&') {
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        if k == name {
            return Some(percent_decode(v));
        }
    }
    None
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_nibble(bytes[i + 1]), hex_nibble(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
