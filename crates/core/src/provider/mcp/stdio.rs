//! A server run as a command, speaking JSON-RPC over its stdin and stdout.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::collections::BTreeMap;
use std::io::{BufReader, Write};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{Value, json};

use super::{announce, Answer, Notices, STDERR_TAIL, Waiters};

pub(super) struct StdioServer {
    child: Mutex<Child>,
    /// Feeds the thread that writes to the server's stdin, so a server that has stopped reading
    /// holds up neither a caller nor the thread reading its answers.
    stdin: mpsc::Sender<String>,
    pub(super) waiters: Waiters,
    /// Set once the server's output has ended, after which no request is answered.
    closed: Arc<AtomicBool>,
    pub(super) stderr: Arc<Mutex<String>>,
}

impl StdioServer {
    pub(super) fn spawn(
        command: &str,
        args: &[String],
        env: &BTreeMap<String, String>,
        notices: Notices,
    ) -> Result<Self, Error> {
        let mut cmd = crate::provider::user_command(command.trim());
        cmd.args(args)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Its own process group, so stopping it also stops what it started (npx's node).
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Could not start {command}: {e}"))?;
        let mut pipe = child.stdin.take().ok_or("No stdin")?;
        let (stdin, lines) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            for line in lines {
                if writeln!(pipe, "{line}")
                    .and_then(|()| pipe.flush())
                    .is_err()
                {
                    break;
                }
            }
        });
        let stdout = child.stdout.take().ok_or("No stdout")?;
        let stderr_pipe = child.stderr.take().ok_or("No stderr")?;
        let waiters: Waiters = Arc::default();
        let closed = Arc::new(AtomicBool::new(false));
        let stderr: Arc<Mutex<String>> = Arc::default();
        {
            let stderr = Arc::clone(&stderr);
            std::thread::spawn(move || {
                for line in crate::system::lossy_lines(BufReader::new(stderr_pipe)) {
                    let mut tail = stderr.lock().unwrap_or_else(PoisonError::into_inner);
                    tail.push_str(&line);
                    tail.push('\n');
                    if tail.len() > STDERR_TAIL {
                        let cut = tail.len() - STDERR_TAIL;
                        let cut = (cut..tail.len())
                            .find(|&i| tail.is_char_boundary(i))
                            .unwrap_or(0);
                        tail.drain(..cut);
                    }
                }
            })
        };
        {
            let (waiters, stdin) = (Arc::clone(&waiters), stdin.clone());
            let closed = Arc::clone(&closed);
            std::thread::spawn(move || {
                for line in crate::system::lossy_lines(BufReader::new(stdout)) {
                    let Ok(v) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    match (v.get("id"), v.get("method")) {
                        // A request from the server: answer pings, refuse the rest.
                        (Some(id), Some(method)) => {
                            let reply = if method == "ping" {
                                json!({"jsonrpc": "2.0", "id": id, "result": {}})
                            } else {
                                json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "Not supported"}})
                            };
                            let _ = stdin.send(reply.to_string());
                        }
                        (Some(id), None) => {
                            let waiter = id.as_u64().and_then(|id| {
                                waiters
                                    .lock()
                                    .unwrap_or_else(PoisonError::into_inner)
                                    .remove(&id)
                            });
                            if let Some(waiter) = waiter {
                                let _ = waiter.send(Ok(v));
                            }
                        }
                        (None, Some(_)) => announce(&notices, &v),
                        _ => {}
                    }
                }
                // The server exited: wake everyone still waiting, and turn down who asks next.
                closed.store(true, Ordering::SeqCst);
                waiters
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clear();
            })
        };
        Ok(StdioServer {
            child: Mutex::new(child),
            stdin,
            waiters,
            closed,
            stderr,
        })
    }

    pub(super) fn alive(&self) -> bool {
        matches!(
            self.child
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .try_wait(),
            Ok(None)
        )
    }

    pub(super) fn send(&self, message: &Value) -> Result<(), Error> {
        self.stdin
            .send(message.to_string())
            .map_err(|_closed| "The MCP server closed".into())
    }

    pub(super) fn send_request(&self, id: u64, message: &Value) -> Result<Answer, Error> {
        let (tx, rx) = mpsc::channel();
        self.waiters
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, tx);
        // Checked after the waiter is in, so one the reader's last sweep missed is not left
        // waiting for an answer that cannot come.
        let sent = if self.closed.load(Ordering::SeqCst) {
            Err("The MCP server closed".into())
        } else {
            self.send(message)
        };
        if let Err(e) = sent {
            self.waiters
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&id);
            return Err(e);
        }
        Ok(rx)
    }
}

impl Drop for StdioServer {
    fn drop(&mut self) {
        crate::provider::stop(self.child.get_mut().unwrap_or_else(PoisonError::into_inner));
    }
}
