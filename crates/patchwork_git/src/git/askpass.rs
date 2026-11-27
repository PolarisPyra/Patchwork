//! Sign-in questions from git and ssh, asked in the app.
//!
//! A git command the user started names Patchwork's own program as the one git and ssh ask
//! through. Started that way it passes the question to the running app over the relay, and
//! prints the answer for git to read. A command nobody watches, such as the fetch on a timer,
//! names none and fails instead of asking.

// Rust guideline compliant 2026-10-09

use std::cell::Cell;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::process::Command;
use std::sync::{OnceLock, mpsc};

use serde_json::{Value, json};

use super::run::NETWORK_LIMIT;

/// Names the relay to the program git starts to ask a question, as `{"port", "token"}`. Set, it
/// makes Patchwork's program that helper instead of the app.
pub const ASKPASS_ENV: &str = "PATCHWORK_ASKPASS";

/// What kind of answer a question takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    /// Text that may be shown, such as a user name.
    Text,
    /// A password, passphrase or token, never shown.
    Secret,
    /// Yes or no, as ssh asks before trusting a host it has not seen.
    Confirm,
}

/// One question git or ssh waits on.
#[derive(Debug)]
pub struct CredentialRequest {
    /// The question, as git or ssh worded it.
    pub prompt: String,
    pub kind: CredentialKind,
    answer: mpsc::Sender<Option<String>>,
}

impl CredentialRequest {
    /// Answers with `value`, or with None declines, which fails the command that asked.
    pub fn answer(self, value: Option<String>) {
        // The command may have been stopped meanwhile, with nothing left to read the answer.
        let _ = self.answer.send(value);
    }
}

/// Shows a question to the user.
type Prompter = Box<dyn Fn(CredentialRequest) + Send + Sync>;
static PROMPTER: OnceLock<Prompter> = OnceLock::new();

/// Provides the local relay endpoint JSON (`{"port", "token"}`) when askpass runs.
type RelayProvider = fn() -> Option<Value>;
static RELAY_PROVIDER: OnceLock<RelayProvider> = OnceLock::new();

/// Sets what shows git's sign-in questions; called once as the core opens.
pub fn set_credential_prompter(prompter: Prompter) {
    let _ = PROMPTER.set(prompter);
}

/// Sets what provides the local relay endpoint for askpass helpers.
pub fn set_relay_endpoint_provider(provider: RelayProvider) {
    let _ = RELAY_PROVIDER.set(provider);
}

thread_local! {
    /// Whether the git commands this chat runs are ones the user started and waits on.
    static ATTENDED: Cell<bool> = const { Cell::new(false) };
}

/// Marks this chat's git commands as watched until it drops.
pub(super) struct Attended(bool);

impl Attended {
    pub(super) fn start() -> Self {
        Self(ATTENDED.replace(true))
    }
}

impl Drop for Attended {
    fn drop(&mut self) {
        ATTENDED.set(self.0);
    }
}

/// Whether a git command in `root` started now may ask the user: one the user started, in a
/// folder on this machine, with the app there to ask.
pub(super) fn may_ask(root: &str) -> bool {
    ATTENDED.get() && PROMPTER.get().is_some() && !patchwork_platform::ssh::is_remote(root)
}

/// Names Patchwork's program as what git and ssh ask through, for a command that may ask.
pub(super) fn offer(cmd: &mut Command, root: &str) {
    if !may_ask(root) {
        return;
    }
    let (Some(relay), Ok(program)) = (
        RELAY_PROVIDER.get().and_then(|provider| provider()),
        std::env::current_exe(),
    ) else {
        return;
    };
    cmd.env(ASKPASS_ENV, relay.to_string())
        .env("GIT_ASKPASS", &program)
        .env("SSH_ASKPASS", &program)
        // ssh asks through the program even with no display to show a window on.
        .env("SSH_ASKPASS_REQUIRE", "force");
}

/// The kind of answer `prompt` takes, read from how git and ssh word their questions.
fn kind_of(prompt: &str) -> CredentialKind {
    let asked = prompt.to_lowercase();
    if asked.contains("(yes/no") {
        CredentialKind::Confirm
    } else if asked.starts_with("username") {
        CredentialKind::Text
    } else {
        CredentialKind::Secret
    }
}

/// Asks the user `prompt` for the helper that sent it, and waits for the answer. None when the
/// user declines, the app cannot ask, or the command that asked has run out of time.
#[must_use]
pub fn answer_askpass(prompt: &str) -> Option<String> {
    let (answer, answered) = mpsc::channel();
    PROMPTER.get()?(CredentialRequest {
        prompt: prompt.trim().to_string(),
        kind: kind_of(prompt),
        answer,
    });
    answered.recv_timeout(NETWORK_LIMIT).ok().flatten()
}

/// Runs Patchwork's program as the helper git or ssh asks through, and returns its exit code.
///
/// The question is its one argument. The answer goes to stdout, where git reads it; with no
/// answer it exits with an error, and the command that asked fails.
#[expect(
    clippy::print_stdout,
    reason = "git reads the answer from this program's stdout"
)]
#[must_use]
pub fn run_askpass() -> i32 {
    let asked = || -> Option<String> {
        let relay: Value = serde_json::from_str(&std::env::var(ASKPASS_ENV).ok()?).ok()?;
        let port = u16::try_from(relay["port"].as_u64()?).ok()?;
        let prompt = std::env::args().nth(1).unwrap_or_default();
        let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
        writeln!(
            stream,
            "{}",
            json!({"token": relay["token"], "askpass": prompt})
        )
        .ok()?;
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).ok()?;
        let said: Value = serde_json::from_str(&line).ok()?;
        said["answer"].as_str().map(str::to_string)
    };
    match asked() {
        Some(answer) => {
            println!("{answer}");
            0
        }
        None => 1,
    }
}
