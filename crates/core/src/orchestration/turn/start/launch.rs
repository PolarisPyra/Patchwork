//! Starting a turn: the agent's command, arguments and input built from the chat, its process
//! spawned, and the watch that retires it once it has sat idle.

// Rust guideline compliant 2026-11-23

use crate::system::Locked;
use crate::Error;
use super::claim::Starting;
use super::running::{LiveSettings, NativeTurn, Pending, RunningTurn, Settings, TurnRequest};
use super::turn::{Turn, assistant_message};
use super::watch::{Watched, watch};
use crate::orchestration::turn::end::{log_cli_version, turn_hooks};

#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Mutex;

use crate::orchestration::changes::attribution::Attribution;
use crate::extensions::hooks::HookEvent;
use crate::provider::TurnSpec;
use crate::provider;
use crate::system::processes::scope;
use crate::Core;
use crate::events::AgentEvent;
use crate::provider::AgentKind;
use crate::system::now_ms;
use crate::chats::Role;
use crate::logging::{AGENT_LOG, Data, Source};
use crate::settings::LogLevel;
use crate::source_control;
use crate::orchestration::changes::checkpoint;
use crate::orchestration::output::ingest::{Wake, ingest};
use crate::orchestration::turn::process::AgentStdin;
use crate::orchestration::turn::process::Interrupts;
use crate::orchestration::turn::process::read_stderr_tail;
use crate::orchestration::turn::process::Stdin;
use crate::orchestration::turn::process::write_line;
use crate::orchestration::messages::queue::Queue;
use crate::orchestration::messages::queue::Steers;
use crate::orchestration::Runner;
use crate::orchestration::tasks::subagents::Controls;

/// Refuses a turn in a chat whose agent is replying, rather than starting a second agent beside
/// it that nothing could stop.
const ALREADY_REPLYING: &str = "This chat already has a reply running";

impl Runner {
    pub(crate) fn start_turn(&self, core: &Arc<Core>, request: TurnRequest) -> Result<(), Error> {
        let starting = Starting::claim(self, &request.chat_id)?;
        self.start_claimed(core, request, starting)
    }

    /// Starts the turn in a chat the caller has claimed, as one that changes the chat first
    /// does, so that no other turn starts in it between the change and this one.
    pub(in crate::orchestration) fn start_claimed(
        &self,
        core: &Arc<Core>,
        request: TurnRequest,
        starting: Starting<'_>,
    ) -> Result<(), Error> {
        let TurnRequest {
            chat_id,
            text,
            attachments,
            images,
            resumes,
        } = request;
        let _starting = starting;
        // Whoever asked may have checked before another turn started here.
        if self.is_running(&chat_id) {
            return Err(ALREADY_REPLYING.into());
        }
        // A turn of its own (a /goal, a compaction) replaces an idle agent process.
        self.replace_idle(&chat_id);
        let (
            agent,
            root,
            session_id,
            model,
            mode,
            effort,
            compact_window,
            context,
            point,
            project_id,
            checkpoint,
        ) = {
            let chat = core.chat(&chat_id)?;
            // The message being sent is the newest user message; the transcript is what came before.
            // A resumed turn whose agent kept no session is told the conversation instead.
            // Claude picks its own session up from where an undo or a fork left it instead.
            let point = (chat.agent == AgentKind::Claude && chat.session_id.is_some())
                .then(|| core.resume_point(&chat_id))
                .flatten();
            let fresh =
                point.is_none() && (chat.context_pending || (resumes && chat.session_id.is_none()));
            let context = fresh.then(|| {
                // A resumed turn sends no new message, so everything so far came before it.
                let end = if resumes {
                    chat.messages.len()
                } else {
                    chat.messages
                        .iter()
                        .rposition(|m| m.role == Role::User)
                        .unwrap_or(chat.messages.len())
                };
                crate::import::transcript(&chat.messages[..end])
            });
            let settings = core.store.settings();
            let root = if let Some(id) = &chat.project_id {
                let project = settings
                    .projects
                    .iter()
                    .find(|p| &p.id == id)
                    .ok_or("Project not found")?;
                // Spawning in a missing folder fails with a bare "No such file or directory". A
                // folder on an SSH host is found missing there, when the helper changes into it.
                if !crate::system::ssh::is_remote(&project.path)
                    && !std::path::Path::new(&project.path).is_dir()
                {
                    return Err(format!(
                        "The {} project's folder, {}, no longer exists",
                        project.name, project.path
                    )
                    .into());
                }
                // Its start is claimed already, so a step that begins from here on sees it.
                if core.worktree_step_running(&chat) {
                    return Err(
                        "Patchwork is merging, updating or discarding this task's worktree; send again once it's done"
                            .into(),
                    );
                }
                core.turn_root(&chat, id, &project.path)
                    .map_err(|e| e.to_string())?
            } else {
                let dir = core.chat_dir(&chat_id);
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                dir.to_string_lossy().into_owned()
            };
            (
                chat.agent,
                root,
                chat.session_id,
                chat.model,
                chat.access_mode,
                chat.effort,
                chat.compact_window
                    .unwrap_or_else(|| settings.tokens.compact_window(chat.agent)),
                context,
                point,
                chat.project_id.clone(),
                // Project files are snapshotted before the newest user message's turn changes them.
                chat.project_id.and(
                    chat.messages
                        .iter()
                        .rev()
                        .find(|m| m.role == Role::User)
                        .map(|m| m.id.clone()),
                ),
            )
        };
        let sends_context = context.is_some();
        // Claude restores a goal of its own on resume, which Patchwork's would run beside. Each
        // session is checked once.
        let checked = format!("claude-goal-checked:{chat_id}");
        if agent == AgentKind::Claude
            && let Some(session) = &session_id
            && !crate::system::ssh::is_remote(&root)
            && core.store.state(&checked).as_deref() != Some(session.as_str())
        {
            crate::provider::claude::launch::clear_own_goal(session, &root, compact_window);
            if let Err(e) = core.store.set_state(&checked, session) {
                log::warn!(error:% = e; "failed to note a checked Claude session: {{error}}");
            }
        }
        let settings = core.store.settings();
        // A chat app's dispatcher has Patchwork's chat tools and nothing of the computer, which
        // only the agents that can be launched without their own tools can promise.
        let dispatcher = core.is_dispatcher(&chat_id);
        if dispatcher && !matches!(agent, AgentKind::Claude | AgentKind::Api) {
            return Err("A chat app's own chat runs on Claude or Patchwork's agent only,                         since the others cannot be kept from the computer.                         Switch it with /agent claude, or start a chat in a project with /new <project>"
                .into());
        }
        let skills = if dispatcher {
            Vec::new()
        } else {
            crate::extensions::skills::enabled(&settings)
        };
        // A /goal that sets or resumes the goal reaches the agent as the goal's brief.
        let text = crate::orchestration::goal::agent_text(&core.store, &chat_id, text);
        // A message that names a skill as "$name" anywhere reaches the agent in the form it runs.
        let text = match crate::extensions::skills::invoked(&text, &skills) {
            Some((skill, rest)) => crate::extensions::skills::invocation(agent, &skill, rest),
            None => text,
        };
        // And with the roles of the custom agents it summons as "&name", and the chat's own.
        let text = if dispatcher {
            text
        } else {
            let (text, switched) = core.summon_agents(&chat_id, text, session_id.is_none());
            core.told_role(&chat_id, switched);
            text
        };
        let text = core.chat_app_voice(&chat_id, text, session_id.is_none(), agent);
        let text = core.agent_brief(&chat_id, text, session_id.is_none());
        let text = core.worktree_brief(&chat_id, text, session_id.is_none());
        let text = core.chat_app_context(&chat_id, text);
        let text = match context {
            Some(context) => format!("{context}{text}"),
            None => text,
        };
        // Codex takes its instructions as the chat's developer instructions; Claude takes them as
        // launch arguments. OpenCode and the listed ACP agents get only the user's system prompt,
        // and Antigravity nothing.
        let instructions = match agent {
            _ if dispatcher => Some(crate::chats::tools::DISPATCHER.to_string()),
            AgentKind::Codex => Some(crate::orchestration::agent_instructions(
                &settings,
                AgentKind::Codex,
            ))
            .filter(|text| !text.is_empty()),
            // Patchwork's own agent has no skill loader, so it is told where the skills are.
            AgentKind::Api => {
                let shell = Some(crate::orchestration::agent_instructions(&settings, agent))
                    .filter(|t| !t.is_empty());
                let parts = [shell, crate::extensions::skills::instructions(&skills)];
                Some(parts.into_iter().flatten().collect::<Vec<_>>().join("\n\n"))
                    .filter(|t| !t.is_empty())
            }
            AgentKind::OpenCode | AgentKind::Qwen | AgentKind::Kimi | AgentKind::Auggie => {
                crate::orchestration::system_prompt(&settings, agent).map(str::to_string)
            }
            AgentKind::Claude | AgentKind::Antigravity => None,
        };
        // Codex and OpenCode load skills from a folder of their own; Claude's comes with its launch.
        let skills_dir = match agent {
            _ if dispatcher => None,
            AgentKind::Codex => crate::extensions::skills::sync(&settings, "codex"),
            AgentKind::OpenCode => crate::extensions::skills::sync(&settings, "opencode"),
            AgentKind::Claude
            | AgentKind::Antigravity
            | AgentKind::Qwen
            | AgentKind::Kimi
            | AgentKind::Auggie
            | AgentKind::Api => None,
        }
        .map(|cli| cli.join("skills").to_string_lossy().into_owned());
        // A dispatcher runs none of the user's hooks or custom agents, which could run commands.
        let (mut guarded, hooks) = if dispatcher {
            (Vec::new(), Vec::new())
        } else {
            (
                crate::permissions::guarded(&settings.permissions, project_id.as_deref()),
                crate::extensions::hooks::agent_hooks(&settings),
            )
        };
        // The custom agents, for the CLIs that can start them as subagents.
        let agents = match agent {
            _ if dispatcher => Vec::new(),
            AgentKind::Claude | AgentKind::Codex | AgentKind::OpenCode => {
                core.chat_agents(&chat_id)
            }
            AgentKind::Antigravity
            | AgentKind::Qwen
            | AgentKind::Kimi
            | AgentKind::Auggie
            | AgentKind::Api => Vec::new(),
        };
        // The tools an agent blocks. Every agent but `opencode run` asks Patchwork at each call;
        // that one is told at launch, and can't be told about servers.
        let blocked = core
            .run_agent_id(&chat_id)
            .and_then(|id| core.agent(&id))
            .map(|agent| agent.blocked)
            .unwrap_or_default();
        if !blocked.is_empty() {
            let opencode_run =
                agent == AgentKind::OpenCode && !crate::provider::opencode::uses_serve();
            if opencode_run && !blocked.servers.is_empty() {
                return Err(
                    "This agent blocks MCP servers, which this version of OpenCode can't \
                            enforce. Update OpenCode, or unblock the servers"
                        .into(),
                );
            }
            if opencode_run {
                for &kind in &blocked.kinds {
                    guarded.retain(|g| g.kind != kind);
                    guarded.push(crate::permissions::Guarded {
                        kind,
                        deny: Vec::new(),
                        ask: Vec::new(),
                        denied: true,
                    });
                }
            }
            if agent == AgentKind::Claude && crate::provider::mcp::relay_endpoint().is_none() {
                return Err(
                    "Patchwork's permission relay is not running, so this agent's blocked \
                            tools would not be refused. Restart Patchwork and try again"
                        .into(),
                );
            }
        }
        let vision_model = settings.vision.clone();
        let unlimited_steps = settings.unlimited_steps;
        let token_limit = settings.turn_token_limit;
        let spec = TurnSpec {
            agent,
            root: crate::system::ssh::here(&root),
            host: crate::system::ssh::split(&root).map(|at| at.host),
            chat_id: &chat_id,
            session_id: session_id.as_deref(),
            model: model.as_deref(),
            vision_model: vision_model.as_ref(),
            text: &text,
            mode,
            effort: effort.as_deref(),
            compact_window,
            unlimited_steps,
            token_limit,
            attachments: &attachments,
            images: &images,
            instructions: instructions.as_deref(),
            skills_dir: skills_dir.as_deref(),
            guarded: &guarded,
            agents: &agents,
            hooks: &hooks,
            dispatcher,
        };

        // Agents often edit files through the shell, so changes are also found by
        // comparing git state before and after the turn.
        let attribution =
            Attribution::shared(source_control::git_changes(&root).unwrap_or_default());
        if !resumes {
            turn_hooks(
                core,
                HookEvent::UserPromptSubmit,
                &chat_id,
                &root,
                agent,
                None,
            );
            if let Some(message_id) = &checkpoint {
                checkpoint::capture(&root, &chat_id, message_id);
            }
        }
        let mut args = provider::turn_args(agent, &spec);
        if agent == AgentKind::Claude && dispatcher {
            args.extend(crate::provider::claude::launch::dispatcher_args(
                crate::chats::tools::DISPATCHER,
            ));
        } else if agent == AgentKind::Claude {
            args.extend(crate::provider::claude::launch::launch_args(
                &settings,
                &hooks,
                crate::orchestration::agent_instructions(&settings, AgentKind::Claude),
            ));
            args.extend(crate::provider::claude::launch::permission_args());
            args.extend(crate::provider::claude::launch::agents_args(&agents, &root));
            args.extend(core.agent_claude_args(&chat_id));
        }
        // After an undo or a fork the session goes on from the reply they left last. The
        // point is forgotten once the session reports in, as it then goes on from there.
        if let Some(point) = &point {
            args.extend(crate::provider::claude::launch::resume_point_args(point));
        }
        if let Some(why) = provider::hooks::unguarded(agent, &args) {
            return Err(why.into());
        }
        let mut input = provider::turn_input(agent, &spec);
        if let Some(host) = spec.host {
            crate::system::ssh::relay_reaches(host)?;
            // The files the input names, such as attachments and where sessions are kept, as the
            // agent on the host finds them.
            input = input
                .iter()
                .map(|line| crate::system::ssh::mirrored(host, line))
                .collect::<Result<_, _>>()?;
        }
        // A project on an SSH host runs its agent there, under Patchwork's helper, which stops it
        // when the connection ends, so no scope is made for it here.
        let remote = crate::system::ssh::is_remote(&root);
        let (mut cmd, scope) = if remote {
            (provider::remote_turn_command(agent)?, None)
        } else {
            scope::scoped(provider::turn_command(agent)?)
        };
        if let Some(shell) = provider::agent_shell(agent) {
            // Claude Code on Windows runs its commands in Git Bash, and refuses to start when it
            // cannot find it on PATH.
            if cfg!(windows) && agent == AgentKind::Claude && shell.ends_with("bash.exe") {
                cmd.env("CLAUDE_CODE_GIT_BASH_PATH", &shell);
            }
            cmd.env("SHELL", shell);
        }
        if agent == AgentKind::Claude {
            cmd.envs(crate::provider::claude::launch::launch_env(compact_window));
            if let Some(file) = crate::provider::claude::launch::shell_env_file() {
                cmd.env("CLAUDE_ENV_FILE", file);
            }
        }
        cmd.args(&args)
            .envs(provider::turn_env(agent, &spec))
            .envs(crate::permissions::hook::env(
                agent,
                &chat_id,
                crate::system::ssh::here(&root),
            ))
            .current_dir(&root);
        if !remote {
            cmd.envs(core.worktree_env(&root));
        }
        // Another provider's key is not this agent's to read, nor its commands'.
        let vision = spec.vision_model.map(|vision| vision.agent);
        for var in provider::withheld_env(agent, vision) {
            cmd.env_remove(var);
        }
        let mut cmd = crate::system::ssh::supervised(cmd)?;
        cmd.stdin(if input.is_empty() {
            Stdio::null()
        } else {
            Stdio::piped()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        #[cfg(unix)]
        cmd.process_group(0);
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to start agent: {e}"))?;
        let pid = child.id();
        if AGENT_LOG.logs(LogLevel::Info) {
            // Below Trace only flags are kept, since an argument can be the prompt.
            let trace = AGENT_LOG.logs(LogLevel::Trace);
            let args: Vec<String> = args
                .iter()
                .map(|a| {
                    if trace || a.starts_with('-') {
                        a.clone()
                    } else {
                        format!("<{} chars>", a.chars().count())
                    }
                })
                .collect();
            let started = format!(
                "{} in {root}: {} {}",
                cmd.get_program().to_string_lossy(),
                args.join(" "),
                if session_id.is_some() {
                    "(resumed)"
                } else {
                    ""
                },
            );
            let source = Source::agent(agent, pid, &chat_id, None);
            AGENT_LOG.write(
                LogLevel::Info,
                "start",
                &source,
                Data::Text(started.trim_end().into()),
            );
            log_cli_version(agent);
        }
        // Until the turn is recorded below nothing else can stop the agent, so a failure before
        // then stops it here.
        let abandon = |child: &mut std::process::Child, error: Error| -> Result<(), Error> {
            let _ = child.kill();
            let _ = child.wait();
            Err(error)
        };
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            return abandon(&mut child, "Agent output is unavailable".into());
        };
        // The agent has the transcript, or its session from the point, so later turns resume its
        // session instead.
        if (sends_context || point.is_some())
            && let Err(error) = core
                .store
                .update_chat(&chat_id, |t| t.context_pending = false)
        {
            return abandon(&mut child, error);
        }
        let source = Source::agent(agent, pid, &chat_id, None);
        let stderr_tail = std::thread::spawn(move || read_stderr_tail(stderr, &source));
        let stdin: Stdin = Arc::new(Mutex::new(AgentStdin::take(&mut child, agent, &chat_id)));
        for line in &input {
            write_line(&stdin, line);
        }
        if provider::closes_input(agent) {
            stdin.locked().take();
        }

        let message = assistant_message();
        let cancelled = Arc::new(AtomicBool::new(false));
        let interrupts = Arc::<Interrupts>::default();
        let exited = Arc::new(AtomicBool::new(false));
        let pending = Pending::default();
        let native_turn = NativeTurn::default();
        let steers = Steers::default();
        let queue = Queue::default();
        let controls = Controls::default();
        let idle = Arc::new(AtomicBool::new(false));
        let alone = Arc::new(AtomicBool::new(false));
        let compacting = Arc::new(AtomicBool::new(false));
        let cancelling = Arc::new(AtomicBool::new(false));
        let compact_window = Arc::new(AtomicU64::new(compact_window));
        let live_mode = Arc::new(Mutex::new(mode));
        let settings: Settings = Arc::new(Mutex::new(LiveSettings {
            model: model.clone(),
            effort: effort.clone(),
            ..LiveSettings::default()
        }));
        let wake = Wake::default();
        let turn = Turn {
            core: Arc::clone(core),
            chat_id: chat_id.clone(),
            message_id: message.id.clone(),
            agent,
            cancelled: Arc::clone(&cancelled),
            interrupts: Arc::clone(&interrupts),
            exited: Arc::clone(&exited),
            attribution: Arc::clone(&attribution),
            scope,
            stdin: Arc::clone(&stdin),
            pending: Arc::clone(&pending),
            native: Arc::clone(&native_turn),
            steers: Arc::clone(&steers),
            queue: Arc::clone(&queue),
            controls: Arc::clone(&controls),
            idle: Arc::clone(&idle),
            alone: Arc::clone(&alone),
            compacting: Arc::clone(&compacting),
            cancelling: Arc::clone(&cancelling),
            wake: wake.clone(),
            last_output: Arc::new(AtomicU64::new(now_ms())),
            calls_open: Arc::new(AtomicBool::new(false)),
            stalled: Arc::new(AtomicBool::new(false)),
            root,
            session_id,
            model,
            vision_model,
            text,
            resumes,
            sent_context: sends_context,
            mode: Arc::clone(&live_mode),
            effort,
            settings: Arc::clone(&settings),
            compact_window: Arc::clone(&compact_window),
            unlimited_steps,
            token_limit,
            attachments,
            images,
            instructions,
            skills_dir,
            guarded,
            agents,
            hooks,
            dispatcher,
        };
        self.turns.locked().insert(
            chat_id,
            RunningTurn {
                pid: child.id(),
                cancelled,
                interrupts,
                exited,
                scope: turn.scope.clone(),
                agent,
                stdin,
                pending,
                native: native_turn,
                steers,
                queue,
                controls,
                idle,
                alone,
                compacting,
                cancelling,
                mode: live_mode,
                settings,
                compact_window,
                wake,
                stale: AtomicBool::new(false),
                root: turn.root.clone(),
                attribution,
            },
        );
        crate::orchestration::turn::recovery::note_agent(
            &core.store,
            &turn.chat_id,
            pid,
            turn.scope.as_deref(),
        );
        // Stopped while it was starting, as by Stop or archiving the chat: it stops now it can.
        if self.stop_starting.locked().remove(&turn.chat_id) {
            self.cancel(&turn.chat_id);
        }
        turn.publish(AgentEvent::MessageStarted { message });
        let watched = Watched {
            core: Arc::clone(core),
            chat_id: turn.chat_id.clone(),
            pid,
            scope: turn.scope.clone(),
            exited: Arc::clone(&turn.exited),
            idle: Arc::clone(&turn.idle),
            alone: Arc::clone(&turn.alone),
            pending: Arc::clone(&turn.pending),
            last_output: Arc::clone(&turn.last_output),
            agent: turn.agent,
            calls_open: Arc::clone(&turn.calls_open),
            stalled: Arc::clone(&turn.stalled),
        };
        std::thread::spawn(move || watch(&watched));
        std::thread::spawn(move || ingest(turn, child, stdout, stderr_tail));
        Ok(())
    }
}
