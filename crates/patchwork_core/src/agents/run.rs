//! Running an agent: the chat each run gets, what its agent is told, what it may not write, and
//! the tokens its runs used.

// Rust guideline compliant 2026-10-09

use std::fmt::Write as _;
use std::sync::Arc;

use super::types::{Agent, AgentRun, AgentUsage};
use crate::Core;
use crate::Error;
use crate::permissions::PermissionKind;
use crate::chats::{Attachment, NewChat, Chat, TokenUsage};

impl Core {
    /// Starts a run of agent `id` on `prompt`, in a new chat, and returns the chat.
    ///
    /// The run works in `project_id` when that is one of the agent's projects, and in a scratch
    /// folder of its own when it is None.
    ///
    /// # Errors
    ///
    /// Returns an error when the agent does not exist, the prompt is empty, the project is not one of the agent's, or the chat cannot be made or started.
    pub fn run_agent(
        self: &Arc<Self>,
        id: &str,
        prompt: &str,
        project_id: Option<String>,
    ) -> Result<Chat, Error> {
        let agent = self.agent(id).ok_or("The agent no longer exists")?;
        let new = NewChat {
            id: None,
            project_id,
            agent: agent.agent,
            model: agent.model.clone(),
            access_mode: agent.access_mode,
            effort: agent.effort.clone(),
            compact_window: None,
        };
        self.start_agent_run(&agent, new, prompt, Vec::new(), false)
    }

    /// Sends `prompt` and `attachments` to agent `id`'s chat, and returns the chat.
    ///
    /// Without a chat yet, it starts one with the agent, model, effort, access, context window
    /// and project in `new`, and the agent keeps the agent, model, effort and access for its
    /// later runs, such as its routines. A chat app's agent starts its chat as its chat app's
    /// messages would.
    ///
    /// # Errors
    ///
    /// Returns an error as `run_agent` does, or when the agent cannot be saved or the message
    /// cannot be sent.
    pub fn run_agent_with(
        self: &Arc<Self>,
        id: &str,
        new: NewChat,
        prompt: &str,
        attachments: Vec<Attachment>,
    ) -> Result<Chat, Error> {
        let mut agent = self.agent(id).ok_or("The agent no longer exists")?;
        if let Some(chat) = self.agent_chat(&agent) {
            let queue = self.runner.is_running(&chat) && !self.store.settings().steer_while_working;
            self.chats()
                .send(&chat, prompt.to_string(), attachments, queue)?;
            return self.chat(&chat);
        }
        if agent.chat_app.is_some() {
            let chat = self.open_chat_app_run(&agent, None, true)?;
            if let Err(error) = self
                .chats()
                .send(&chat.id, prompt.to_string(), attachments, false)
            {
                self.chats().discard(&chat.id);
                return Err(error);
            }
            return self.chat(&chat.id);
        }
        if (agent.agent, &agent.model, &agent.effort, agent.access_mode)
            != (new.agent, &new.model, &new.effort, new.access_mode)
        {
            agent.agent = new.agent;
            agent.model.clone_from(&new.model);
            agent.effort.clone_from(&new.effort);
            agent.access_mode = new.access_mode;
            agent = self.save_agent(agent)?;
        }
        self.start_agent_run(&agent, new, prompt, attachments, true)
    }

    /// The id of `agent`'s chat, while it is there and not archived.
    pub fn agent_chat(&self, agent: &Agent) -> Option<String> {
        let id = agent.chat_id.as_ref()?;
        self.store
            .with_chats(|chats| chats.iter().any(|t| &t.id == id && t.archived_at.is_none()))
            .then(|| id.clone())
    }

    /// Makes chat `chat_id` agent `id`'s chat.
    pub(crate) fn set_agent_chat(&self, id: &str, chat_id: &str) -> Result<(), Error> {
        let mut agent = self.agent(id).ok_or("The agent no longer exists")?;
        agent.chat_id = Some(chat_id.to_string());
        self.store.save_agent(&agent)
    }

    /// Starts `agent`'s run in a chat made from `new`, as its chat when `main`. See `run_agent`.
    fn start_agent_run(
        self: &Arc<Self>,
        agent: &Agent,
        new: NewChat,
        prompt: &str,
        attachments: Vec<Attachment>,
        main: bool,
    ) -> Result<Chat, Error> {
        let prompt = prompt.trim();
        if prompt.is_empty() && attachments.is_empty() {
            return Err("Say what the agent should do".into());
        }
        if let Some(project) = &new.project_id {
            let local = self
                .store
                .settings()
                .projects
                .iter()
                .any(|p| &p.id == project && p.host().is_none());
            if !local || !agent.projects.contains(project) {
                return Err(format!("{} can't work in that project", agent.name).into());
            }
        }
        // Linked before the first turn, which reads it for the agent's instructions.
        // Its chat is recorded before it is announced, so it is never listed as a run.
        // A run that cannot start leaves no empty chat behind.
        let new = NewChat { id: None, ..new };
        self.chats()
            .start(new, prompt.to_string(), attachments, |chat| {
                self.store
                    .update_chat(&chat.id, |t| t.agent_id = Some(agent.id.clone()))?;
                if main {
                    self.set_agent_chat(&agent.id, &chat.id)?;
                }
                Ok(())
            })
    }

    /// `text` with the agent's instructions and the files it knows before it, when chat
    /// `chat_id` is an agent's run and `fresh`: the message starts its agent's session.
    pub(crate) fn agent_brief(&self, chat_id: &str, text: String, fresh: bool) -> String {
        if !fresh {
            return text;
        }
        let Some(agent) = self
            .store
            .chat(chat_id)
            .and_then(|t| t.agent_id)
            .and_then(|id| self.agent(&id))
        else {
            return text;
        };
        self.brief_of(&agent) + &text
    }

    /// What agent `id` is told at the start of each conversation, before the first message.
    ///
    /// # Errors
    ///
    /// Returns an error when the agent no longer exists.
    pub fn agent_brief_text(&self, id: &str) -> Result<String, Error> {
        let agent = self.agent(id).ok_or("The agent no longer exists")?;
        Ok(self.brief_of(&agent))
    }

    /// The agent's name, instructions and the files it knows, as its runs are told them.
    fn brief_of(&self, agent: &Agent) -> String {
        let mut brief = format!(
            "<agent>\nYou are {}, an agent the user set up in Patchwork. Follow the user's \
             instructions for this agent below.\n",
            agent.name
        );
        let instructions = agent.instructions.trim();
        if !instructions.is_empty() {
            let _ = write!(brief, "<instructions>\n{instructions}\n</instructions>\n");
        }
        let files = self.knowledge_files(&agent.id);
        if let (false, Some(dir)) = (files.is_empty(), self.knowledge_dir(&agent.id)) {
            let _ = writeln!(
                brief,
                "Files you know are in {}, which you can read but not change. Read one when the \
                 work needs it:",
                dir.display()
            );
            for file in files {
                let _ = writeln!(brief, "- {} ({} bytes)", file.name, file.size);
            }
        }
        brief.push_str("</agent>\n\n");
        brief
    }

    /// Claude's arguments that let an agent's run read the files the agent knows without asking.
    /// Read-only: a run may not change them. Empty for a chat that is not an agent's run, an agent
    /// that knows no files, or a folder Claude's rules can't name.
    pub(crate) fn agent_claude_args(&self, chat_id: &str) -> Vec<String> {
        let Some(id) = self.run_agent_id(chat_id) else {
            return Vec::new();
        };
        let mut args = self.agent_blocked_claude_args(&id);
        if self.knowledge_files(&id).is_empty() {
            return args;
        }
        let Some(dir) = self
            .knowledge_dir(&id)
            .and_then(|d| std::fs::canonicalize(d).ok())
        else {
            return args;
        };
        let dir = dir.to_string_lossy();
        // An absolute path in a rule starts with "//"; one with a separator or a pattern in
        // it would read as more than the folder.
        if !dir.starts_with('/') || dir.contains([',', ' ', '(', ')', '*', '?', '[', ']']) {
            return args;
        }
        args.extend(["--allowedTools".to_string(), format!("Read(/{dir}/**)")]);
        args
    }

    /// Claude's names for the tools agent `id` blocks, so they are not offered at all. The
    /// permission hook refuses them too; this keeps the model from trying.
    fn agent_blocked_claude_args(&self, id: &str) -> Vec<String> {
        let Some(agent) = self.agent(id) else {
            return Vec::new();
        };
        let mut tools: Vec<String> = Vec::new();
        for kind in &agent.blocked.kinds {
            let names: &[&str] = match kind {
                PermissionKind::Command => &["Bash"],
                PermissionKind::Edit => &["Edit", "Write", "MultiEdit", "NotebookEdit"],
                PermissionKind::Web => &["WebFetch"],
                _ => &[],
            };
            tools.extend(names.iter().map(ToString::to_string));
        }
        // A server name Claude can't parse as one is left to the hook.
        tools.extend(
            agent
                .blocked
                .servers
                .iter()
                .filter(|s| {
                    s.chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                })
                .map(|s| format!("mcp__{s}")),
        );
        if tools.is_empty() {
            return Vec::new();
        }
        vec!["--disallowedTools".to_string(), tools.join(",")]
    }

    /// Agent `id`'s runs, newest first, with the tokens each used, and the total.
    #[must_use]
    pub fn agent_usage(&self, id: &str) -> AgentUsage {
        let mut runs: Vec<AgentRun> = self.store.with_chats(|chats| {
            chats
                .iter()
                .filter(|t| t.agent_id.as_deref() == Some(id))
                .map(|t| AgentRun {
                    chat_id: t.id.clone(),
                    title: t.title.clone(),
                    started_at: t.created_at,
                    usage: t
                        .messages
                        .iter()
                        .filter_map(|m| m.usage)
                        .fold(TokenUsage::default(), add),
                })
                .collect()
        });
        runs.sort_by_key(|r| std::cmp::Reverse(r.started_at));
        let total = runs
            .iter()
            .map(|r| r.usage)
            .fold(TokenUsage::default(), add);
        AgentUsage { runs, total }
    }
}

/// The tokens of `a` and `b` together. The context is the larger of the two.
fn add(a: TokenUsage, b: TokenUsage) -> TokenUsage {
    TokenUsage {
        input: a.input.saturating_add(b.input),
        cache_read: a.cache_read.saturating_add(b.cache_read),
        cache_write: a.cache_write.saturating_add(b.cache_write),
        output: a.output.saturating_add(b.output),
        context: a.context.max(b.context),
        context_window: a.context_window.max(b.context_window),
        first_token_ms: None,
        output_per_second: None,
        cost_micros: sum(a.cost_micros, b.cost_micros),
        subagent_tokens: a.subagent_tokens.saturating_add(b.subagent_tokens),
        subagent_cost_micros: sum(a.subagent_cost_micros, b.subagent_cost_micros),
    }
}

/// Two costs that may be missing together; missing only while both are.
fn sum(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0).saturating_add(b.unwrap_or(0))),
    }
}
