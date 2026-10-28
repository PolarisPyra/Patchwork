//! One running turn: its shared state, and the spawn that starts it.
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

use super::attribution::{self, Attribution, Reach, SharedAttribution};
use crate::permissions::Target;
use crate::provider::TurnSpec;
use crate::provider;
use crate::scope;
use crate::Server;
use crate::types::AccessMode;
use crate::types::AgentEvent;
use crate::types::AgentKind;
use crate::types::Attachment;
use crate::types::Goal;
use crate::types::GoalStatus;
use crate::types::Message;
use crate::types::MessageStatus;
use crate::types::new_id;
use crate::types::now_ms;
use crate::types::Role;
use crate::debug_log::{AGENTS, Data, Source};
use crate::types::LogLevel;
use crate::workspace;
use super::FollowUpInput;
use super::checkpoint;
use super::claude;
use super::codex;
use super::ingest::ingest;
use super::process::AgentStdin;
use super::process::close_stdin_unless_steered;
use super::process::read_stderr_tail;
use super::process::Stdin;
use super::process::terminate;
use super::process::IDLE_LIMIT;
use super::process::TURN_TIMEOUT;
use super::process::WATCH_EVERY;
use super::process::write_line;
use super::queue::Queue;
use super::queue::Steers;
use super::Runner;
use super::stream::publish;
use super::stream::Unsaved;
use super::subagents::Controls;
use super::subagents::Tasks;


/// An approval waiting for the user.
pub(super) struct PendingApproval {
    pub(super) message_id: String,
    /// What `approval_reply` needs to answer it.
    pub(super) reply: Value,
    /// What it asks to do, for the permission rules.
    pub(super) target: Option<Target>,
    /// The folder the thread works in.
    pub(super) root: String,
}

/// Pending approvals of one turn, by approval id.
pub(super) type Pending = Arc<Mutex<HashMap<String, PendingApproval>>>;

/// The agent's id for the turn now running (Codex reports it once the turn starts).
pub(super) type NativeTurn = Arc<Mutex<Option<String>>>;

pub(super) struct RunningTurn {
    pub(super) pid: u32,
    pub(super) cancelled: Arc<AtomicBool>,
    /// Set once the agent process has exited and been reaped.
    pub(super) exited: Arc<AtomicBool>,
    /// The systemd scope the agent runs in, when scopes are on.
    pub(super) scope: Option<String>,
    pub(super) agent: AgentKind,
    pub(super) stdin: Stdin,
    pub(super) pending: Pending,
    pub(super) native_turn: NativeTurn,
    pub(super) steers: Steers,
    pub(super) queue: Queue,
    pub(super) tasks: Tasks,
    pub(super) controls: Controls,
    /// The agent has finished its reply and stays up only for shell commands it left running;
    /// the thread counts as idle, and the next message starts a new reply in this process.
    pub(super) idle: Arc<AtomicBool>,
    /// Shared with the project's watcher, which adds files to the reply as they change.
    pub(super) attribution: SharedAttribution,
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
        let TurnRequest { thread_id, text, attachments, images } = request;
        // A turn of its own (a /goal, a compaction) replaces an idle agent process.
        self.retire_idle(&thread_id);
        let (agent, root, session_id, model, mode, effort, plan, context, project_id, checkpoint) = {
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
                thread.plan_mode,
                context,
                thread.project_id.clone(),
                // Project files are snapshotted before the newest user message's turn changes them.
                thread.project_id.and(
                    thread.messages.iter().rev().find(|m| m.role == Role::User).map(|m| m.id.clone()),
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
        // Codex takes its instructions in the turn's input; Claude takes them as launch arguments.
        let instructions = match agent {
            AgentKind::Codex => codex::developer_instructions(&settings),
            // Patchwork's own agent has no skill loader, so it is told where the skills are.
            AgentKind::Api => {
                let parts = [codex::developer_instructions(&settings), crate::skills::instructions(&skills)];
                Some(parts.into_iter().flatten().collect::<Vec<_>>().join("\n\n")).filter(|t| !t.is_empty())
            }
            AgentKind::Claude | AgentKind::Antigravity | AgentKind::OpenCode => None,
        };
        let skills_dir = Some(crate::skills::loaded_dir(&settings).to_string_lossy().into_owned());
        let guarded = crate::permissions::guarded(&settings.permissions, project_id.as_deref());
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
            attachments: &attachments,
            images: &images,
            instructions: instructions.as_deref(),
            skills_dir: skills_dir.as_deref(),
            guarded: &guarded,
            agents: &agents,
        };

        // Agents often edit files through the shell, so changes are also found by
        // comparing git state before and after the turn.
        let attribution = Attribution::shared(workspace::git_changes(&root).unwrap_or_default());
        plugin_hooks(server, "userPromptSubmit", &thread_id, &root, agent, None);
        if let Some(message_id) = &checkpoint {
            checkpoint::capture(&root, &thread_id, message_id);
        }
        let mut args = provider::turn_args(agent, &spec);
        if agent == AgentKind::Claude {
            args.extend(claude::launch_args(&settings));
            args.extend(claude::permission_args(&guarded, &root));
            args.extend(claude::agents_args(&agents, &root));
        }
        let input = provider::turn_input(agent, &spec);
        let (mut cmd, scope) = scope::scoped(provider::turn_command(agent)?);
        if let Some(shell) = crate::effective_shell(server.store.settings().shell.as_deref()) {
            cmd.env("SHELL", shell);
        }
        if agent == AgentKind::Claude {
            cmd.envs(claude::launch_env(&settings));
        }
        cmd.args(&args)
            .envs(provider::turn_env(agent, &spec))
            .envs(provider::hook::env(agent, &thread_id, &root))
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
                if session_id.is_some() { "(resumed)" } else { "" },
            );
            let source = Source::agent(agent, pid, &thread_id, None);
            AGENTS.write(LogLevel::Info, "start", &source, Data::Text(started.trim_end().into()));
            log_cli_version(agent);
        }
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            return Err("Agent output is unavailable".into());
        };
        // The agent has the transcript now, so later turns resume its session instead.
        if sends_context {
            server.store.update_thread(&thread_id, |t| t.context_pending = false)?;
        }
        let source = Source::agent(agent, pid, &thread_id, None);
        let stderr_tail = std::thread::spawn(move || read_stderr_tail(stderr, source));
        let stdin: Stdin = Arc::new(Mutex::new(AgentStdin::take(&mut child, agent, &thread_id)));
        input.iter().for_each(|line| {
            write_line(&stdin, line);
        });

        let message = assistant_message();
        let cancelled = Arc::new(AtomicBool::new(false));
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
            attachments,
            images,
            instructions,
            skills_dir,
            guarded,
            agents,
        };
        self.turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                thread_id,
                RunningTurn {
                    pid: child.id(),
                    cancelled,
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
                    && t.messages.iter().all(|m| m.background.is_empty())
            });
        if retire {
            w.server.runner.retire_idle(&w.thread_id);
            return;
        }
    }
}

/// Everything the stdout reader needs to persist and publish one turn's events.
pub(super) struct Turn {
    pub(super) server: Arc<Server>,
    pub(super) thread_id: String,
    pub(super) message_id: String,
    pub(super) agent: AgentKind,
    pub(super) cancelled: Arc<AtomicBool>,
    /// Set when the turn ran past `TURN_TIMEOUT` and the agent was stopped for it.
    pub(super) timed_out: Arc<AtomicBool>,
    pub(super) exited: Arc<AtomicBool>,
    /// The files this reply changed, as far as they are known yet.
    pub(super) attribution: SharedAttribution,
    /// The agent's systemd scope, when agents run in scopes.
    pub(super) scope: Option<String>,
    pub(super) stdin: Stdin,
    pub(super) pending: Pending,
    pub(super) native_turn: NativeTurn,
    pub(super) steers: Steers,
    pub(super) queue: Queue,
    pub(super) tasks: Tasks,
    pub(super) controls: Controls,
    pub(super) idle: Arc<AtomicBool>,
    pub(super) root: String,
    pub(super) session_id: Option<String>,
    pub(super) model: Option<String>,
    pub(super) vision_model: Option<String>,
    pub(super) text: String,
    pub(super) mode: AccessMode,
    pub(super) plan: bool,
    pub(super) effort: Option<String>,
    pub(super) attachments: Vec<Attachment>,
    pub(super) images: Vec<provider::Image>,
    /// What `spec` passes as `TurnSpec::instructions`.
    pub(super) instructions: Option<String>,
    /// What `spec` passes as `TurnSpec::skills_dir`.
    pub(super) skills_dir: Option<String>,
    /// What `spec` passes as `TurnSpec::guarded`.
    pub(super) guarded: Vec<crate::permissions::Guarded>,
    /// What `spec` passes as `TurnSpec::agents`.
    pub(super) agents: Vec<crate::agents::Agent>,
}

impl Turn {
    pub(super) fn publish(&self, event: AgentEvent) {
        // Changes that waited for a call to cover them, which this finished one does.
        let covered = match &event {
            AgentEvent::ToolStarted { tool, .. } => {
                let reach = Reach::of(tool);
                attribution::lock(&self.attribution).tool_started(&tool.id, tool.started_at, reach);
                None
            }
            AgentEvent::ToolFinished { tool_id, message_id, .. } => {
                let paths = attribution::lock(&self.attribution).tool_finished(tool_id);
                Some((message_id.clone(), paths))
            }
            // A background command writes until it ends, after its call has returned.
            AgentEvent::BackgroundTaskStarted { task, .. } => {
                let reach = Reach::Anything(task.description.clone());
                let id = background_id(&task.tool_id);
                attribution::lock(&self.attribution).tool_started(&id, None, reach);
                None
            }
            AgentEvent::BackgroundTaskEnded { tool_id, message_id } => {
                let id = background_id(tool_id);
                let paths = attribution::lock(&self.attribution).tool_finished(&id);
                Some((message_id.clone(), paths))
            }
            _ => None,
        };
        publish(&self.server, &self.thread_id, event);
        if let Some((message_id, paths)) = covered {
            for path in paths {
                let touched = AgentEvent::FileTouched { message_id: message_id.clone(), path };
                publish(&self.server, &self.thread_id, touched);
            }
        }
    }

    /// Starts a new assistant message for what the agent says after taking a steering
    /// message, so the answer follows that message instead of growing the reply above it.
    pub(super) fn next_segment(&mut self, unsaved: &mut Unsaved) {
        self.save_text(unsaved);
        let message = assistant_message();
        self.message_id = message.id.clone();
        self.publish(AgentEvent::MessageStarted { message });
    }

    /// Closes stdin, which lets the agent exit, unless it still owes an answer to a
    /// steering message.
    pub(super) fn close_unless_steered(&self) {
        close_stdin_unless_steered(&self.stdin, &self.steers);
    }

    pub(super) fn spec(&self) -> TurnSpec<'_> {
        TurnSpec {
            root: &self.root,
            session_id: self.session_id.as_deref(),
            model: self.model.as_deref(),
            vision_model: self.vision_model.as_deref(),
            text: &self.text,
            mode: self.mode,
            plan: self.plan,
            effort: self.effort.as_deref(),
            attachments: &self.attachments,
            images: &self.images,
            instructions: self.instructions.as_deref(),
            skills_dir: self.skills_dir.as_deref(),
            guarded: &self.guarded,
            agents: &self.agents,
        }
    }
}

/// An empty assistant message the agent's output streams into.
pub(super) fn assistant_message() -> Message {
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

/// Ends a turn whose agent has exited: forgets it, completes each of its messages, then starts
/// the turn that drops its goal or answers the steering messages it never took. When the agent lost
/// the session the turn resumed (`session_lost`), the turn runs again in a new session instead,
/// given the conversation so far as a transcript.
/// How an agent's turn ended, as `finish` needs to record it.
pub(super) struct TurnEnd {
    pub(super) status: MessageStatus,
    pub(super) error: Option<String>,
    pub(super) cancelled: bool,
    pub(super) clear_in_agent: Option<&'static str>,
    pub(super) session_lost: bool,
}

/// Pairs the turn's replies with the project's snapshots from before and after the turn, so their
/// changed files keep showing what the turn did once the changes are committed or edited again.
pub(super) fn record_turn(turn: &Turn, replies: &[String]) {
    let Some(first) = replies.first() else { return };
    let Some(thread) = turn.server.store.thread(&turn.thread_id) else { return };
    if thread.project_id.is_none() {
        return;
    }
    // The snapshot before the turn belongs to the user message the first reply answers.
    let Some(at) = thread.messages.iter().position(|m| &m.id == first) else { return };
    let Some(asked) = thread.messages[..at].iter().rev().find(|m| m.role == Role::User) else {
        return;
    };
    if let Some(trees) = checkpoint::record_end(&turn.root, &turn.thread_id, &asked.id, replies) {
        for id in replies {
            turn.publish(AgentEvent::TurnRecorded { message_id: id.clone(), turn: trees.clone() });
        }
    }
}

pub(super) fn finish(turn: Turn, segments: &[String], end: TurnEnd) {
    let TurnEnd { status, error, cancelled, clear_in_agent, session_lost } = end;
    // Forget the turn before announcing it. Queued messages remain persisted in the
    // thread until the user explicitly resumes them; unanswered steers were already
    // delivered to the agent and continue automatically if needed.
    let unanswered = {
        let mut turns = turn
            .server
            .runner
            .turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // A retired idle turn may already have been replaced by a new one.
        if turns
            .get(&turn.thread_id)
            .is_some_and(|t| Arc::ptr_eq(&t.exited, &turn.exited))
        {
            turns.remove(&turn.thread_id);
        }
        // Queued messages stay in the thread as queued until the turn has finished.
        // Steering messages the agent never answered continue automatically unless
        // the turn was stopped; messages sent after Stop still start the next turn.
        let unanswered = turn
            .steers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
            .collect::<Vec<_>>();
        let queued = std::mem::take(
            &mut *turn.queue.lock().unwrap_or_else(PoisonError::into_inner),
        );
        let mut handed_over = turn
            .server
            .runner
            .handed_over
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        queued.iter().for_each(|message| {
            handed_over.remove(&message.message_id);
        });
        if cancelled { Vec::new() } else { unanswered }
    };
    if session_lost && unanswered.is_empty() {
        let server = &turn.server;
        let reset = server.store.update_thread(&turn.thread_id, |t| {
            t.session_id = None;
            t.context_pending = true;
        });
        if let Err(e) = reset {
            log::warn!("failed to drop a lost session: {e}");
        } else {
            turn.publish(AgentEvent::MessageRemoved { message_id: turn.message_id.clone() });
            let (thread_id, text) = (turn.thread_id.clone(), turn.text.clone());
            let (attachments, images) = (turn.attachments.clone(), turn.images.clone());
            let started = server.runner.start_turn(
                server,
                TurnRequest { thread_id, text, attachments, images },
            );
            match started {
                Ok(()) => return,
                Err(e) => log::warn!("failed to rerun the turn in a new session: {e}"),
            }
        }
    }
    record_turn(&turn, segments);
    // Earlier messages of the turn were answered in full before a steer moved it on. None are
    // left when the agent went idle after its last reply, which was completed then.
    if let Some((last, earlier)) = segments.split_last() {
        earlier.iter().for_each(|id| {
            turn.publish(AgentEvent::TurnCompleted {
                message_id: id.clone(),
                status: MessageStatus::Complete,
                error: None,
            });
        });
        turn.publish(AgentEvent::TurnCompleted {
            message_id: last.clone(),
            status,
            error,
        });
    }
    // On a thread of its own, so a slow hook does not hold up the next turn.
    {
        let (server, thread_id, root) =
            (turn.server.clone(), turn.thread_id.clone(), turn.root.clone());
        let (agent, ended) = (turn.agent, if cancelled { "interrupt" } else { "stop" });
        std::thread::spawn(move || {
            for event in [ended, "sessionEnd"] {
                plugin_hooks(&server, event, &thread_id, &root, agent, Some(status));
            }
        });
    }

    // The agent drops the goal in a turn of its own; unanswered steers wait for that
    // system turn instead, and queued messages for the end of it.
    if let Some(command) = clear_in_agent {
        let server = &turn.server;
        match server.send_message(turn.thread_id.clone(), command.into(), vec![], false) {
            Ok(_) => {
                let runner = &server.runner;
                unanswered.iter().for_each(|f| {
                    let _ = runner.enqueue(
                        &turn.thread_id,
                        FollowUpInput {
                            message_id: &f.message_id,
                            text: &f.text,
                            attachments: &f.attachments,
                            images: &f.images,
                        },
                    );
                });
                return;
            }
            Err(e) => log::warn!("failed to clear the goal in the agent's session: {e}"),
        }
    }
    // Unanswered steers have already been delivered, so clear their pending UI state
    // before continuing them in a new turn.
    unanswered.iter().for_each(|f| {
        publish(
            &turn.server,
            &turn.thread_id,
            AgentEvent::MessageQueued {
                message_id: f.message_id.clone(),
                queued: false,
            },
        );
    });
    if !unanswered.is_empty() {
        let text = unanswered
            .iter()
            .map(|f| f.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let (attachments, images) = unanswered.into_iter().map(|f| (f.attachments, f.images)).fold(
            (vec![], vec![]),
            |(mut a, mut i), (fa, fi)| {
                a.extend(fa);
                i.extend(fi);
                (a, i)
            },
        );
        let server = &turn.server;
        if let Err(e) =
            server
                .runner
                .start_turn(
                    server,
                    TurnRequest { thread_id: turn.thread_id.clone(), text, attachments, images },
                )
        {
            log::warn!("failed to start the turn for queued messages: {e}");
        }
    } else if !cancelled && status == MessageStatus::Complete {
        // Queued messages go once the turn has finished; after Stop or an error they wait for
        // the user to send them.
        turn.server.send_queued_later(&turn.thread_id);
    }
}

/// Runs the enabled plugins' hooks for `event` and logs the ones that failed.
fn plugin_hooks(
    server: &Server,
    event: &str,
    thread_id: &str,
    root: &str,
    agent: AgentKind,
    status: Option<MessageStatus>,
) {
    let name = |value: serde_json::Result<Value>| {
        value.ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
    };
    let agent = name(serde_json::to_value(agent));
    let status = status.map(|status| name(serde_json::to_value(status)));
    let context = crate::plugins::HookContext {
        event,
        thread_id,
        root,
        agent: &agent,
        status: status.as_deref(),
    };
    for failure in crate::plugins::run_hooks(&server.store.settings(), &context) {
        log::warn!("plugin hook {event} {failure}");
    }
}

/// A background command's window, apart from its call's, which has the same id.
fn background_id(tool_id: &str) -> String {
    format!("background:{tool_id}")
}

/// Logs the agent's CLI version once per run of the app. A thread asks, since running the CLI
/// takes a moment the turn should not wait for.
fn log_cli_version(agent: AgentKind) {
    static LOGGED: Mutex<Vec<AgentKind>> = Mutex::new(Vec::new());
    if agent == AgentKind::Api {
        return;
    }
    {
        let mut logged = LOGGED.lock().unwrap_or_else(PoisonError::into_inner);
        if logged.contains(&agent) {
            return;
        }
        logged.push(agent);
    }
    std::thread::spawn(move || {
        let version = provider::cli_version(agent).unwrap_or_else(|| "unknown".into());
        let source = Source { agent: Some(agent), ..Source::default() };
        AGENTS.write(LogLevel::Info, "cli", &source, Data::Text(format!("{agent:?} {version}")));
    });
}
