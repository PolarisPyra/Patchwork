//! Runs agent turns by spawning the agent CLIs and translating their JSONL
//! output into `AgentEvent`s. Each turn is one child process; its stdin stays
//! open while the turn runs so approvals can be answered.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use crate::goal;
use crate::orchestration;
use crate::provider::{self, Parsed, TurnSpec};
use crate::scope;
use crate::types::*;
use crate::workspace;
use crate::{Res, Server};
use serde_json::Value;

/// How much of the agent's stderr is kept for error reporting.
const STDERR_TAIL_BYTES: usize = 4096;

/// How long a stopped agent gets to exit after SIGTERM before it is sent SIGKILL.
const KILL_AFTER: Duration = Duration::from_secs(5);

/// How long quitting the app waits for agents to exit after SIGTERM.
const QUIT_GRACE: Duration = Duration::from_secs(2);

/// The agent's stdin; None once the turn has finished.
type Stdin = Arc<Mutex<Option<ChildStdin>>>;

/// Pending approvals of one turn: approval id → its message and the provider's reply context.
type Pending = Arc<Mutex<HashMap<String, (String, Value)>>>;

/// A message sent during a turn, kept until the agent answers it or the turn ends.
struct FollowUp {
    /// The user message this is, so the thread can show it as queued or drop it.
    message_id: String,
    text: String,
    attachments: Vec<Attachment>,
    images: Vec<provider::Image>,
}

/// Steering messages written to the agent that it has not yet answered, oldest first.
type Steers = Arc<Mutex<VecDeque<FollowUp>>>;

/// Subagents the agent started: tool call id → the agent's own task id.
type Tasks = Arc<Mutex<HashMap<String, String>>>;

/// Messages the agent could not take mid-turn; they start the next turn.
type Queue = Arc<Mutex<Vec<FollowUp>>>;

/// What became of a message sent to a thread.
pub enum Delivery {
    /// Written to the running turn.
    Steered,
    /// Waiting for the running turn to end.
    Queued,
    /// No turn is running.
    NotRunning,
}

/// The agent's id for the turn now running (Codex reports it once the turn starts).
type NativeTurn = Arc<Mutex<Option<String>>>;

struct RunningTurn {
    pid: u32,
    cancelled: Arc<AtomicBool>,
    /// Set once the agent process has exited and been reaped.
    exited: Arc<AtomicBool>,
    agent: AgentKind,
    stdin: Stdin,
    pending: Pending,
    native_turn: NativeTurn,
    steers: Steers,
    queue: Queue,
    tasks: Tasks,
}

#[derive(Default)]
pub struct Runner {
    turns: Mutex<HashMap<String, RunningTurn>>,
    /// Messages this process has handed to a turn, whether steered or queued. Written under the
    /// `turns` lock, so a withdrawal sees it in the same order as the queue it checks.
    handed_over: Mutex<HashSet<String>>,
}

impl Runner {
    pub fn is_running(&self, thread_id: &str) -> bool {
        self.turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(thread_id)
    }

    pub fn start_turn(
        &self,
        server: &Arc<Server>,
        thread_id: String,
        text: String,
        attachments: Vec<Attachment>,
        images: Vec<provider::Image>,
    ) -> Result<(), String> {
        let (agent, root, session_id, model, mode, effort) = {
            let thread = server.store.thread(&thread_id).ok_or("Task not found")?;
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
            )
        };
        let spec = TurnSpec {
            root: &root,
            session_id: session_id.as_deref(),
            model: model.as_deref(),
            text: &text,
            mode,
            effort: effort.as_deref(),
            attachments: &attachments,
            images: &images,
        };

        // Agents often edit files through the shell, so changes are also found by
        // comparing git state before and after the turn.
        let baseline = workspace::git_changes(&root).unwrap_or_default();
        let mut args = provider::turn_args(agent, &spec);
        if agent == AgentKind::Claude {
            args.extend(orchestration::claude::launch_args(&server.store.settings()));
        }
        let input = provider::turn_input(agent, &spec);
        let mut cmd = scope::scoped(provider::agent_command(agent)?);
        if let Some(shell) = server.store.settings().shell {
            cmd.env("SHELL", shell);
        }
        cmd.args(&args)
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
        let stderr_tail = std::thread::spawn(move || read_stderr_tail(stderr));
        let stdin: Stdin = Arc::new(Mutex::new(child.stdin.take()));
        for line in &input {
            write_line(&stdin, line);
        }

        let message = assistant_message();
        let cancelled = Arc::new(AtomicBool::new(false));
        let exited = Arc::new(AtomicBool::new(false));
        let pending = Pending::default();
        let native_turn = NativeTurn::default();
        let steers = Steers::default();
        let queue = Queue::default();
        let tasks = Tasks::default();
        let turn = Turn {
            server: server.clone(),
            thread_id: thread_id.clone(),
            message_id: message.id.clone(),
            agent,
            cancelled: cancelled.clone(),
            exited: exited.clone(),
            baseline,
            stdin: stdin.clone(),
            pending: pending.clone(),
            native_turn: native_turn.clone(),
            steers: steers.clone(),
            queue: queue.clone(),
            tasks: tasks.clone(),
            root,
            session_id,
            model,
            text,
            mode,
            effort,
            attachments,
            images,
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
                    agent,
                    stdin,
                    pending,
                    native_turn,
                    steers,
                    queue,
                    tasks,
                },
            );
        turn.publish(AgentEvent::MessageStarted { message });
        std::thread::spawn(move || drive(turn, child, stdout, stderr_tail));
        Ok(())
    }

    /// Hands a message to the turn running in `thread_id`: written to the agent when it
    /// can take input mid-turn, otherwise kept to start the next turn.
    pub fn steer(
        &self,
        thread_id: &str,
        message_id: &str,
        session_id: Option<&str>,
        text: &str,
        attachments: &[Attachment],
        images: &[provider::Image],
    ) -> Delivery {
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(turn) = turns.get(thread_id) else {
            return Delivery::NotRunning;
        };
        let follow_up = FollowUp {
            message_id: message_id.into(),
            text: text.into(),
            attachments: attachments.to_vec(),
            images: images.to_vec(),
        };
        self.handed_over
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(message_id.into());
        // A stopped turn takes nothing more; the message starts the next turn.
        if turn.cancelled.load(Ordering::SeqCst) {
            turn.queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(follow_up);
            return Delivery::Queued;
        }
        let turn_id = turn
            .native_turn
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let line = provider::steer_input(
            turn.agent,
            text,
            attachments,
            images,
            session_id,
            turn_id.as_deref(),
        );
        // Under the stdin lock, so the reader cannot close stdin between the write and
        // recording that an answer is due.
        let mut stdin = turn.stdin.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(line) = line
            && write_to(&mut stdin, &line)
        {
            turn.steers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push_back(follow_up);
            return Delivery::Steered;
        }
        turn.queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(follow_up);
        Delivery::Queued
    }

    /// Holds a message for the next turn of the one running in `thread_id`, even when the agent
    /// could take it now. False when no turn is running there.
    pub fn enqueue(
        &self,
        thread_id: &str,
        message_id: &str,
        text: &str,
        attachments: &[Attachment],
        images: &[provider::Image],
    ) -> bool {
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(turn) = turns.get(thread_id) else {
            return false;
        };
        self.handed_over
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(message_id.into());
        turn.queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(FollowUp {
                message_id: message_id.into(),
                text: text.into(),
                attachments: attachments.to_vec(),
                images: images.to_vec(),
            });
        true
    }

    /// Whether the message is waiting in the queue of the turn running in `thread_id`.
    pub fn is_queued(&self, thread_id: &str, message_id: &str) -> bool {
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        turns.get(thread_id).is_some_and(|turn| {
            turn.queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .any(|f| f.message_id == message_id)
        })
    }

    /// Takes a queued message back so it is not sent. False if the agent has already been handed
    /// it. A message this process never handed over was queued before a restart, so nothing
    /// holds it and it counts as taken back.
    pub fn withdraw(&self, thread_id: &str, message_id: &str) -> bool {
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(turn) = turns.get(thread_id) {
            let mut queue = turn.queue.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(index) = queue.iter().position(|f| f.message_id == message_id) {
                queue.remove(index);
                return true;
            }
        }
        !self
            .handed_over
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(message_id)
    }

    /// Stops one subagent of the running turn, leaving the rest of the turn going.
    pub fn stop_subagent(&self, thread_id: &str, tool_id: &str) -> Result<(), String> {
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        let turn = turns
            .get(thread_id)
            .ok_or("This turn is no longer running")?;
        let line = turn
            .tasks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(tool_id)
            .and_then(|task| provider::stop_subagent(turn.agent, task))
            .ok_or("This subagent can't be stopped")?;
        if write_line(&turn.stdin, &line) {
            Ok(())
        } else {
            Err("This turn no longer accepts input".into())
        }
    }

    /// Stops the turn and drops the messages waiting for it. `last_line` is written
    /// to the agent first, with a moment to act on it.
    pub fn cancel(&self, thread_id: &str, last_line: Option<String>) -> Vec<String> {
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        let mut dropped = Vec::new();
        if let Some(turn) = turns.get(thread_id) {
            turn.cancelled.store(true, Ordering::SeqCst);
            dropped = turn
                .queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .drain(..)
                .map(|f| f.message_id)
                .collect();
            let (pid, exited) = (turn.pid, turn.exited.clone());
            if last_line.is_some_and(|line| write_line(&turn.stdin, &line)) {
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(300));
                    terminate(pid, exited);
                });
            } else {
                terminate(pid, exited);
            }
        }
        dropped
    }

    /// Stops every running agent, so none outlives the app when it quits. Agents that
    /// are still running after `QUIT_GRACE` are sent SIGKILL.
    pub fn stop_all(&self) {
        let running: Vec<(u32, Arc<AtomicBool>)> = self
            .turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .map(|turn| {
                turn.cancelled.store(true, Ordering::SeqCst);
                (turn.pid, turn.exited.clone())
            })
            .collect();
        for (pid, _) in &running {
            workspace::signal_process_group(*pid, libc::SIGTERM);
        }
        let deadline = Instant::now() + QUIT_GRACE;
        while Instant::now() < deadline && running.iter().any(|(_, e)| !e.load(Ordering::SeqCst)) {
            std::thread::sleep(Duration::from_millis(50));
        }
        for (pid, exited) in &running {
            if !exited.load(Ordering::SeqCst) {
                workspace::signal_process_group(*pid, libc::SIGKILL);
            }
        }
    }

    /// Sends the user's answer to a pending approval and records it.
    pub fn respond(
        &self,
        server: &Server,
        thread_id: &str,
        approval_id: &str,
        allow: bool,
    ) -> Result<(), String> {
        let (agent, stdin, (message_id, reply)) = {
            let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
            let turn = turns
                .get(thread_id)
                .ok_or("This turn is no longer running")?;
            let reply = turn
                .pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(approval_id)
                .ok_or("This request was already answered")?;
            (turn.agent, turn.stdin.clone(), reply)
        };
        write_line(&stdin, &provider::approval_reply(agent, &reply, allow));
        publish(
            server,
            thread_id,
            AgentEvent::ApprovalResolved {
                message_id: message_id.clone(),
                approval_id: approval_id.into(),
                allowed: allow,
            },
        );
        Ok(())
    }
}

/// Writes one line to the agent's stdin, if it is still open.
fn write_line(stdin: &Mutex<Option<ChildStdin>>, line: &str) -> bool {
    write_to(
        &mut stdin.lock().unwrap_or_else(PoisonError::into_inner),
        line,
    )
}

fn write_to(stdin: &mut Option<ChildStdin>, line: &str) -> bool {
    let Some(pipe) = stdin.as_mut() else {
        return false;
    };
    match writeln!(pipe, "{line}").and_then(|()| pipe.flush()) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("failed to write to agent: {e}");
            false
        }
    }
}

/// Saves `event` onto the stored message, then streams it to the frontend.
pub(crate) fn publish(server: &Server, thread_id: &str, event: AgentEvent) {
    if let Err(e) = save_event(server, thread_id, &event) {
        log::warn!("failed to save agent event: {e}");
    }
    server.emit(AgentEventEnvelope {
        thread_id: thread_id.into(),
        event,
    });
}

/// Sends SIGTERM to the agent's process group, then SIGKILL if the agent has not exited
/// after `KILL_AFTER`.
fn terminate(pid: u32, exited: Arc<AtomicBool>) {
    workspace::signal_process_group(pid, libc::SIGTERM);
    std::thread::spawn(move || {
        std::thread::sleep(KILL_AFTER);
        if !exited.load(Ordering::SeqCst) {
            workspace::signal_process_group(pid, libc::SIGKILL);
        }
    });
}

/// Why the agent's process ended, when it left no error message of its own.
fn exit_error(exit: Option<ExitStatus>) -> String {
    if let Some(code) = exit.and_then(|s| s.code()) {
        return format!("Agent exited with code {code}");
    }
    // systemd sends SIGKILL to an agent whose scope ran out of memory.
    #[cfg(unix)]
    if exit.and_then(|s| std::os::unix::process::ExitStatusExt::signal(&s)) == Some(libc::SIGKILL) {
        return "The agent was killed, likely for using too much memory".into();
    }
    "Agent exited unexpectedly".into()
}

/// A model request the agent made during a turn, and the tokens it used.
struct Request {
    id: String,
    /// The message that was current when the request was reported.
    message_id: String,
    /// False for a subagent's request, which does not count toward the context.
    top_level: bool,
    usage: TokenUsage,
}

/// The tokens a message used, summed over its requests. Its context is the prompt of the
/// last request the main agent made for it.
fn message_usage(requests: &[Request], message_id: &str) -> TokenUsage {
    let mut total = TokenUsage::default();
    for r in requests.iter().filter(|r| r.message_id == message_id) {
        total.input += r.usage.input;
        total.cache_read += r.usage.cache_read;
        total.cache_write += r.usage.cache_write;
        total.output += r.usage.output;
        if r.top_level {
            total.context = r.usage.context;
        }
    }
    total
}

/// Closes stdin, which lets the agent exit, unless it still owes an answer to a steering message.
fn close_stdin_unless_steered(stdin: &Stdin, steers: &Steers) {
    let mut stdin = stdin.lock().unwrap_or_else(PoisonError::into_inner);
    if steers
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_empty()
    {
        stdin.take();
    }
}

/// How long Codex has to start its next turn toward an active goal after one ends. It starts
/// one within milliseconds, so a longer wait means the goal is not being pursued.
const CODEX_GOAL_GRACE: Duration = Duration::from_secs(5);

/// Ends the agent's input once its turn is over. While Codex pursues a goal, the input stays
/// open for the grace window: a turn started inside it continues the run, and otherwise the
/// input closes. Starting a turn advances `round`, which cancels a close still waiting.
fn end_input(turn: &Turn, goal_run: bool, round: &Arc<AtomicU64>) {
    if !goal_run {
        close_stdin_unless_steered(&turn.stdin, &turn.steers);
        return;
    }
    let armed = round.fetch_add(1, Ordering::SeqCst) + 1;
    let (round, stdin, steers) = (round.clone(), turn.stdin.clone(), turn.steers.clone());
    std::thread::spawn(move || {
        std::thread::sleep(CODEX_GOAL_GRACE);
        if round.load(Ordering::SeqCst) == armed {
            close_stdin_unless_steered(&stdin, &steers);
        }
    });
}

/// Everything the stdout reader needs to persist and publish one turn's events.
struct Turn {
    server: Arc<Server>,
    thread_id: String,
    message_id: String,
    agent: AgentKind,
    cancelled: Arc<AtomicBool>,
    exited: Arc<AtomicBool>,
    /// `git status` of the project when the turn started.
    baseline: Vec<FileChange>,
    stdin: Stdin,
    pending: Pending,
    native_turn: NativeTurn,
    steers: Steers,
    queue: Queue,
    tasks: Tasks,
    root: String,
    session_id: Option<String>,
    model: Option<String>,
    text: String,
    mode: AccessMode,
    effort: Option<String>,
    attachments: Vec<Attachment>,
    images: Vec<provider::Image>,
}

impl Turn {
    fn publish(&self, event: AgentEvent) {
        publish(&self.server, &self.thread_id, event);
    }

    /// Starts a new assistant message for what the agent says after taking a steering
    /// message, so the answer follows that message instead of growing the reply above it.
    fn next_segment(&mut self, unsaved: &mut String) {
        self.save_text(unsaved);
        let message = assistant_message();
        self.message_id = message.id.clone();
        self.publish(AgentEvent::MessageStarted { message });
    }

    /// Closes stdin, which lets the agent exit, unless it still owes an answer to a
    /// steering message.
    fn close_unless_steered(&self) {
        close_stdin_unless_steered(&self.stdin, &self.steers);
    }

    /// Streams a text delta to the frontend without saving it; the reader
    /// collects it in `unsaved` so it is stored in one write later.
    fn stream_text(&self, event: AgentEvent, unsaved: &mut String) {
        if let AgentEvent::TextDelta { text, .. } = &event {
            unsaved.push_str(text);
        }
        self.server.emit(AgentEventEnvelope {
            thread_id: self.thread_id.clone(),
            event,
        });
    }

    /// Saves text that was streamed but not yet stored.
    fn save_text(&self, unsaved: &mut String) {
        if unsaved.is_empty() {
            return;
        }
        let event = AgentEvent::TextDelta {
            message_id: self.message_id.clone(),
            text: std::mem::take(unsaved),
        };
        if let Err(e) = save_event(&self.server, &self.thread_id, &event) {
            log::warn!("failed to save agent text: {e}");
        }
    }

    fn spec(&self) -> TurnSpec<'_> {
        TurnSpec {
            root: &self.root,
            session_id: self.session_id.as_deref(),
            model: self.model.as_deref(),
            text: &self.text,
            mode: self.mode,
            effort: self.effort.as_deref(),
            attachments: &self.attachments,
            images: &self.images,
        }
    }
}

/// An empty assistant message the agent's output streams into.
fn assistant_message() -> Message {
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
        thinking: String::new(),
        finished_at: None,
        usage: None,
    }
}

/// Reads the agent's stdout until it closes, then records the final status.
fn drive(mut turn: Turn, mut child: Child, stdout: ChildStdout, stderr_tail: JoinHandle<String>) {
    let mut has_text = false;
    // Antigravity streams one text block as many deltas; other agents send whole blocks.
    let mut last_was_text = false;
    // Tool id → the message it was started in, which a steer may since have moved on from.
    let mut tool_owner: HashMap<String, String> = HashMap::new();
    // Subagents still running, and whether the call of one sent to the background, which
    // returns at once, failed; it is reported finished when the subagent ends.
    let mut running_tasks: HashMap<String, Option<bool>> = HashMap::new();
    // Background shell commands the agent started itself: tool call id → the message they are shown in.
    let mut shell_tasks: HashMap<String, String> = HashMap::new();
    // Every message of the turn: the first reply, then one per steering message taken.
    let mut segments = vec![turn.message_id.clone()];
    let mut stream_error: Option<String> = None;
    let mut goal = turn
        .server
        .store
        .thread(&turn.thread_id)
        .and_then(|t| t.goal);
    // The agent reported its turn finished; Codex may start another for an active goal.
    let mut ended = false;
    // Codex reported this run's goal active, so it starts further turns toward it.
    let mut goal_run = false;
    // Bumped whenever a turn starts, so a grace window that is still waiting does not close stdin.
    let goal_round = Arc::new(AtomicU64::new(0));
    // Text streamed to the frontend but not yet stored; saving every delta
    // stalls the reader, so it is stored before the next event and at the end.
    let mut unsaved = String::new();
    // Every model request the turn has made; each reply's usage is summed from these.
    let mut requests: Vec<Request> = Vec::new();

    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { continue };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        // A message Claude took after it had already ended its reply (while a background
        // subagent kept it running) is answered from here on, below that message.
        if ended
            && turn.agent == AgentKind::Claude
            && !turn
                .steers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_empty()
        {
            turn.steers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clear();
            turn.next_segment(&mut unsaved);
            segments.push(turn.message_id.clone());
            (has_text, last_was_text) = (false, false);
            ended = false;
        }
        let parsed = provider::parse_line(turn.agent, &value, &turn.spec(), &turn.message_id);
        for item in parsed {
            match item {
                Parsed::Send(line) => {
                    write_line(&turn.stdin, &line);
                }
                Parsed::Approval {
                    title,
                    detail,
                    reply,
                } => {
                    let approval = Approval {
                        id: new_id(),
                        title,
                        detail,
                        status: ApprovalStatus::Pending,
                    };
                    turn.pending
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(approval.id.clone(), (turn.message_id.clone(), reply));
                    turn.save_text(&mut unsaved);
                    turn.publish(AgentEvent::ApprovalRequested {
                        message_id: turn.message_id.clone(),
                        approval,
                    });
                }
                Parsed::Finished => {
                    ended = true;
                    let steered = !turn
                        .steers
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .is_empty();
                    // Claude ends its reply to take the steering messages, then answers them.
                    if steered && turn.agent == AgentKind::Claude {
                        turn.steers
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .clear();
                        turn.next_segment(&mut unsaved);
                        segments.push(turn.message_id.clone());
                        (has_text, last_was_text) = (false, false);
                        // Its answer ends with another result; stdin stays open for more.
                        ended = false;
                        continue;
                    }
                    // The finished turn can no longer be steered; until Codex starts the
                    // next one, messages wait for the run to end.
                    *turn
                        .native_turn
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner) = None;
                    if stream_error.is_some() {
                        goal_run = false;
                    }
                    // Claude stays up while a subagent it sent to the background works, and
                    // takes messages meanwhile; stdin is closed once that subagent ends.
                    let background = running_tasks.values().any(Option::is_some);
                    if !background {
                        end_input(&turn, goal_run, &goal_round);
                    }
                }
                Parsed::Steered => {
                    turn.steers
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .pop_front();
                    turn.next_segment(&mut unsaved);
                    segments.push(turn.message_id.clone());
                    (has_text, last_was_text) = (false, false);
                    if ended && !goal_run {
                        turn.close_unless_steered();
                    }
                }
                Parsed::SteerRejected(message) => {
                    log::warn!("the agent did not take the steering message: {message}");
                    let refused = turn
                        .steers
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .pop_front();
                    turn.queue
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .extend(refused);
                    if ended && !goal_run {
                        turn.close_unless_steered();
                    }
                }
                Parsed::Goal(update) => {
                    // Once stopped, the agent's last word on the goal no longer applies.
                    if turn.cancelled.load(Ordering::SeqCst) {
                        continue;
                    }
                    let next = goal::next_goal(goal.as_ref(), update);
                    if next != goal {
                        goal = next;
                        turn.save_text(&mut unsaved);
                        turn.publish(AgentEvent::GoalUpdated { goal: goal.clone() });
                    }
                    // Codex goes on toward an active goal in turns it starts itself.
                    goal_run = turn.agent == AgentKind::Codex
                        && goal
                            .as_ref()
                            .is_some_and(|g| g.status == GoalStatus::Active);
                    if ended {
                        end_input(&turn, goal_run, &goal_round);
                    }
                }
                Parsed::SessionId(id) => {
                    let store = &turn.server.store;
                    if let Err(e) =
                        store.update_thread(&turn.thread_id, |t| t.session_id = Some(id.clone()))
                    {
                        log::warn!("failed to save session id: {e}");
                    }
                    turn.session_id = Some(id);
                }
                Parsed::TaskStarted { tool_id, task_id } => {
                    running_tasks.insert(tool_id.clone(), None);
                    turn.tasks
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(tool_id, task_id);
                }
                Parsed::BackgroundTask {
                    tool_id,
                    description,
                } => {
                    shell_tasks.insert(tool_id.clone(), turn.message_id.clone());
                    turn.publish(AgentEvent::BackgroundTaskStarted {
                        message_id: turn.message_id.clone(),
                        task: BackgroundTask {
                            tool_id,
                            description,
                        },
                    });
                }
                Parsed::Usage(windows) => turn.server.usage.record(turn.agent, windows),
                Parsed::RequestUsage {
                    request_id,
                    usage,
                    top_level,
                } => {
                    // Claude reports a request once per content block, so a repeat updates it.
                    let index = requests.iter().position(|r| r.id == request_id);
                    let message_id = match index {
                        Some(i) => {
                            requests[i].usage = usage;
                            requests[i].message_id.clone()
                        }
                        None => {
                            requests.push(Request {
                                id: request_id,
                                message_id: turn.message_id.clone(),
                                top_level,
                                usage,
                            });
                            turn.message_id.clone()
                        }
                    };
                    turn.save_text(&mut unsaved);
                    turn.publish(AgentEvent::Usage {
                        usage: message_usage(&requests, &message_id),
                        message_id,
                    });
                }
                Parsed::TaskEnded { tool_id, failed } => {
                    if let Some(message_id) = shell_tasks.remove(&tool_id) {
                        turn.publish(AgentEvent::BackgroundTaskEnded {
                            message_id,
                            tool_id: tool_id.clone(),
                        });
                    }
                    if let Some(Some(call_failed)) = running_tasks.remove(&tool_id)
                        && let Some(owner) = tool_owner.get(&tool_id)
                    {
                        turn.save_text(&mut unsaved);
                        turn.publish(AgentEvent::ToolFinished {
                            message_id: owner.clone(),
                            tool_id,
                            is_error: failed || call_failed,
                        });
                    }
                    let background = running_tasks.values().any(Option::is_some);
                    if ended && !background {
                        end_input(&turn, goal_run, &goal_round);
                    }
                }
                Parsed::TurnStarted(id) => {
                    ended = false;
                    goal_round.fetch_add(1, Ordering::SeqCst);
                    *turn
                        .native_turn
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner) = Some(id);
                }
                Parsed::Error(message) => {
                    stream_error.get_or_insert(message);
                }
                Parsed::Event(mut event) => {
                    // Reasoning is shown as it streams, but not saved.
                    if let AgentEvent::Thinking { .. } = &event {
                        turn.server.emit(AgentEventEnvelope {
                            thread_id: turn.thread_id.clone(),
                            event,
                        });
                        continue;
                    }
                    match &mut event {
                        // Separate successive text blocks so they do not run together. Antigravity
                        // and Claude stream a block as chunks, so chunks after the first stay
                        // together.
                        AgentEvent::TextDelta { text, .. } => {
                            let streams =
                                matches!(turn.agent, AgentKind::Antigravity | AgentKind::Claude);
                            let continues = streams && last_was_text;
                            if has_text && !continues {
                                text.insert_str(0, "\n\n");
                            }
                            has_text = true;
                        }
                        // Codex reports a tool when it starts and again when it completes;
                        // only the first ToolStarted for an id is kept.
                        AgentEvent::ToolStarted { tool, message_id } => {
                            if tool_owner.contains_key(&tool.id) {
                                continue;
                            }
                            tool_owner.insert(tool.id.clone(), message_id.clone());
                        }
                        AgentEvent::ToolFinished {
                            tool_id,
                            message_id,
                            is_error,
                        } => {
                            if let Some(owner) = tool_owner.get(tool_id) {
                                message_id.clone_from(owner);
                            }
                            if let Some(held) = running_tasks.get_mut(tool_id.as_str()) {
                                *held = Some(*is_error);
                                continue;
                            }
                        }
                        _ => {}
                    }
                    last_was_text = matches!(event, AgentEvent::TextDelta { .. });
                    if last_was_text {
                        turn.stream_text(event, &mut unsaved);
                    } else {
                        turn.save_text(&mut unsaved);
                        turn.publish(event);
                    }
                }
            }
        }
    }
    // The stream has closed, whether the turn finished, failed, or was cancelled.
    turn.save_text(&mut unsaved);
    // The agent process is gone, and the background commands it started with it.
    for (tool_id, message_id) in shell_tasks.drain() {
        turn.publish(AgentEvent::BackgroundTaskEnded {
            message_id,
            tool_id,
        });
    }

    let exit = child.wait().ok();
    turn.exited.store(true, Ordering::SeqCst);
    let tail = stderr_tail.join().unwrap_or_default();
    let cancelled = turn.cancelled.load(Ordering::SeqCst);
    let (status, error) = if cancelled {
        (MessageStatus::Cancelled, None)
    } else if stream_error.is_some() || !exit.is_some_and(|s| s.success()) {
        let error = stream_error
            .or_else(|| last_lines(&tail))
            .unwrap_or_else(|| exit_error(exit));
        (MessageStatus::Error, Some(error))
    } else {
        (MessageStatus::Complete, None)
    };

    let after = workspace::git_changes(&turn.root).unwrap_or_default();
    for path in changed_paths(&turn.baseline, &after) {
        turn.publish(AgentEvent::FileTouched {
            message_id: turn.message_id.clone(),
            path,
        });
    }

    // A stopped goal is paused; Claude's goal stands as its evaluator last judged it, which
    // also covers a paused goal that Claude took up again with this run's message.
    // The stored goal is read again, since the user may have cleared it meanwhile.
    let goal = turn
        .server
        .store
        .thread(&turn.thread_id)
        .and_then(|t| t.goal);
    let pursued = |g: &&Goal| {
        g.status == GoalStatus::Active
            || (turn.agent == AgentKind::Claude && g.status == GoalStatus::Paused)
    };
    if let Some(g) = goal.as_ref().filter(pursued) {
        let status = if cancelled {
            Some(GoalStatus::Paused)
        } else if turn.agent == AgentKind::Claude && status == MessageStatus::Complete {
            turn.session_id
                .as_deref()
                .map(|id| provider::claude::goal_outcome(id, &g.objective))
        } else if turn.agent == AgentKind::Antigravity && status == MessageStatus::Complete {
            // Antigravity blocks stopping until its goal is verified, so a clean end meets it.
            Some(GoalStatus::Complete)
        } else {
            None
        };
        if let Some(status) = status.filter(|s| *s != g.status) {
            let goal = Some(Goal {
                status,
                ..g.clone()
            });
            turn.publish(AgentEvent::GoalUpdated { goal });
        }
    }

    // Forget the turn before announcing it, so a follow-up message sent on
    // TurnCompleted is not rejected as "already running". Messages that waited for
    // the turn are taken at the same moment, so none is left behind.
    let waiting = {
        let mut turns = turn
            .server
            .runner
            .turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        turns.remove(&turn.thread_id);
        let mut waiting =
            std::mem::take(&mut *turn.queue.lock().unwrap_or_else(PoisonError::into_inner));
        // Steering messages the agent never answered wait too, unless the turn was
        // stopped; messages sent after Stop still start the next turn.
        let unanswered = turn
            .steers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
            .collect::<Vec<_>>();
        if !cancelled {
            waiting.extend(unanswered);
        }
        waiting
    };
    // Earlier messages of the turn were answered in full before a steer moved it on.
    for id in &segments[..segments.len() - 1] {
        turn.publish(AgentEvent::TurnCompleted {
            message_id: id.clone(),
            status: MessageStatus::Complete,
            error: None,
        });
    }
    turn.publish(AgentEvent::TurnCompleted {
        message_id: turn.message_id.clone(),
        status,
        error,
    });

    // Waiting messages are sent now, so they leave the queue before the next turn starts.
    for f in &waiting {
        publish(
            &turn.server,
            &turn.thread_id,
            AgentEvent::MessageQueued {
                message_id: f.message_id.clone(),
                queued: false,
            },
        );
    }
    if !waiting.is_empty() {
        let text = waiting
            .iter()
            .map(|f| f.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let (attachments, images) = waiting.into_iter().map(|f| (f.attachments, f.images)).fold(
            (vec![], vec![]),
            |(mut a, mut i), (fa, fi)| {
                a.extend(fa);
                i.extend(fi);
                (a, i)
            },
        );
        let server = turn.server.clone();
        if let Err(e) =
            server
                .runner
                .start_turn(&server, turn.thread_id.clone(), text, attachments, images)
        {
            log::warn!("failed to start the turn for queued messages: {e}");
        }
    }
}

/// Paths whose git status or line counts differ between two snapshots.
fn changed_paths(before: &[FileChange], after: &[FileChange]) -> Vec<String> {
    let key = |c: &FileChange| (c.status.clone(), c.additions, c.deletions);
    after
        .iter()
        .filter(|a| {
            before
                .iter()
                .find(|b| b.path == a.path)
                .is_none_or(|b| key(b) != key(a))
        })
        .map(|a| a.path.clone())
        .collect()
}

/// Reads all of stderr but keeps only roughly the last `STDERR_TAIL_BYTES`.
fn read_stderr_tail(stderr: ChildStderr) -> String {
    let mut reader = BufReader::new(stderr);
    let mut tail = String::new();
    let mut line = Vec::new();
    while reader.read_until(b'\n', &mut line).unwrap_or(0) > 0 {
        tail.push_str(&String::from_utf8_lossy(&line));
        line.clear();
        if tail.len() > STDERR_TAIL_BYTES {
            let mut cut = tail.len() - STDERR_TAIL_BYTES;
            while !tail.is_char_boundary(cut) {
                cut += 1;
            }
            tail.drain(..cut);
        }
    }
    tail
}

/// The last few non-empty stderr lines, used as the error text when the CLI fails without a message.
fn last_lines(tail: &str) -> Option<String> {
    let mut lines: Vec<&str> = tail
        .lines()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .take(3)
        .collect();
    lines.reverse();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// Applies one event to the stored message. Used for both persistence and replay.
/// Applies `event` to the stored thread. Removing a message moves the ones after it, so
/// that goes through `update_thread`; every other event changes at most one message in place.
fn save_event(server: &Server, thread_id: &str, event: &AgentEvent) -> Res<()> {
    if let AgentEvent::MessageRemoved { .. } = event {
        return server.store.update_thread(thread_id, |t| apply(t, event));
    }
    let message_id = match event {
        AgentEvent::MessageStarted { message } => Some(message.id.as_str()),
        _ => event_message(event),
    };
    server
        .store
        .update_message(thread_id, message_id, |t| apply(t, event))
}

/// The existing message `event` changes, if it changes one.
fn event_message(event: &AgentEvent) -> Option<&str> {
    match event {
        AgentEvent::MessageStarted { .. }
        | AgentEvent::GoalUpdated { .. }
        | AgentEvent::MessageRemoved { .. } => None,
        AgentEvent::TextDelta { message_id, .. }
        | AgentEvent::Thinking { message_id, .. }
        | AgentEvent::ToolStarted { message_id, .. }
        | AgentEvent::ToolFinished { message_id, .. }
        | AgentEvent::PlanUpdated { message_id, .. }
        | AgentEvent::FileTouched { message_id, .. }
        | AgentEvent::ApprovalRequested { message_id, .. }
        | AgentEvent::ApprovalResolved { message_id, .. }
        | AgentEvent::Usage { message_id, .. }
        | AgentEvent::MessageQueued { message_id, .. }
        | AgentEvent::BackgroundTaskStarted { message_id, .. }
        | AgentEvent::BackgroundTaskEnded { message_id, .. }
        | AgentEvent::TurnCompleted { message_id, .. } => Some(message_id),
    }
}

fn apply(thread: &mut Thread, event: &AgentEvent) {
    match event {
        AgentEvent::MessageStarted { message } => {
            if !thread.messages.iter().any(|m| m.id == message.id) {
                thread.messages.push(message.clone());
            }
            return;
        }
        AgentEvent::GoalUpdated { goal } => {
            thread.goal = goal.clone();
            return;
        }
        AgentEvent::MessageRemoved { message_id } => {
            thread.messages.retain(|m| m.id != *message_id);
            return;
        }
        _ => {}
    }
    let Some(message_id) = event_message(event) else {
        return;
    };
    let Some(msg) = thread.messages.iter_mut().find(|m| m.id == *message_id) else {
        return;
    };
    match event {
        AgentEvent::TextDelta { text, .. } => msg.text.push_str(text),
        AgentEvent::Thinking { text, .. } => msg.thinking.push_str(text),
        AgentEvent::ToolStarted { tool, .. } => msg.tools.push(tool.clone()),
        AgentEvent::ToolFinished {
            tool_id, is_error, ..
        } => {
            if let Some(tool) = msg.tools.iter_mut().find(|t| t.id == *tool_id) {
                tool.ended_at = Some(now_ms());
                tool.status = if *is_error {
                    StepStatus::Error
                } else {
                    StepStatus::Done
                };
            }
        }
        AgentEvent::PlanUpdated { steps, .. } => msg.plan = steps.clone(),
        AgentEvent::Usage { usage, .. } => msg.usage = Some(*usage),
        AgentEvent::MessageQueued { queued, .. } => msg.queued = *queued,
        AgentEvent::BackgroundTaskStarted { task, .. } => msg.background.push(task.clone()),
        AgentEvent::BackgroundTaskEnded { tool_id, .. } => {
            msg.background.retain(|t| t.tool_id != *tool_id);
        }
        AgentEvent::FileTouched { path, .. } => {
            if !msg.files.contains(path) {
                msg.files.push(path.clone());
            }
        }
        AgentEvent::ApprovalRequested { approval, .. } => msg.approvals.push(approval.clone()),
        AgentEvent::ApprovalResolved {
            approval_id,
            allowed,
            ..
        } => {
            if let Some(approval) = msg.approvals.iter_mut().find(|a| a.id == *approval_id) {
                approval.status = if *allowed {
                    ApprovalStatus::Allowed
                } else {
                    ApprovalStatus::Denied
                };
            }
        }
        AgentEvent::TurnCompleted { status, error, .. } => {
            msg.status = *status;
            msg.error = error.clone();
            msg.finished_at = Some(now_ms());
            // Unanswered requests lapse with the turn.
            for approval in msg
                .approvals
                .iter_mut()
                .filter(|a| a.status == ApprovalStatus::Pending)
            {
                approval.status = ApprovalStatus::Denied;
            }
            // Nothing can still be running once the turn has ended.
            let tool_end = if *status == MessageStatus::Complete {
                StepStatus::Done
            } else {
                StepStatus::Error
            };
            for tool in msg
                .tools
                .iter_mut()
                .filter(|t| t.status == StepStatus::Running)
            {
                tool.status = tool_end;
                tool.ended_at = Some(now_ms());
            }
            for step in msg
                .plan
                .iter_mut()
                .filter(|s| s.status == StepStatus::Running)
            {
                step.status = StepStatus::Pending;
            }
        }
        AgentEvent::MessageStarted { .. }
        | AgentEvent::GoalUpdated { .. }
        | AgentEvent::MessageRemoved { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(path: &str, status: &str, additions: u32) -> FileChange {
        FileChange {
            path: path.into(),
            status: status.into(),
            additions,
            deletions: 0,
            staged: false,
        }
    }

    #[test]
    fn changed_paths_finds_new_and_modified_files() {
        let before = [change("a.rs", "modified", 1), change("b.rs", "modified", 2)];
        let after = [
            change("a.rs", "modified", 1),
            change("b.rs", "modified", 5),
            change("c.rs", "untracked", 3),
        ];
        assert_eq!(changed_paths(&before, &after), ["b.rs", "c.rs"]);
    }

    /// Runs a real turn through the installed CLI, allowing every approval, and
    /// checks the parsed stream. Needs the agent installed and signed in:
    /// `cargo test -- --ignored`.
    fn live_turn(agent: AgentKind, mode: AccessMode) {
        let root = std::env::temp_dir().join(new_id());
        std::fs::create_dir_all(&root).unwrap();
        let root_str = root.to_str().unwrap();
        let ok = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(ok.success());

        let spec = TurnSpec {
            root: root_str,
            session_id: None,
            model: (agent == AgentKind::Claude).then_some("haiku"),
            text: "Create a file named hello.txt whose only content is the two letters hi (no period). Do nothing else.",
            mode,
            effort: None,
            attachments: &[],
            images: &[],
        };
        let baseline = workspace::git_changes(root_str).unwrap();
        let input = provider::turn_input(agent, &spec);
        let mut child = provider::agent_command(agent)
            .unwrap()
            .args(provider::turn_args(agent, &spec))
            .current_dir(&root)
            .stdin(if input.is_empty() {
                Stdio::null()
            } else {
                Stdio::piped()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin: Stdin = Arc::new(Mutex::new(child.stdin.take()));
        for line in &input {
            write_line(&stdin, line);
        }

        let mut parsed = Vec::new();
        for line in BufReader::new(child.stdout.take().unwrap()).lines() {
            let Ok(value) = serde_json::from_str::<Value>(&line.unwrap()) else {
                continue;
            };
            for item in provider::parse_line(agent, &value, &spec, "m") {
                match &item {
                    Parsed::Send(line) => {
                        write_line(&stdin, line);
                    }
                    Parsed::Approval { reply, .. } => {
                        write_line(&stdin, &provider::approval_reply(agent, reply, true));
                    }
                    Parsed::Finished => drop(stdin.lock().unwrap().take()),
                    _ => {}
                }
                parsed.push(item);
            }
        }
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(parsed.iter().any(|p| matches!(p, Parsed::SessionId(_))));
        assert!(
            !parsed.iter().any(|p| matches!(p, Parsed::Error(_))),
            "{parsed:?}"
        );
        if mode == AccessMode::Supervised && agent != AgentKind::Antigravity {
            assert!(parsed.iter().any(|p| matches!(p, Parsed::Approval { .. })));
        }
        let changed = changed_paths(&baseline, &workspace::git_changes(root_str).unwrap());
        assert_eq!(changed, ["hello.txt"]);
        let content = std::fs::read_to_string(root.join("hello.txt")).unwrap();
        assert_eq!(content.trim(), "hi");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    #[ignore = "runs the real Claude Code CLI"]
    fn live_claude_supervised_turn() {
        live_turn(AgentKind::Claude, AccessMode::Supervised);
    }

    #[test]
    #[ignore = "runs the real Codex CLI"]
    fn live_codex_supervised_turn() {
        live_turn(AgentKind::Codex, AccessMode::Supervised);
    }

    #[test]
    #[ignore = "runs the real Claude Code CLI"]
    fn live_claude_turn() {
        live_turn(AgentKind::Claude, AccessMode::FullAccess);
    }

    #[test]
    #[ignore = "runs the real Codex CLI"]
    fn live_codex_turn() {
        live_turn(AgentKind::Codex, AccessMode::FullAccess);
    }

    #[test]
    #[ignore = "runs the real Antigravity CLI"]
    fn live_antigravity_turn() {
        live_turn(AgentKind::Antigravity, AccessMode::AutoAcceptEdits);
    }
}
