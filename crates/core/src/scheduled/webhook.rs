//! Tasks started by a webhook: a `POST` to `http://127.0.0.1:<port>/hooks/<token>` on this
//! computer starts the task whose secret token it names, with the request's body after its
//! prompt. The listener only takes connections from this computer; a service elsewhere reaches
//! it through a tunnel the person sets up.

// Rust guideline compliant 2026-10-09

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use super::types::Trigger;
use crate::Core;

/// The port the listener asks for first, so addresses stay the same between runs.
const DEFAULT_PORT: u16 = 47615;

/// The port the listener is on, in the app state.
const PORT_KEY: &str = "scheduled_webhook_port";

/// The path every webhook's address starts with.
const PATH: &str = "/hooks/";

/// The most of a request read: its headers and body together.
const REQUEST_LIMIT: usize = 64 * 1024;

/// The most of the body the run is told, in characters.
const BODY_LIMIT: usize = 4000;

/// How long a caller has to send its request.
const READ_TIMEOUT: Duration = Duration::from_secs(5);

impl Core {
    /// Starts listening for webhooks on this computer, on a thread of its own, for the life of
    /// the app. The port it gets is kept, so a task's address stays the same next time.
    pub fn listen_for_webhooks(self: &Arc<Self>) {
        let wanted = self
            .store
            .state(PORT_KEY)
            .and_then(|p| p.parse().ok())
            .unwrap_or(DEFAULT_PORT);
        let listener = [wanted, DEFAULT_PORT, 0]
            .into_iter()
            .find_map(|port| TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).ok());
        let Some(listener) = listener else {
            log::warn!("webhooks will not start tasks: no port on 127.0.0.1 could be opened");
            return;
        };
        let port = listener.local_addr().map_or(wanted, |a| a.port());
        if let Err(e) = self.store.set_state(PORT_KEY, &port.to_string()) {
            log::warn!(error:% = e; "could not keep the webhook port: {{error}}");
        }
        let core = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("webhooks".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let core = Arc::clone(&core);
                    let handled = std::thread::Builder::new()
                        .name("webhook".into())
                        .spawn(move || core.answer_webhook(stream));
                    if let Err(e) = handled {
                        log::warn!(error:% = e; "could not answer a webhook: {{error}}");
                    }
                }
            });
        if let Err(e) = spawned {
            log::warn!(error:% = e; "webhooks will not start tasks: {{error}}");
        }
    }

    /// The address that starts the task with webhook `token`, once the listener has a port.
    #[must_use]
    pub fn webhook_url(&self, token: &str) -> Option<String> {
        let port = self.store.state(PORT_KEY)?;
        Some(format!("http://127.0.0.1:{port}{PATH}{token}"))
    }

    /// Reads one request from `stream`, starts the task it names, and answers.
    fn answer_webhook(self: &Arc<Self>, mut stream: TcpStream) {
        let (status, text) = match read_request(&mut stream) {
            Some((method, path, body)) => self.call_webhook(&method, &path, &body),
            None => ("400 Bad Request", "The request could not be read"),
        };
        let reply = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{text}",
            text.len()
        );
        if let Err(e) = stream.write_all(reply.as_bytes()) {
            log::debug!(error:% = e; "could not answer a webhook: {{error}}");
        }
    }

    /// Starts the task a `method` request to `path` names, and the status to answer with.
    fn call_webhook(
        self: &Arc<Self>,
        method: &str,
        path: &str,
        body: &str,
    ) -> (&'static str, &'static str) {
        let Some(token) = path
            .strip_prefix(PATH)
            .map(|t| t.split('?').next().unwrap_or(t))
        else {
            return ("404 Not Found", "No such webhook");
        };
        let task = self.store.scheduled_tasks().into_iter().find(|t| {
            matches!(&t.trigger, Trigger::Webhook { token: own } if !own.is_empty() && own == token)
        });
        let Some(task) = task else {
            return ("404 Not Found", "No such webhook");
        };
        if method != "POST" {
            return ("405 Method Not Allowed", "Send a POST request");
        }
        if !task.enabled {
            return ("409 Conflict", "The task is switched off");
        }
        let body = cut(body.trim(), BODY_LIMIT);
        let detail = if body.is_empty() {
            "This run was started by the task's webhook.".to_owned()
        } else {
            format!("This run was started by the task's webhook. It sent:\n\n```\n{body}\n```")
        };
        match self.start_on_event(&task, &detail) {
            Ok(_) => {
                log::info!(task:% = task.name; "scheduled task started by its webhook: {{task}}");
                ("202 Accepted", "Started")
            }
            Err(e) => {
                log::warn!(
                    task:% = task.name, error:% = e;
                    "scheduled task {{task}} did not start from its webhook: {{error}}"
                );
                ("500 Internal Server Error", "The task did not start")
            }
        }
    }
}

/// The method, path and body of the request on `stream`, or None when it is not a whole
/// HTTP request within the limits.
fn read_request(stream: &mut TcpStream) -> Option<(String, String, String)> {
    stream.set_read_timeout(Some(READ_TIMEOUT)).ok()?;
    let mut data = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        data.extend_from_slice(&chunk[..n]);
        if let Some(at) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
        if data.len() > REQUEST_LIMIT {
            return None;
        }
    };
    let head = String::from_utf8_lossy(&data[..header_end]).into_owned();
    let mut lines = head.lines();
    let mut first = lines.next()?.split_whitespace();
    let (method, path) = (first.next()?.to_owned(), first.next()?.to_owned());
    let length: usize = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse().ok())
        .unwrap_or(0);
    let length = length.min(REQUEST_LIMIT);
    while data.len() < header_end + length {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&chunk[..n]);
    }
    let end = data.len().min(header_end + length);
    let body = String::from_utf8_lossy(&data[header_end..end]).into_owned();
    Some((method, path, body))
}

/// `text` cut to its first `limit` characters, with a note when it was cut.
fn cut(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((at, _)) => format!("{}\n… (cut short)", &text[..at]),
        None => text.to_owned(),
    }
}
