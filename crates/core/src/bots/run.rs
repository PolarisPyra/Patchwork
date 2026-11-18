//! Running a bot: the thread each run gets, what its agent is told, what it may not write, and
//! the tokens its runs used.

// Rust guideline compliant 2026-10-09

use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use super::types::{Bot, BotRun, BotUsage};
use crate::Core;
use crate::Error;
use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::threads::{Attachment, NewThread, Thread, TokenUsage};

/// The paths a sandboxed turn may not write, and the paths inside those it may.
type Sandbox = (Vec<PathBuf>, Vec<PathBuf>);

/// Why a bot's run refuses to start where the sandbox can't run.
const SANDBOX_MISSING: &str =
    "Bots run only inside a sandbox, which needs bubblewrap (bwrap). Install it and try again";

/// Files and folders in the home folder a bot's run may read but not change: the agents'
/// settings, hooks and instructions, the shells' startup files, and what runs at login, so a
/// run can't plant something that runs later outside its sandbox.
const HOME_PROTECTED: &[&str] = &[
    ".claude/settings.json",
    ".claude/settings.local.json",
    ".claude/hooks",
    ".claude/CLAUDE.md",
    ".claude/agents",
    ".claude/commands",
    ".claude/skills",
    ".codex/config.toml",
    ".codex/AGENTS.md",
    ".config/opencode",
    ".bashrc",
    ".bash_profile",
    ".bash_login",
    ".bash_logout",
    ".profile",
    ".zshrc",
    ".zshenv",
    ".zprofile",
    ".zlogin",
    ".config/fish",
    ".config/autostart",
    ".config/systemd/user",
    ".config/environment.d",
    ".local/bin",
    ".gitconfig",
    ".config/git",
    ".ssh",
];

impl Core {
    /// Starts a run of bot `id` on `prompt`, in a new thread, and returns the thread.
    ///
    /// The run works in `project_id` when that is one of the bot's projects, and in a scratch
    /// folder of its own when it is None.
    ///
    /// # Errors
    ///
    /// Returns an error when the bot does not exist, the prompt is empty, the sandbox can't run
    /// here, the project is not one of the bot's, or the thread cannot be made or started.
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
            plan_mode: false,
        };
        self.start_bot_run(&bot, new, prompt, Vec::new())
    }

    /// Starts a run of bot `id` on `prompt` and `attachments` with the agent, model, effort,
    /// access, context window and project in `new`, and returns the thread. The bot keeps the
    /// agent, model, effort and access for its later runs, such as its routines.
    ///
    /// # Errors
    ///
    /// Returns an error as `run_bot` does, or when the bot cannot be saved.
    pub fn run_bot_with(
        self: &Arc<Self>,
        id: &str,
        new: NewThread,
        prompt: &str,
        attachments: Vec<Attachment>,
    ) -> Result<Thread, Error> {
        let mut bot = self.bot(id).ok_or("The bot no longer exists")?;
        if (bot.agent, &bot.model, &bot.effort, bot.access_mode)
            != (new.agent, &new.model, &new.effort, new.access_mode)
        {
            bot.agent = new.agent;
            bot.model.clone_from(&new.model);
            bot.effort.clone_from(&new.effort);
            bot.access_mode = new.access_mode;
            bot = self.save_bot(bot)?;
        }
        self.start_bot_run(&bot, new, prompt, attachments)
    }

    /// Starts `bot`'s run in a thread made from `new`. See `run_bot`.
    fn start_bot_run(
        self: &Arc<Self>,
        bot: &Bot,
        new: NewThread,
        prompt: &str,
        attachments: Vec<Attachment>,
    ) -> Result<Thread, Error> {
        let prompt = prompt.trim();
        if prompt.is_empty() && attachments.is_empty() {
            return Err("Say what the bot should do".into());
        }
        if !crate::system::sandbox::available() {
            return Err(SANDBOX_MISSING.into());
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
        // Linked before the first turn, which reads it for the bot's instructions and sandbox.
        if let Err(e) = self
            .store
            .update_thread(&thread.id, |t| t.bot_id = Some(bot.id.clone()))
        {
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
        let Some(bot) = self.store.thread(thread_id).and_then(|t| t.bot_id) else {
            return Vec::new();
        };
        if self.knowledge_files(&bot).is_empty() {
            return Vec::new();
        }
        let Some(dir) = self
            .knowledge_dir(&bot)
            .and_then(|d| std::fs::canonicalize(d).ok())
        else {
            return Vec::new();
        };
        let dir = dir.to_string_lossy();
        // An absolute path in a rule starts with "//"; one with a separator or a pattern in
        // it would read as more than the folder.
        if !dir.starts_with('/') || dir.contains([',', ' ', '(', ')', '*', '?', '[', ']']) {
            return Vec::new();
        }
        vec!["--allowedTools".to_string(), format!("Read(/{dir}/**)")]
    }

    /// What a turn of thread `thread_id` may not write, and what it may inside those: a
    /// worktree's limits (see `worktree_sandbox`), and for a bot's run also Patchwork's own
    /// settings, data and caches, every bot's files, the projects it is not working in, and the
    /// home folder's agent settings and shell startup files. None for a chat or task that runs
    /// unsandboxed.
    ///
    /// # Errors
    ///
    /// Returns an error for a bot's run where the sandbox can't run, so it never runs without
    /// one.
    pub(crate) fn run_sandbox(&self, thread_id: &str) -> Result<Option<Sandbox>, Error> {
        let worktree = self.worktree_sandbox(thread_id);
        let Some(thread) = self.store.thread(thread_id) else {
            return Ok(worktree);
        };
        let owner = match &thread.parent {
            Some(parent) => match self.store.thread(&parent.thread_id) {
                Some(owner) => owner,
                None => return Ok(worktree),
            },
            None => thread,
        };
        if owner.bot_id.is_none() {
            return Ok(worktree);
        }
        if !crate::system::sandbox::available() {
            return Err(SANDBOX_MISSING.into());
        }
        let (mut read_only, mut writable) = worktree.unwrap_or_default();
        // Made now, so a run can't make it and plant files there itself.
        if let Err(e) = std::fs::create_dir_all(&self.bots_dir) {
            log::warn!("cannot create {}: {e}", self.bots_dir.display());
        }
        let dirs = crate::system::dirs();
        read_only.extend([
            self.bots_dir.clone(),
            dirs.config.clone(),
            dirs.data.clone(),
            dirs.cache.clone(),
        ]);
        if let Some(data) = self.chats_dir.parent() {
            read_only.push(data.to_path_buf());
        }
        // The bash tool keeps this turn's shell in a file in the read-only cache; only that
        // file stays writable, made now so the bind has something to hold.
        let shell = crate::harness::tools::shell_file(thread_id);
        if let Some(dir) = shell.parent()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            log::warn!("cannot create {}: {e}", dir.display());
        }
        if let Err(e) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&shell)
        {
            log::warn!("cannot create {}: {e}", shell.display());
        }
        writable.push(shell);
        let home = crate::system::fs::home_dir().and_then(|h| std::fs::canonicalize(h).ok());
        if let Some(home) = &home {
            read_only.extend(
                HOME_PROTECTED
                    .iter()
                    .filter_map(|p| std::fs::canonicalize(home.join(p)).ok()),
            );
        }
        // The folder the run works in: its project, or its own scratch folder.
        let projects = self.store.settings().projects;
        let root = if let Some(id) = &owner.project_id {
            projects
                .iter()
                .find(|p| &p.id == id)
                .and_then(|p| std::fs::canonicalize(&p.path).ok())
        } else {
            let own = self.chat_dir(&owner.id);
            writable.push(own.clone());
            std::fs::canonicalize(own).ok()
        };
        for project in projects {
            if project.host().is_some() || Some(&project.id) == owner.project_id.as_ref() {
                continue;
            }
            for path in crate::threads::project_folders(&project.path) {
                // A folder that holds the home folder or the run's own would take them with it.
                let holds = |p: &Option<PathBuf>| p.as_ref().is_some_and(|p| p.starts_with(&path));
                if !holds(&home) && !holds(&root) {
                    read_only.push(path);
                }
            }
        }
        Ok(Some((read_only, writable)))
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
