//! Secrets an agent asks the user for. The agent calls [`TOOL`], a private card in the chat takes
//! the value, and the agent is told only the path of an owner-only file holding it, which it reads
//! inside the command that needs it. The value never enters the thread, the logs or the model's
//! context. Each file is deleted a day after it was written.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;

use crate::Server;

/// Where a secret request stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "secrets.ts")]
#[serde(rename_all = "camelCase")]
pub enum SecretStatus {
    Pending,
    Saved,
    Declined,
    /// The turn ended, or the agent stopped waiting, before the user answered.
    Cancelled,
    TimedOut,
}

/// What makes an approval a request for a secret, whose title is the card's label and whose
/// detail says what it is for. It never holds the value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "secrets.ts")]
#[serde(rename_all = "camelCase")]
pub struct Secret {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    pub status: SecretStatus,
}

/// The tool every agent asks for a secret with.
pub const TOOL: &str = "request_secret";

/// What the agent is told the tool does.
pub const DESCRIPTION: &str = "Ask the user to enter a secret, such as an API key, token or password, in a private card in the chat, and wait for the answer. You never see the value. Once saved, the result names an owner-only file holding it: read it inside the command that needs it, such as TOKEN=\"$(cat <file>)\" some-cli ..., and never print, echo or copy it. The file is deleted after 24 hours. When the user asks how to give you a secret, call this rather than explaining.";

/// The folder in Patchwork's state folder the files are written to.
const FOLDER: &str = "secrets";
/// How long a file is kept.
const KEEP: Duration = Duration::from_secs(24 * 60 * 60);
/// How often old files are looked for.
const SWEEP_EVERY: Duration = Duration::from_secs(60 * 60);
const DEFAULT_WAIT: Duration = Duration::from_secs(10 * 60);
const MAX_WAIT: Duration = Duration::from_secs(60 * 60);

/// The tool's arguments, as JSON Schema.
pub fn input_schema() -> Value {
    json!({"type": "object", "properties": {
        "label": {"type": "string", "description": "The card's title, such as \"GitHub token\"."},
        "reason": {"type": "string", "description": "What it is for and where to get it."},
        "placeholder": {"type": "string", "description": "Hint shown in the empty field."},
        "timeoutMs": {"type": "integer", "minimum": 1000, "maximum": 3_600_000, "description": "How long to wait for the user. Default 10 minutes."}
    }, "required": ["label", "reason"]})
}

/// What the agent asked for, read from the tool's arguments.
pub struct SecretAsk {
    pub label: String,
    pub reason: String,
    pub placeholder: Option<String>,
    /// How long to wait for the user.
    pub wait: Duration,
}

impl SecretAsk {
    pub fn parse(args: &Value) -> Result<Self, String> {
        let text = |key: &str| {
            args[key]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        let label = text("label").ok_or("Give the secret a label")?;
        let wait = args["timeoutMs"]
            .as_u64()
            .map_or(DEFAULT_WAIT, Duration::from_millis)
            .clamp(Duration::from_secs(1), MAX_WAIT);
        Ok(Self {
            label,
            reason: text("reason").unwrap_or_default(),
            placeholder: text("placeholder"),
            wait,
        })
    }
}

/// The tool's result for the agent: where the secret is, or why there is none.
pub fn tool_result(status: SecretStatus, file: Option<&Path>) -> String {
    match (status, file) {
        (SecretStatus::Saved, Some(file)) => json!({
            "status": "saved",
            "file": file,
            "note": "Read the file inside the command that needs it and never print it. It is deleted after 24 hours.",
        }),
        (status, _) => json!({
            "status": status,
            "note": "No secret was saved. Ask again if it is still needed.",
        }),
    }
    .to_string()
}

/// The requests waiting for the user, by id, each with its thread and what it is answered
/// through: the file's path, or None when declined.
#[derive(Default)]
pub struct Waiters(Mutex<HashMap<String, Waiter>>);

/// A request's thread, and what it is answered through.
type Waiter = (String, Sender<Option<PathBuf>>);

impl Waiters {
    /// Waits on `id` in `thread_id`, until it is answered.
    pub(crate) fn add(&self, id: &str, thread_id: &str) -> Receiver<Option<PathBuf>> {
        let (tx, rx) = mpsc::channel();
        self.lock()
            .insert(id.to_string(), (thread_id.to_string(), tx));
        rx
    }

    /// Stops waiting on `id`; false if it was answered first.
    pub(crate) fn remove(&self, id: &str) -> bool {
        self.lock().remove(id).is_some()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Waiter>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Server {
    /// Answers the agent's request `request_id` in the thread: `Some` saves the value to a file
    /// only the user can read and tells the agent its path, and None declines.
    pub fn answer_secret(
        &self,
        thread_id: &str,
        request_id: &str,
        value: Option<String>,
    ) -> Result<(), String> {
        let gone = || "This request is no longer waiting for an answer".to_string();
        let mut waiters = self.secrets.lock();
        if !waiters
            .get(request_id)
            .is_some_and(|(thread, _)| thread == thread_id)
        {
            return Err(gone());
        }
        let file = match value {
            Some(value) if value.is_empty() => return Err("Enter the secret first".into()),
            Some(value) => Some(store(&value)?),
            None => None,
        };
        let (_, waiter) = waiters.remove(request_id).ok_or_else(gone)?;
        drop(waiters);
        if waiter.send(file.clone()).is_err() {
            if let Some(file) = file {
                let _ = std::fs::remove_file(file);
            }
            return Err(gone());
        }
        Ok(())
    }
}

/// Writes `value` to a new file in a folder only the user can open, readable only by them.
fn store(value: &str) -> Result<PathBuf, String> {
    use std::io::Write;
    let failed = |e: std::io::Error| format!("Could not save the secret: {e}");
    let dir = crate::system::dirs().state.join(FOLDER);
    std::fs::create_dir_all(&dir).map_err(failed)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(failed)?;
    }
    let path = dir.join(crate::system::new_id());
    let partial = path.with_extension("partial");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let written = options.open(&partial).and_then(|mut file| {
        file.write_all(value.as_bytes())?;
        file.sync_all()
    });
    if let Err(e) = written.and_then(|()| std::fs::rename(&partial, &path)) {
        let _ = std::fs::remove_file(&partial);
        return Err(failed(e));
    }
    Ok(path)
}

/// Deletes the files written more than a day ago, now and then every hour while Patchwork runs.
pub fn keep_swept() {
    std::thread::spawn(|| {
        loop {
            sweep();
            std::thread::sleep(SWEEP_EVERY);
        }
    });
}

fn sweep() {
    let Ok(entries) = std::fs::read_dir(crate::system::dirs().state.join(FOLDER)) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age > KEEP);
        if old && let Err(e) = std::fs::remove_file(entry.path()) {
            log::warn!("could not delete an expired secret file: {e}");
        }
    }
}
