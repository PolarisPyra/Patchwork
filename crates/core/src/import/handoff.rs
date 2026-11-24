//! A chat carried on in its agent's own CLI in a terminal, and brought back: the command that
//! resumes its session there, and the turns taken there read from the session's file into the
//! chat afterwards.

// Rust guideline compliant 2026-10-09

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Arc;

use super::{Session, claude, codex};
use crate::chats::{Message, MessageStatus};
use crate::provider::AgentKind;
use crate::{Core, Error, provider};

/// Where the time a chat went to its CLI is kept, so only turns taken since come back.
const HANDOFF_KEY: &str = "cli_handoff:";

/// The command that carries a chat on in its agent's CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliHandoff {
    /// The CLI to run.
    pub program: String,
    /// Its arguments, which resume the chat's session.
    pub args: Vec<String>,
    /// The folder the chat works in.
    pub cwd: String,
    /// The CLI's name, for the terminal's tab.
    pub label: &'static str,
}

/// Whether chats of `agent` can be carried on in its CLI and brought back.
#[must_use]
pub fn hands_off(agent: AgentKind) -> bool {
    matches!(agent, AgentKind::Claude | AgentKind::Codex)
}

impl Core {
    /// The command that resumes chat `chat_id`'s session in its agent's CLI. Its idle agent is
    /// stopped first, so the two never write to the session at once.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is unknown, is replying, has no session yet, runs an agent
    /// whose CLI cannot resume it, or the CLI is not installed.
    pub fn continue_in_cli(&self, chat_id: &str) -> Result<CliHandoff, Error> {
        let chat = self.chat(chat_id)?;
        if !hands_off(chat.agent) {
            return Err("Only Claude Code and Codex chats can be continued in their CLI.".into());
        }
        if self.runner.is_running(chat_id) {
            return Err("Wait for the reply to finish, or stop it, first.".into());
        }
        let session = chat
            .session_id
            .clone()
            .ok_or("Send a message first, so the chat has a session to resume.")?;
        let program = provider::agent_binary(chat.agent)?
            .to_string_lossy()
            .into_owned();
        let (args, label) = match chat.agent {
            AgentKind::Codex => (vec!["resume".to_owned(), session], "Codex"),
            _ => (vec!["--resume".to_owned(), session], "Claude Code"),
        };
        // After an undo or a fork, the next turn picks the session up at an earlier message, or
        // branches it; the CLI would carry on from its newest instead.
        if self.resume_point(chat_id).is_some() {
            return Err(
                "Send one message here first: the chat was undone or forked, and the CLI would \
                 resume it from before that."
                    .into(),
            );
        }
        let cwd = self.chat_root(chat_id)?;
        if crate::system::ssh::is_remote(&cwd) {
            return Err("Chats on an SSH host cannot be continued in a terminal here.".into());
        }
        self.runner.retire_idle(chat_id);
        self.store.set_state(
            &format!("{HANDOFF_KEY}{chat_id}"),
            &crate::system::now_ms().to_string(),
        )?;
        Ok(CliHandoff {
            program,
            args,
            cwd,
            label,
        })
    }

    /// Whether chat `chat_id` went to its CLI and has not been brought back.
    #[must_use]
    pub fn in_cli(&self, chat_id: &str) -> bool {
        self.store
            .state(&format!("{HANDOFF_KEY}{chat_id}"))
            .is_some()
    }

    /// Reads the turns taken in the CLI since chat `chat_id` went there into the chat, and
    /// returns how many messages came back. The agent's next turn resumes the same session, so
    /// it already knows them.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is unknown or its session's file cannot be found or read.
    pub fn bring_back_from_cli(&self, chat_id: &str) -> Result<usize, Error> {
        let key = format!("{HANDOFF_KEY}{chat_id}");
        let since: u64 = self
            .store
            .state(&key)
            .and_then(|s| s.parse().ok())
            .unwrap_or_default();
        let chat = self.chat(chat_id)?;
        let id = chat
            .session_id
            .clone()
            .ok_or("The chat has no session to read.")?;
        let session = read_session(chat.agent, &id)
            .ok_or("The CLI's session file could not be found or read.")?;
        let newest = chat.messages.last().map_or(0, |m| m.created_at);
        let since = since.max(newest.saturating_add(1));
        let back: Vec<Arc<Message>> = session
            .messages
            .into_iter()
            .filter(|m| m.created_at >= since && !m.text.trim().is_empty())
            .map(|m| {
                Arc::new(Message {
                    created_at: m.created_at,
                    ..Message::new(m.role, m.text, MessageStatus::Complete)
                })
            })
            .collect();
        let count = back.len();
        if count > 0 {
            self.store
                .update_chat(chat_id, |c| c.messages.extend(back))?;
        }
        self.store.clear_state(&key)?;
        Ok(count)
    }

    /// Brings chat `chat_id` back from its CLI before a message is sent to it, so the agent is
    /// not started on a session the CLI may still hold. A session that cannot be read is left
    /// as it is, and the chat counts as back.
    pub(crate) fn back_before_sending(&self, chat_id: &str) {
        if !self.in_cli(chat_id) {
            return;
        }
        if let Err(e) = self.bring_back_from_cli(chat_id) {
            log::warn!(chat:% = chat_id, error:% = e; "could not bring chat {{chat}} back from the CLI: {{error}}");
            if let Err(e) = self.store.clear_state(&format!("{HANDOFF_KEY}{chat_id}")) {
                log::warn!(error:% = e; "could not mark the chat as back: {{error}}");
            }
        }
    }
}

/// The session `id` of `agent` as its CLI saved it.
fn read_session(agent: AgentKind, id: &str) -> Option<Session> {
    let path = session_file(agent, id)?;
    match agent {
        AgentKind::Codex => codex::parse(BufReader::new(File::open(path).ok()?)),
        _ => claude::parse(&std::fs::read_to_string(path).ok()?, false),
    }
}

/// Where the CLI saved session `id`: Claude Code's `projects/<folder>/<id>.jsonl`, or Codex's
/// `sessions/…/rollout-…-<id>.jsonl`.
fn session_file(agent: AgentKind, id: &str) -> Option<PathBuf> {
    if agent == AgentKind::Codex {
        let dir = provider::codex::home()?.join("sessions");
        let suffix = format!("{id}.jsonl");
        return provider::codex::rollout_files(&dir)
            .into_iter()
            .map(|(_, path)| path)
            .find(|path| {
                path.file_name()
                    .is_some_and(|n| n.to_string_lossy().ends_with(&suffix))
            });
    }
    let projects = provider::claude::home()?.join("projects");
    let name = format!("{id}.jsonl");
    std::fs::read_dir(projects)
        .ok()?
        .flatten()
        .map(|dir| dir.path().join(&name))
        .find(|path| path.is_file())
}
