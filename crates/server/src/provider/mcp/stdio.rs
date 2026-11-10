//! A server run as a command, speaking JSON-RPC over its stdin and stdout.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{Value, json};

use super::{announce, Answer, Notices, STDERR_TAIL, Waiters};

pub(super) struct StdioServer {
    child: Mutex<Child>,
    stdin: Arc<Mutex<ChildStdin>>,
    pub(super) waiters: Waiters,
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
        let stdin = Arc::new(Mutex::new(child.stdin.take().ok_or("No stdin")?));
        let stdout = child.stdout.take().ok_or("No stdout")?;
        let stderr_pipe = child.stderr.take().ok_or("No stderr")?;
        let waiters: Waiters = Arc::default();
        let stderr: Arc<Mutex<String>> = Arc::default();
        {
            let stderr = Arc::clone(&stderr);
            std::thread::spawn(move || {
                for line in BufReader::new(stderr_pipe).lines().map_while(Result::ok) {
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
            let (waiters, stdin) = (Arc::clone(&waiters), Arc::clone(&stdin));
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
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
                            let mut stdin = stdin.lock().unwrap_or_else(PoisonError::into_inner);
                            let _ = writeln!(stdin, "{reply}");
                            let _ = stdin.flush();
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
                // The server exited: wake everyone still waiting.
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
        let mut stdin = self.stdin.lock().unwrap_or_else(PoisonError::into_inner);
        writeln!(stdin, "{message}")
            .and_then(|()| stdin.flush())
            .map_err(|e| format!("The MCP server closed: {e}").into())
    }

    pub(super) fn send_request(&self, id: u64, message: &Value) -> Result<Answer, Error> {
        let (tx, rx) = mpsc::channel();
        self.waiters
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, tx);
        if let Err(e) = self.send(message) {
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
