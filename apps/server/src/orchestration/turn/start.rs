//! One running turn: its shared state, and the spawn that starts it.

use crate::orchestration::turn::end::{background_id, log_cli_version, plugin_hooks};

use std::collections::HashMap;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::time::Instant;

use serde_json::Value;

use crate::orchestration::changes::attribution::{self, Attribution, Reach, SharedAttribution};
use crate::plugins::HookEvent;
use crate::permissions::Target;
use crate::provider::TurnSpec;
use crate::provider;
use crate::system::scope;
use crate::Server;
use crate::threads::AccessMode;
use crate::orchestration::AgentEvent;
use crate::provider::AgentKind;
use crate::threads::Attachment;
use crate::threads::Goal;
use crate::threads::GoalStatus;
use crate::threads::Message;
use crate::threads::MessageStatus;
use crate::system::new_id;
use crate::system::now_ms;
use crate::threads::Role;
use crate::logging::{AGENTS, Data, Source};
use crate::settings::LogLevel;
use crate::workspace;
use crate::orchestration::changes::checkpoint;
use crate::orchestration::output::ingest::ingest;
use crate::orchestration::turn::process::AgentStdin;
use crate::orchestration::turn::process::Interrupts;
use crate::orchestration::turn::process::close_stdin_unless_steered;
use crate::orchestration::turn::process::read_stderr_tail;
use crate::orchestration::turn::process::Stdin;
use crate::orchestration::turn::process::terminate;
use crate::orchestration::turn::process::IDLE_LIMIT;
use crate::orchestration::turn::process::TURN_TIMEOUT;
use crate::orchestration::turn::process::WATCH_EVERY;
use crate::orchestration::turn::process::write_line;
use crate::orchestration::messages::queue::Queue;
use crate::orchestration::messages::queue::Steers;
use crate::orchestration::Runner;
use crate::orchestration::output::stream::publish;
use crate::orchestration::output::stream::Unsaved;
use crate::orchestration::tasks::subagents::Controls;
use crate::orchestration::tasks::subagents::Tasks;

/// An approval waiting for the user.
pub(in crate::orchestration) struct PendingApproval {
    pub(in crate::orchestration) message_id: String,
    /// What `approval_reply` needs to answer it.
    pub(in crate::orchestration) reply: Value,
    /// What it asks to do, for the permission rules.
    pub(in crate::orchestration) target: Option<Target>,
    /// The folder the thread works in.
    pub(in crate::orchestration) root: String,
}

/// Pending approvals of one turn, by approval id.
pub(in crate::orchestration) type Pending = Arc<Mutex<HashMap<String, PendingApproval>>>;

/// The agent's id for the turn now running (Codex reports it once the turn starts).
pub(in crate::orchestration) type NativeTurn = Arc<Mutex<Option<String>>>;

pub(in crate::orchestration) struct RunningTurn {
    pub(in crate::orchestration) pid: u32,
    pub(in crate::orchestration) cancelled: Arc<AtomicBool>,
    /// The interrupts Stop wrote to the agent, and whether it answered them.
    pub(in crate::orchestration) interrupts: Arc<Interrupts>,
    /// Set once the agent process has exited and been reaped.
    pub(in crate::orchestration) exited: Arc<AtomicBool>,
    /// The systemd scope the agent runs in, when scopes are on.
    pub(in crate::orchestration) scope: Option<String>,
    pub(in crate::orchestration) agent: AgentKind,
    pub(in crate::orchestration) stdin: Stdin,
    pub(in crate::orchestration) pending: Pending,
    pub(in crate::orchestration) native_turn: NativeTurn,
    pub(in crate::orchestration) steers: Steers,
    pub(in crate::orchestration) queue: Queue,
    pub(in crate::orchestration) tasks: Tasks,
    pub(in crate::orchestration) controls: Controls,
    /// The agent has finished its reply and stays up only for shell commands it left running;
    /// the thread counts as idle, and the next message starts a new reply in this process.
    pub(in crate::orchestration) idle: Arc<AtomicBool>,
    /// Shared with the project's watcher, which adds files to the reply as they change.
    pub(in crate::orchestration) attribution: SharedAttribution,
}

/// What a new turn sends to the agent, and the thread it belongs to.
pub struct TurnRequest {
    pub thread_id: String,
    pub text: String,
    pub attachments: Vec<Attachment>,
    pub images: Vec<provider::Image>,
}

impl Runner {
    pub fn start_turn(&self, server: &Arc<Server>, request: TurnRequest) -> Result<(), String> {
        let TurnRequest {
            thread_id,
            text,
            attachments,
            images,
        } = request;
        // A turn of its own (a /goal, a compaction) replaces an idle agent process.
        self.retire_idle(&thread_id);
        let (
            agent,
            root,
            session_id,
            model,
            mode,
            effort,
            compact_window,
            plan,
            context,
            project_id,
            checkpoint,
        ) = {
            let thread = server.store.thread(&thread_id).ok_or("Task not found")?;
            // The message being sent is the newest user message; the transcript is what came before.
            let context = thread.context_pending.then(|| {
                let end = thread
                    .messages
                    .iter()
                    .rposition(|m| m.role == Role::User)
                    .unwrap_or(thread.messages.len());
                crate::import::transcript(&thread.messages[..end])
            });
            let settings = server.store.settings();
            let root = match &thread.project_id {
                Some(id) => {
                    let project = settings
                        .projects
                        .iter()
                        .find(|p| &p.id == id)
                        .ok_or("Project not found")?;
                    // Spawning in a missing folder fails with a bare "No such file or directory".
                    if !std::path::Path::new(&project.path).is_dir() {
                        return Err(format!(
                            "The {} project's folder, {}, no longer exists",
                            project.name, project.path
                        ));
                    }
                    project.path.clone()
                }
                None => {
                    let dir = server.chat_dir(&thread_id);
                    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                    dir.to_string_lossy().into_owned()
                }
            };
            (
                thread.agent,
                root,
                thread.session_id,
                thread.model,
                thread.access_mode,
                thread.effort,
                thread
                    .compact_window
                    .unwrap_or_else(|| settings.tokens.compact_window(thread.agent)),
                thread.plan_mode,
                context,
                thread.project_id.clone(),
                // Project files are snapshotted before the newest user message's turn changes them.
                thread.project_id.and(
                    thread
                        .messages
                        .iter()
                        .rev()
                        .find(|m| m.role == Role::User)
                        .map(|m| m.id.clone()),
                ),
            )
        };
        let sends_context = context.is_some();
        let settings = server.store.settings();
        let skills = crate::skills::enabled(&settings);
        // A message that names a skill as "$name" anywhere reaches the agent in the form it runs.
        let text = match crate::skills::invoked(&text, &skills) {
            Some((skill, rest)) => crate::skills::invocation(agent, &skill, rest),
            None => text,
        };
        // And with the roles of the custom agents it summons as "&name", and the chat's own.
        let (text, switched) = server.summon_agents(&thread_id, text, session_id.is_none());
        server.told_role(&thread_id, switched);
        let text = match context {
            Some(context) => format!("{context}{text}"),
            None => text,
        };
        // Codex takes its instructions as the thread's developer instructions; Claude takes them as
        // launch arguments. OpenCode and Antigravity get only the user's system prompt.
        let instructions = match agent {
            AgentKind::Codex => Some(crate::orchestration::agent_instructions(
                &settings,
                AgentKind::Codex,
            ))
            .filter(|text| !text.is_empty()),
            // Patchwork's own agent has no skill loader, so it is told where the skills are.
            AgentKind::Api => {
                let shell = Some(crate::orchestration::agent_instructions(&settings, agent))
                    .filter(|t| !t.is_empty());
                let parts = [shell, crate::skills::instructions(&skills)];
                Some(parts.into_iter().flatten().collect::<Vec<_>>().join("\n\n"))
                    .filter(|t| !t.is_empty())
            }
            AgentKind::OpenCode | AgentKind::Antigravity => {
                crate::orchestration::system_prompt(&settings, agent).map(str::to_string)
            }
            AgentKind::Claude => None,
        };
        // Codex and OpenCode load skills from a folder of their own; Claude's comes with its launch.
        let skills_dir = match agent {
            AgentKind::Codex => crate::skills::sync(&settings, "codex"),
            AgentKind::OpenCode => crate::skills::sync(&settings, "opencode"),
            AgentKind::Claude | AgentKind::Antigravity | AgentKind::Api => None,
        }
        .map(|cli| cli.join("skills").to_string_lossy().into_owned());
        let guarded = crate::permissions::guarded(&settings.permissions, project_id.as_deref());
        let hooks = crate::plugins::agent_hooks(&settings);
        // The custom agents, for the CLIs that can start them as subagents.
        let agents = match agent {
            AgentKind::Claude | AgentKind::Codex | AgentKind::OpenCode => {
                server.thread_agents(&thread_id)
            }
            AgentKind::Antigravity | AgentKind::Api => Vec::new(),
        };
        let vision_model = settings.vision_model.clone();
        let spec = TurnSpec {
            root: &root,
            session_id: session_id.as_deref(),
            model: model.as_deref(),
            vision_model: vision_model.as_deref(),
            text: &text,
            mode,
            plan,
            effort: effort.as_deref(),
            compact_window,
            attachments: &attachments,
            images: &images,
            instructions: instructions.as_deref(),
            skills_dir: skills_dir.as_deref(),
            guarded: &guarded,
            agents: &agents,
            hooks: &hooks,
        };

        // Agents often edit files through the shell, so changes are also found by
        // comparing git state before and after the turn.
        let attribution = Attribution::shared(workspace::git_changes(&root).unwrap_or_default());
        plugin_hooks(
            server,
            HookEvent::UserPromptSubmit,
            &thread_id,
            &root,
            agent,
            None,
        );
        if let Some(message_id) = &checkpoint {
            checkpoint::capture(&root, &thread_id, message_id);
        }
        let mut args = provider::turn_args(agent, &spec);
        if agent == AgentKind::Claude {
            args.extend(crate::provider::claude::launch::launch_args(
                &settings, &hooks,
            ));
            args.extend(crate::provider::claude::launch::permission_args());
            args.extend(crate::provider::claude::launch::agents_args(&agents, &root));
        }
        let input = provider::turn_input(agent, &spec);
        let (mut cmd, scope) = scope::scoped(provider::turn_command(agent)?);
        if let Some(shell) = provider::agent_shell(agent, settings.shell.as_deref()) {
            // Claude Code on Windows runs its commands in Git Bash, and refuses to start when it
            // cannot find it on PATH.
            if cfg!(windows) && agent == AgentKind::Claude && shell.ends_with("bash.exe") {
                cmd.env("CLAUDE_CODE_GIT_BASH_PATH", &shell);
            }
            cmd.env("SHELL", shell);
        }
        if agent == AgentKind::Claude {
            cmd.envs(crate::provider::claude::launch::launch_env(compact_window));
        }
        cmd.args(&args)
            .envs(provider::turn_env(agent, &spec))
            .envs(crate::permissions::hook::env(agent, &thread_id, &root))
            .current_dir(&root)
            .stdin(if input.is_empty() {
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
        if AGENTS.logs(LogLevel::Info) {
            // Below Trace only flags are kept, since an argument can be the prompt.
            let trace = AGENTS.logs(LogLevel::Trace);
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
            let source = Source::agent(agent, pid, &thread_id, None);
            AGENTS.write(
                LogLevel::Info,
                "start",
                &source,
                Data::Text(started.trim_end().into()),
            );
            log_cli_version(agent);
        }
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            return Err("Agent output is unavailable".into());
        };
        // The agent has the transcript now, so later turns resume its session instead.
        if sends_context {
            server
                .store
                .update_thread(&thread_id, |t| t.context_pending = false)?;
        }
        let source = Source::agent(agent, pid, &thread_id, None);
        let stderr_tail = std::thread::spawn(move || read_stderr_tail(stderr, source));
        let stdin: Stdin = Arc::new(Mutex::new(AgentStdin::take(&mut child, agent, &thread_id)));
        input.iter().for_each(|line| {
            write_line(&stdin, line);
        });

        let message = assistant_message();
        let cancelled = Arc::new(AtomicBool::new(false));
        let interrupts = Arc::<Interrupts>::default();
        let timed_out = Arc::new(AtomicBool::new(false));
        let exited = Arc::new(AtomicBool::new(false));
        let pending = Pending::default();
        let native_turn = NativeTurn::default();
        let steers = Steers::default();
        let queue = Queue::default();
        let tasks = Tasks::default();
        let controls = Controls::default();
        let idle = Arc::new(AtomicBool::new(false));
        let turn = Turn {
            server: server.clone(),
            thread_id: thread_id.clone(),
            message_id: message.id.clone(),
            agent,
            cancelled: cancelled.clone(),
            interrupts: interrupts.clone(),
            timed_out: timed_out.clone(),
            exited: exited.clone(),
            attribution: attribution.clone(),
            scope,
            stdin: stdin.clone(),
            pending: pending.clone(),
            native_turn: native_turn.clone(),
            steers: steers.clone(),
            queue: queue.clone(),
            tasks: tasks.clone(),
            controls: controls.clone(),
            idle: idle.clone(),
            root,
            session_id,
            model,
            vision_model,
            text,
            mode,
            plan,
            effort,
            compact_window,
            attachments,
            images,
            instructions,
            skills_dir,
            guarded,
            agents,
            hooks,
        };
        self.turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                thread_id,
                RunningTurn {
                    pid: child.id(),
                    cancelled,
                    interrupts,
                    exited,
                    scope: turn.scope.clone(),
                    agent,
                    stdin,
                    pending,
                    native_turn,
                    steers,
                    queue,
                    tasks,
                    controls,
                    idle,
                    attribution,
                },
            );
        turn.publish(AgentEvent::MessageStarted { message });
        // Claude restores an unmet goal whenever its session resumes, so this turn pursues it.
        if agent == AgentKind::Claude && turn.session_id.is_some() {
            let paused = server
                .store
                .thread(&turn.thread_id)
                .and_then(|t| t.goal)
                .filter(|g| g.status == GoalStatus::Paused);
            if let Some(goal) = paused {
                let status = GoalStatus::Active;
                turn.publish(AgentEvent::GoalUpdated {
                    goal: Some(Goal { status, ..goal }),
                });
            }
        }
        let watched = Watched {
            server: server.clone(),
            thread_id: turn.thread_id.clone(),
            pid: child.id(),
            exited: turn.exited.clone(),
            idle: turn.idle.clone(),
            timed_out,
        };
        std::thread::spawn(move || watch(watched));
        std::thread::spawn(move || ingest(turn, child, stdout, stderr_tail));
        Ok(())
    }
}

/// What `watch` checks one agent process by.
struct Watched {
    server: Arc<Server>,
    thread_id: String,
    pid: u32,
    exited: Arc<AtomicBool>,
    idle: Arc<AtomicBool>,
    timed_out: Arc<AtomicBool>,
}

/// Stops the agent when one reply runs past `TURN_TIMEOUT`, and retires it once it has been
/// idle for `IDLE_LIMIT` with no shell commands of its own still running. Ends with the agent.
fn watch(w: Watched) {
    let mut busy_since = Instant::now();
    let mut was_idle = false;
    loop {
        std::thread::sleep(WATCH_EVERY);
        if w.exited.load(Ordering::SeqCst) {
            return;
        }
        let idle = w.idle.load(Ordering::SeqCst);
        if !idle && was_idle {
            busy_since = Instant::now();
        }
        was_idle = idle;
        if !idle && busy_since.elapsed() >= TURN_TIMEOUT {
            w.timed_out.store(true, Ordering::SeqCst);
            terminate(w.pid, w.exited);
            return;
        }
        // Idle time counts from the last reply, as idle compaction counts it.
        let retire = idle
            && w.server.store.thread(&w.thread_id).is_some_and(|t| {
                let finished = t.messages.last().and_then(|m| m.finished_at).unwrap_or(0);
                let idle_ms = u64::try_from(IDLE_LIMIT.as_millis()).unwrap_or(u64::MAX);
                now_ms().saturating_sub(finished) >= idle_ms
                    && t.messages
                        .iter()
                        .all(|m| m.background.iter().all(|task| !task.status.is_live()))
            });
        if retire {
            w.server.runner.retire_idle(&w.thread_id);
            return;
        }
    }
}

/// Everything the stdout reader needs to persist and publish one turn's events.
pub(in crate::orchestration) struct Turn {
    pub(in crate::orchestration) server: Arc<Server>,
    pub(in crate::orchestration) thread_id: String,
    pub(in crate::orchestration) message_id: String,
    pub(in crate::orchestration) agent: AgentKind,
    pub(in crate::orchestration) cancelled: Arc<AtomicBool>,
    pub(in crate::orchestration) interrupts: Arc<Interrupts>,
    /// Set when the turn ran past `TURN_TIMEOUT` and the agent was stopped for it.
    pub(in crate::orchestration) timed_out: Arc<AtomicBool>,
    pub(in crate::orchestration) exited: Arc<AtomicBool>,
    /// The files this reply changed, as far as they are known yet.
    pub(in crate::orchestration) attribution: SharedAttribution,
    /// The agent's systemd scope, when agents run in scopes.
    pub(in crate::orchestration) scope: Option<String>,
    pub(in crate::orchestration) stdin: Stdin,
    pub(in crate::orchestration) pending: Pending,
    pub(in crate::orchestration) native_turn: NativeTurn,
    pub(in crate::orchestration) steers: Steers,
    pub(in crate::orchestration) queue: Queue,
    pub(in crate::orchestration) tasks: Tasks,
    pub(in crate::orchestration) controls: Controls,
    pub(in crate::orchestration) idle: Arc<AtomicBool>,
    pub(in crate::orchestration) root: String,
    pub(in crate::orchestration) session_id: Option<String>,
    pub(in crate::orchestration) model: Option<String>,
    pub(in crate::orchestration) vision_model: Option<String>,
    pub(in crate::orchestration) text: String,
    pub(in crate::orchestration) mode: AccessMode,
    pub(in crate::orchestration) plan: bool,
    pub(in crate::orchestration) effort: Option<String>,
    pub(in crate::orchestration) compact_window: u64,
    pub(in crate::orchestration) attachments: Vec<Attachment>,
    pub(in crate::orchestration) images: Vec<provider::Image>,
    /// What `spec` passes as `TurnSpec::instructions`.
    pub(in crate::orchestration) instructions: Option<String>,
    /// What `spec` passes as `TurnSpec::skills_dir`.
    pub(in crate::orchestration) skills_dir: Option<String>,
    /// What `spec` passes as `TurnSpec::guarded`.
    pub(in crate::orchestration) guarded: Vec<crate::permissions::Guarded>,
    /// What `spec` passes as `TurnSpec::agents`.
    pub(in crate::orchestration) agents: Vec<crate::agents::Agent>,
    /// What `spec` passes as `TurnSpec::hooks`.
    pub(in crate::orchestration) hooks: Vec<crate::plugins::AgentHook>,
}

impl Turn {
    pub(in crate::orchestration) fn publish(&self, event: AgentEvent) {
        // Changes that waited for a call to cover them, which this finished one does.
        let covered = match &event {
            AgentEvent::ToolStarted { tool, .. } => {
                let reach = Reach::of(tool);
                attribution::lock(&self.attribution).tool_started(&tool.id, tool.started_at, reach);
                None
            }
            AgentEvent::ToolFinished {
                tool_id,
                message_id,
                ..
            } => {
                let paths = attribution::lock(&self.attribution).tool_finished(tool_id);
                Some((message_id.clone(), paths))
            }
            // A background task writes until it ends, after its call has returned.
            AgentEvent::BackgroundTaskStarted { task, .. } => {
                let reach = Reach::Anything(task.description.clone());
                let id = background_id(&task.tool_id);
                attribution::lock(&self.attribution).tool_started(&id, None, reach);
                None
            }
            AgentEvent::BackgroundTaskEnded { message_id, task } => {
                let id = background_id(&task.tool_id);
                let paths = attribution::lock(&self.attribution).tool_finished(&id);
                Some((message_id.clone(), paths))
            }
            _ => None,
        };
        publish(&self.server, &self.thread_id, event);
        if let Some((message_id, paths)) = covered {
            for path in paths {
                let touched = AgentEvent::FileTouched {
                    message_id: message_id.clone(),
                    path,
                };
                publish(&self.server, &self.thread_id, touched);
            }
        }
    }

    /// Starts a new assistant message for what the agent says after taking a steering
    /// message, so the answer follows that message instead of growing the reply above it.
    pub(in crate::orchestration) fn next_segment(&mut self, unsaved: &mut Unsaved) {
        self.save_text(unsaved);
        let message = assistant_message();
        self.message_id = message.id.clone();
        self.publish(AgentEvent::MessageStarted { message });
    }

    /// Closes stdin, which lets the agent exit, unless it still owes an answer to a
    /// steering message.
    pub(in crate::orchestration) fn close_unless_steered(&self) {
        close_stdin_unless_steered(&self.stdin, &self.steers);
    }

    pub(in crate::orchestration) fn spec(&self) -> TurnSpec<'_> {
        TurnSpec {
            root: &self.root,
            session_id: self.session_id.as_deref(),
            model: self.model.as_deref(),
            vision_model: self.vision_model.as_deref(),
            text: &self.text,
            mode: self.mode,
            plan: self.plan,
            effort: self.effort.as_deref(),
            compact_window: self.compact_window,
            attachments: &self.attachments,
            images: &self.images,
            instructions: self.instructions.as_deref(),
            skills_dir: self.skills_dir.as_deref(),
            guarded: &self.guarded,
            agents: &self.agents,
            hooks: &self.hooks,
        }
    }
}

/// An empty assistant message the agent's output streams into.
pub(in crate::orchestration) fn assistant_message() -> Message {
    Message {
        id: new_id(),
        role: Role::Assistant,
        text: String::new(),
        created_at: now_ms(),
        status: MessageStatus::Streaming,
        tools: vec![],
        plan: vec![],
        files: vec![],
        error: None,
        approvals: vec![],
        attachments: vec![],
        steer: false,
        queued: false,
        background: Vec::new(),
        cut_off: false,
        refusal: None,
        parts: Vec::new(),
        finished_at: None,
        usage: None,
        turn: None,
    }
}
