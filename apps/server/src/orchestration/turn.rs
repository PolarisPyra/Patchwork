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

use serde_json::Value;

use crate::provider::TurnSpec;
use crate::provider;
use crate::scope;
use crate::Server;
use crate::types::AccessMode;
use crate::types::AgentEvent;
use crate::types::AgentKind;
use crate::types::Attachment;
use crate::types::FileChange;
use crate::types::Goal;
use crate::types::GoalStatus;
use crate::types::Message;
use crate::types::MessageStatus;
use crate::types::new_id;
use crate::types::now_ms;
use crate::types::Role;
use crate::workspace;
use super::FollowUpInput;
use super::checkpoint;
use super::claude;
use super::codex;
use super::ingest::ingest;
use super::process::close_stdin_unless_steered;
use super::process::read_stderr_tail;
use super::process::Stdin;
use super::process::terminate;
use super::process::TURN_TIMEOUT;
use super::process::write_line;
use super::queue::Queue;
use super::queue::Steers;
use super::Runner;
use super::stream::publish;
use super::stream::Unsaved;
use super::subagents::Controls;
use super::subagents::Tasks;


/// Pending approvals of one turn: approval id → its message and the provider's reply context.
pub(super) type Pending = Arc<Mutex<HashMap<String, (String, Value)>>>;

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
        let (agent, root, session_id, model, mode, effort, plan, context, checkpoint) = {
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
                Some(id) => settings
                    .projects
                    .iter()
                    .find(|p| &p.id == id)
                    .map(|p| p.path.clone())
                    .ok_or("Project not found")?,
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
                // Project files are snapshotted before the newest user message's turn changes them.
                thread.project_id.and(
                    thread.messages.iter().rev().find(|m| m.role == Role::User).map(|m| m.id.clone()),
                ),
            )
        };
        let sends_context = context.is_some();
        let text = match context {
            Some(context) => format!("{context}{text}"),
            None => text,
        };
        let settings = server.store.settings();
        // Codex takes its instructions in the turn's input; Claude takes them as launch arguments.
        let instructions = match agent {
            AgentKind::Codex => codex::developer_instructions(&settings),
            AgentKind::Claude | AgentKind::Antigravity | AgentKind::OpenCode => None,
        };
        let spec = TurnSpec {
            root: &root,
            session_id: session_id.as_deref(),
            model: model.as_deref(),
            text: &text,
            mode,
            plan,
            effort: effort.as_deref(),
            attachments: &attachments,
            images: &images,
            instructions: instructions.as_deref(),
        };

        // Agents often edit files through the shell, so changes are also found by
        // comparing git state before and after the turn.
        let baseline = workspace::git_changes(&root).unwrap_or_default();
        if let Some(message_id) = &checkpoint {
            checkpoint::capture(&root, &thread_id, message_id);
        }
        let mut args = provider::turn_args(agent, &spec);
        if agent == AgentKind::Claude {
            args.extend(claude::launch_args(&settings));
        }
        let input = provider::turn_input(agent, &spec);
        let (mut cmd, scope) = scope::scoped(provider::turn_command(agent)?);
        if let Some(shell) = crate::effective_shell(server.store.settings().shell.as_deref()) {
            cmd.env("SHELL", shell);
        }
        cmd.args(&args)
            .envs(provider::turn_env(agent, &spec))
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
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            return Err("Agent output is unavailable".into());
        };
        // The agent has the transcript now, so later turns resume its session instead.
        if sends_context {
            server.store.update_thread(&thread_id, |t| t.context_pending = false)?;
        }
        let stderr_tail = std::thread::spawn(move || read_stderr_tail(stderr));
        let stdin: Stdin = Arc::new(Mutex::new(child.stdin.take()));
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
            baseline,
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
            text,
            mode,
            plan,
            effort,
            attachments,
            images,
            instructions,
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
        let (pid, exited) = (child.id(), turn.exited.clone());
        std::thread::spawn(move || {
            std::thread::sleep(TURN_TIMEOUT);
            if !exited.load(Ordering::SeqCst) {
                timed_out.store(true, Ordering::SeqCst);
                terminate(pid, exited);
            }
        });
        std::thread::spawn(move || ingest(turn, child, stdout, stderr_tail));
        Ok(())
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
    /// `git status` of the project when the turn started.
    pub(super) baseline: Vec<FileChange>,
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
    pub(super) text: String,
    pub(super) mode: AccessMode,
    pub(super) plan: bool,
    pub(super) effort: Option<String>,
    pub(super) attachments: Vec<Attachment>,
    pub(super) images: Vec<provider::Image>,
    /// What `spec` passes as `TurnSpec::instructions`.
    pub(super) instructions: Option<String>,
}

impl Turn {
    pub(super) fn publish(&self, event: AgentEvent) {
        publish(&self.server, &self.thread_id, event);
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
            text: &self.text,
            mode: self.mode,
            plan: self.plan,
            effort: self.effort.as_deref(),
            attachments: &self.attachments,
            images: &self.images,
            instructions: self.instructions.as_deref(),
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
        // Queued messages stay in the thread as queued until the user presses Send.
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

    // The agent drops the goal in a turn of its own; unanswered steers wait for that
    // system turn instead. Queued messages remain for the user to send explicitly.
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
    }
}
