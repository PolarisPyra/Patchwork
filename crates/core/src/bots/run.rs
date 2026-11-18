//! Running a bot: the thread each run gets, what its agent is told, what it may not write, and
//! the tokens its runs used.

// Rust guideline compliant 2026-10-09

use std::fmt::Write as _;
use std::sync::Arc;

use super::types::{Bot, BotRun, BotUsage};
use crate::Core;
use crate::Error;
use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::permissions::PermissionKind;
use crate::threads::{Attachment, NewThread, Thread, TokenUsage};

impl Core {
    /// Starts a run of bot `id` on `prompt`, in a new thread, and returns the thread.
    ///
    /// The run works in `project_id` when that is one of the bot's projects, and in a scratch
    /// folder of its own when it is None.
    ///
    /// # Errors
    ///
    /// Returns an error when the bot does not exist, the prompt is empty, the project is not one of the bot's, or the thread cannot be made or started.
    pub fn run_bot(
        self: &Arc<Self>,
        id: &str,
        prompt: &str,
        project_id: Option<String>,
    ) -> Result<Thread, Error> {
        let bot = self.bot(id).ok_or("The bot no longer exists")?;
        let new = NewThread {
            id: None,
            project_id,
            agent: bot.agent,
            model: bot.model.clone(),
            access_mode: bot.access_mode,
            effort: bot.effort.clone(),
            compact_window: None,
        };
        self.start_bot_run(&bot, new, prompt, Vec::new(), false)
    }

    /// Sends `prompt` and `attachments` to bot `id`'s chat, and returns the chat.
    ///
    /// Without a chat yet, it starts one with the agent, model, effort, access, context window
    /// and project in `new`, and the bot keeps the agent, model, effort and access for its
    /// later runs, such as its routines. A chat app's bot starts its chat as its chat app's
    /// messages would.
    ///
    /// # Errors
    ///
    /// Returns an error as `run_bot` does, or when the bot cannot be saved or the message
    /// cannot be sent.
    pub fn run_bot_with(
        self: &Arc<Self>,
        id: &str,
        new: NewThread,
        prompt: &str,
        attachments: Vec<Attachment>,
    ) -> Result<Thread, Error> {
        let mut bot = self.bot(id).ok_or("The bot no longer exists")?;
        if let Some(chat) = self.bot_chat(&bot) {
            let queue = self.runner.is_running(&chat) && !self.store.settings().steer_while_working;
            self.send_message(chat.clone(), prompt.to_string(), attachments, queue)?;
            return self.thread(&chat);
        }
        if bot.chat_app.is_some() {
            let thread = self.open_chat_app_run(&bot, None, true)?;
            if let Err(error) =
                self.send_message(thread.id.clone(), prompt.to_string(), attachments, false)
            {
                self.delete_thread(&thread.id);
                self.emit(AgentEventEnvelope {
                    thread_id: thread.id,
                    event: AgentEvent::ThreadDeleted,
                });
                return Err(error);
            }
            return self.thread(&thread.id);
        }
        if (bot.agent, &bot.model, &bot.effort, bot.access_mode)
            != (new.agent, &new.model, &new.effort, new.access_mode)
        {
            bot.agent = new.agent;
            bot.model.clone_from(&new.model);
            bot.effort.clone_from(&new.effort);
            bot.access_mode = new.access_mode;
            bot = self.save_bot(bot)?;
        }
        self.start_bot_run(&bot, new, prompt, attachments, true)
    }

    /// The id of `bot`'s chat, while it is there and not archived.
    pub(crate) fn bot_chat(&self, bot: &Bot) -> Option<String> {
        let id = bot.thread_id.as_ref()?;
        self.store
            .with_threads(|threads| {
                threads
                    .iter()
                    .any(|t| &t.id == id && t.archived_at.is_none())
            })
            .then(|| id.clone())
    }

    /// Makes thread `thread_id` bot `id`'s chat.
    pub(crate) fn set_bot_chat(&self, id: &str, thread_id: &str) -> Result<(), Error> {
        let mut bot = self.bot(id).ok_or("The bot no longer exists")?;
        bot.thread_id = Some(thread_id.to_string());
        self.store.save_bot(&bot)
    }

    /// Starts `bot`'s run in a thread made from `new`, as its chat when `main`. See `run_bot`.
    fn start_bot_run(
        self: &Arc<Self>,
        bot: &Bot,
        new: NewThread,
        prompt: &str,
        attachments: Vec<Attachment>,
        main: bool,
    ) -> Result<Thread, Error> {
        let prompt = prompt.trim();
        if prompt.is_empty() && attachments.is_empty() {
            return Err("Say what the bot should do".into());
        }
        if let Some(project) = &new.project_id {
            let local = self
                .store
                .settings()
                .projects
                .iter()
                .any(|p| &p.id == project && p.host().is_none());
            if !local || !bot.projects.contains(project) {
                return Err(format!("{} can't work in that project", bot.name).into());
            }
        }
        let thread = self.create_thread(NewThread { id: None, ..new })?;
        // Linked before the first turn, which reads it for the bot's instructions.
        // Its chat is recorded before it is announced, so it is never listed as a run.
        let linked = self
            .store
            .update_thread(&thread.id, |t| t.bot_id = Some(bot.id.clone()))
            .and_then(|()| {
                if main {
                    self.set_bot_chat(&bot.id, &thread.id)
                } else {
                    Ok(())
                }
            });
        if let Err(e) = linked {
            self.delete_thread(&thread.id);
            return Err(e);
        }
        let thread = self.store.thread(&thread.id).unwrap_or(thread);
        self.emit(AgentEventEnvelope {
            thread_id: thread.id.clone(),
            event: AgentEvent::ThreadCreated {
                thread: thread.clone(),
            },
        });
        // A run that cannot start leaves no empty chat behind.
        if let Err(error) =
            self.send_message(thread.id.clone(), prompt.to_string(), attachments, false)
        {
            self.delete_thread(&thread.id);
            self.emit(AgentEventEnvelope {
                thread_id: thread.id,
                event: AgentEvent::ThreadDeleted,
            });
            return Err(error);
        }
        Ok(self.store.thread(&thread.id).unwrap_or(thread))
    }

    /// `text` with the bot's instructions and the files it knows before it, when thread
    /// `thread_id` is a bot's run and `fresh`: the message starts its agent's session.
    pub(crate) fn bot_brief(&self, thread_id: &str, text: String, fresh: bool) -> String {
        if !fresh {
            return text;
        }
        let Some(bot) = self
            .store
            .thread(thread_id)
            .and_then(|t| t.bot_id)
            .and_then(|id| self.bot(&id))
        else {
            return text;
        };
        self.brief_of(&bot) + &text
    }

    /// What bot `id` is told at the start of each conversation, before the first message.
    ///
    /// # Errors
    ///
    /// Returns an error when the bot no longer exists.
    pub fn bot_brief_text(&self, id: &str) -> Result<String, Error> {
        let bot = self.bot(id).ok_or("The bot no longer exists")?;
        Ok(self.brief_of(&bot))
    }

    /// The bot's name, instructions and the files it knows, as its runs are told them.
    fn brief_of(&self, bot: &Bot) -> String {
        let mut brief = format!(
            "<bot>\nYou are {}, a bot the user set up in Patchwork. Follow the user's \
             instructions for this bot below.\n",
            bot.name
        );
        let instructions = bot.instructions.trim();
        if !instructions.is_empty() {
            let _ = write!(brief, "<instructions>\n{instructions}\n</instructions>\n");
        }
        let files = self.knowledge_files(&bot.id);
        if let (false, Some(dir)) = (files.is_empty(), self.knowledge_dir(&bot.id)) {
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
        brief.push_str("</bot>\n\n");
        brief
    }

    /// Claude's arguments that let a bot's run read the files the bot knows without asking.
    /// Read-only: a run may not change them. Empty for a thread that is not a bot's run, a bot
    /// that knows no files, or a folder Claude's rules can't name.
    pub(crate) fn bot_claude_args(&self, thread_id: &str) -> Vec<String> {
        let Some(id) = self.run_bot_id(thread_id) else {
            return Vec::new();
        };
        let mut args = self.bot_blocked_claude_args(&id);
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

    /// Claude's names for the tools bot `id` blocks, so they are not offered at all. The
    /// permission hook refuses them too; this keeps the model from trying.
    fn bot_blocked_claude_args(&self, id: &str) -> Vec<String> {
        let Some(bot) = self.bot(id) else {
            return Vec::new();
        };
        let mut tools: Vec<String> = Vec::new();
        for kind in &bot.blocked.kinds {
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
            bot.blocked
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

    /// Bot `id`'s runs, newest first, with the tokens each used, and the total.
    #[must_use]
    pub fn bot_usage(&self, id: &str) -> BotUsage {
        let mut runs: Vec<BotRun> = self.store.with_threads(|threads| {
            threads
                .iter()
                .filter(|t| t.bot_id.as_deref() == Some(id))
                .map(|t| BotRun {
                    thread_id: t.id.clone(),
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
        BotUsage { runs, total }
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
        first_token_ms: None,
        output_per_second: None,
    }
}
