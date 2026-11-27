//! High-level client handle for an application running under Oxidation.

// Rust guideline compliant 2026-10-09

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::protocol::{Command, STATE_VAR, parse_command};

/// Active handoff connection to the Oxidation host process.
#[derive(Debug, Clone)]
pub struct ClientSession {
    state_path: PathBuf,
}

impl ClientSession {
    /// Connects to an active Oxidation host if `OXIDATION_STATE` is set in the environment.
    #[must_use]
    pub fn connect() -> Option<Self> {
        let state_path = std::env::var_os(STATE_VAR).map(PathBuf::from)?;
        Some(Self { state_path })
    }

    /// Connects only in debug builds (`cfg!(debug_assertions)`), returning `None` in release.
    #[must_use]
    pub fn connect_debug() -> Option<Self> {
        cfg!(debug_assertions).then(Self::connect).flatten()
    }

    /// Constructs a session pointing at an explicit state file path (useful in tests).
    #[must_use]
    pub fn with_state_path(state_path: PathBuf) -> Self {
        Self { state_path }
    }

    /// Path of the state handoff file assigned by the host.
    #[must_use]
    pub fn state_path(&self) -> &Path {
        &self.state_path
    }

    /// Reads and deserializes the JSON state saved by the previous run, then removes the file
    /// so a future cold launch starts fresh.
    #[must_use]
    pub fn take_state<T: DeserializeOwned>(&self) -> Option<T> {
        let text = std::fs::read_to_string(&self.state_path).ok()?;
        let _ = std::fs::remove_file(&self.state_path);
        serde_json::from_str(&text).ok()
    }

    /// Serializes `state` as JSON and writes it to the state handoff file.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if serialization or writing the file fails.
    pub fn save_state<T: Serialize>(&self, state: &T) -> std::io::Result<()> {
        let text = serde_json::to_string(state).map_err(std::io::Error::other)?;
        std::fs::write(&self.state_path, text)
    }

    /// Spawns a background thread reading Oxidation commands from standard input and delivers
    /// them over a standard `mpsc::Receiver`.
    #[must_use]
    pub fn listen(&self) -> mpsc::Receiver<Command> {
        let (tx, rx) = mpsc::channel();
        self.listen_with(move |command| tx.send(command).is_ok());
        rx
    }

    /// Spawns a background thread reading Oxidation commands from standard input and invokes
    /// `on_command` for each one until stdin closes or `on_command` returns `false`.
    pub fn listen_with<F>(&self, mut on_command: F)
    where
        F: FnMut(Command) -> bool + Send + 'static,
    {
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { return };
                let Some(command) = parse_command(&line) else {
                    continue;
                };
                if !on_command(command) {
                    return;
                }
            }
        });
    }

    /// Returns the inherited Unix socket used for file descriptor handover, opened at most once.
    #[cfg(unix)]
    #[must_use]
    pub fn fd_socket(&self) -> Option<std::os::fd::BorrowedFd<'static>> {
        unix_socket()
    }

    /// Sends `fds` to the host over the descriptor handover socket.
    ///
    /// Returns `Ok(false)` when no handover socket was provided by the host.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if sending the descriptors fails.
    #[cfg(unix)]
    pub fn hand_over_fds(&self, fds: &[std::os::fd::OwnedFd]) -> std::io::Result<bool> {
        let Some(socket) = self.fd_socket() else {
            return Ok(false);
        };
        crate::fds::send_fds(socket, fds)?;
        Ok(true)
    }

    /// Receives up to `expected` descriptors from the host, waiting up to `timeout` per message.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if reading from the socket fails.
    #[cfg(unix)]
    pub fn take_fds(
        &self,
        expected: usize,
        timeout: std::time::Duration,
    ) -> std::io::Result<Vec<std::os::fd::OwnedFd>> {
        if expected == 0 {
            return Ok(Vec::new());
        }
        let Some(socket) = self.fd_socket() else {
            return Ok(Vec::new());
        };
        let mut fds = Vec::with_capacity(expected);
        while fds.len() < expected {
            match crate::fds::receive_fds(socket, timeout)? {
                Some(more) => fds.extend(more),
                None => break,
            }
        }
        Ok(fds)
    }
}

#[cfg(unix)]
fn unix_socket() -> Option<std::os::fd::BorrowedFd<'static>> {
    use std::os::fd::{AsFd, FromRawFd as _, OwnedFd};
    use std::sync::OnceLock;

    static SOCKET: OnceLock<Option<OwnedFd>> = OnceLock::new();
    SOCKET
        .get_or_init(|| {
            let fd: i32 = std::env::var(crate::protocol::FDS_VAR).ok()?.parse().ok()?;
            // SAFETY: the host left this descriptor open exclusively for this child process, and
            // `OnceLock` guarantees it is wrapped into an `OwnedFd` at most once.
            Some(unsafe { OwnedFd::from_raw_fd(fd) })
        })
        .as_ref()
        .map(AsFd::as_fd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct SampleState {
        page: String,
        scroll: i32,
    }

    #[test]
    fn saves_and_takes_typed_state_once() {
        let dir =
            std::env::temp_dir().join(format!("oxidation-client-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("state.json");
        let session = ClientSession::with_state_path(path.clone());
        let expected = SampleState {
            page: "general".into(),
            scroll: 42,
        };
        session.save_state(&expected).expect("save state");
        assert!(path.is_file());
        assert_eq!(session.take_state::<SampleState>(), Some(expected));
        assert!(!path.exists());
        assert_eq!(session.take_state::<SampleState>(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
