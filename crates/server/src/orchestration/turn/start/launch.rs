//! Starting a turn: the agent's command, arguments and input built from the thread, its process
//! spawned, and the watch that retires it once it has sat idle.

// Rust guideline compliant 2026-10-09

use crate::Error;
use super::claim::Starting;
use super::running::{NativeTurn, Pending, RunningTurn, TurnRequest};
use super::turn::{Turn, assistant_message};
use crate::orchestration::turn::end::{log_cli_version, turn_hooks};

#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::sync::PoisonError;

use crate::orchestration::changes::attribution::Attribution;
use crate::hooks::HookEvent;
use crate::provider::TurnSpec;
use crate::provider;
use crate::system::scope;
use crate::Server;
use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::provider::AgentKind;
use crate::threads::Goal;
use crate::threads::GoalStatus;
use crate::threads::MessageStatus;
use crate::system::now_ms;
use crate::threads::Role;
use crate::logging::{AGENT_LOG, Data, Source};
use crate::settings::LogLevel;
use crate::workspace;
use crate::orchestration::changes::checkpoint;
use crate::orchestration::output::ingest::ingest;
use crate::orchestration::turn::process::AgentStdin;
use crate::orchestration::turn::process::Interrupts;
use crate::orchestration::turn::process::read_stderr_tail;
use crate::orchestration::turn::process::Stdin;
use crate::orchestration::turn::process::IDLE_LIMIT;
use crate::orchestration::turn::process::WATCH_EVERY;
use crate::orchestration::turn::process::QUIET_AFTER;
use crate::orchestration::turn::process::write_line;
use crate::orchestration::messages::queue::Queue;
use crate::orchestration::messages::queue::Steers;
use crate::orchestration::Runner;
use crate::orchestration::tasks::subagents::Controls;
use crate::orchestration::tasks::subagents::Tasks;

/// Refuses a turn in a thread whose agent is replying, rather than starting a second agent beside
/// it that nothing could stop.
const ALREADY_REPLYING: &str = "This chat already has a reply running";

impl Runner {
    pub(crate) fn start_turn(
        &self,
        server: &Arc<Server>,
        request: TurnRequest,
    ) -> Result<(), Error> {
        let starting = Starting::claim(self, &request.thread_id)?;
        self.start_claimed(server, request, starting)
    }

    /// Starts the turn in a thread the caller has claimed, as one that changes the thread first
    /// does, so that no other turn starts in it between the change and this one.
    pub(in crate::orchestration) fn start_claimed(
        &self,
        server: &Arc<Server>,
        request: TurnRequest,
        starting: Starting<'_>,
    ) -> Result<(), Error> {
        let TurnRequest {
            thread_id,
            text,
            attachments,
            images,
            resumes,
        } = request;
        let _starting = starting;
        // Whoever asked may have checked before another turn started here.
        if self.is_running(&thread_id) {
            return Err(ALREADY_REPLYING.into());
        }
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
            point,
            project_id,
            checkpoint,
        ) = {
            let thread = server.thread(&thread_id)?;
            // The message being sent is the newest user message; the transcript is what came before.
            // A resumed turn whose agent kept no session is told the conversation instead.
            // Claude picks its own session up from where an undo or a fork left it instead.
            let point = (thread.agent == AgentKind::Claude && thread.session_id.is_some())
                .then(|| server.resume_point(&thread_id))
                .flatten();
            let fresh = point.is_none()
                && (thread.context_pending || (resumes && thread.session_id.is_none()));
            let context = fresh.then(|| {
                // A resumed turn sends no new message, so everything so far came before it.
                let end = if resumes {
                    thread.messages.len()
                } else {
                    thread
                        .messages
                        .iter()
                        .rposition(|m| m.role == Role::User)
                        .unwrap_or(thread.messages.len())
                };
                crate::import::transcript(&thread.messages[..end])
            });
            let settings = server.store.settings();
            let root = if let Some(id) = &thread.project_id {
                let project = settings
                    .projects
                    .iter()
                    .find(|p| &p.id == id)
                    .ok_or("Project not found")?;
                // Spawning in a missing folder fails with a bare "No such file or directory". A
                // folder on an SSH host is found missing there, when the helper changes into it.
                if !crate::system::remote::is_remote(&project.path)
                    && !std::path::Path::new(&project.path).is_dir()
                {
                    return Err(format!(
                        "The {} project's folder, {}, no longer exists",
                        project.name, project.path
                    )
                    .into());
                }
                project.path.clone()
            } else {
                let dir = server.chat_dir(&thread_id);
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                dir.to_string_lossy().into_owned()
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
                point,
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
        let hooks = crate::hooks::agent_hooks(&settings);
        // The custom agents, for the CLIs that can start them as subagents.
        let agents = match agent {
            AgentKind::Claude | AgentKind::Codex | AgentKind::OpenCode => {
                server.thread_agents(&thread_id)
            }
            AgentKind::Antigravity | AgentKind::Api => Vec::new(),
        };
        let vision_model = settings.vision.clone();
        let spec = TurnSpec {
            root: crate::system::remote::here(&root),
            host: crate::system::remote::split(&root).map(|at| at.host),
            thread_id: &thread_id,
            session_id: session_id.as_deref(),
            model: model.as_deref(),
            vision_model: vision_model.as_ref(),
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
        if !resumes {
            turn_hooks(
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
        }
        let mut args = provider::turn_args(agent, &spec);
        if agent == AgentKind::Claude {
            args.extend(crate::provider::claude::launch::launch_args(
                &settings,
                &hooks,
                crate::orchestration::agent_instructions(&settings, AgentKind::Claude),
            ));
            args.extend(crate::provider::claude::launch::permission_args());
            args.extend(crate::provider::claude::launch::agents_args(&agents, &root));
            // After an undo or a fork the session goes on from the reply they left last. The
            // point is forgotten once the session reports in, as it then goes on from there.
            if let Some(point) = &point {
                args.extend(crate::provider::claude::launch::resume_point_args(point));
            }
        }
        // The chat's own pick for the subagents its CLI starts, where the CLI takes one.
        let sub = crate::threads::sub_pick(&server.store, &settings, &thread_id, agent);
        if let (AgentKind::Codex, Some(sub)) = (agent, &sub) {
            args.extend(crate::provider::codex::sub_agent_args(sub));
        }
        if let Some(why) = provider::hooks::unguarded(agent, &args) {
            return Err(why.into());
        }
        let mut input = provider::turn_input(agent, &spec);
        if let Some(host) = spec.host {
            // The files the input names, such as attachments and where sessions are kept, as the
            // agent on the host finds them.
            input = input
                .iter()
                .map(|line| crate::system::remote::mirrored(host, line))
                .collect::<Result<_, _>>()?;
        }
        // A project on an SSH host runs its agent there, under Patchwork's helper, which stops it
        // when the connection ends, so no scope is made for it here.
        let remote = crate::system::remote::is_remote(&root);
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
            if let Some(sub) = &sub {
                cmd.envs(crate::provider::claude::launch::sub_agent_env(sub));
            }
        }
        cmd.args(&args)
            .envs(provider::turn_env(agent, &spec))
            .envs(crate::permissions::hook::env(
                agent,
                &thread_id,
                crate::system::remote::here(&root),
            ))
            .current_dir(&root);
        let mut cmd = crate::system::remote::supervised(cmd)?;
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
            let source = Source::agent(agent, pid, &thread_id, None);
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
            && let Err(error) = server
                .store
                .update_thread(&thread_id, |t| t.context_pending = false)
        {
            return abandon(&mut child, error);
        }
        let source = Source::agent(agent, pid, &thread_id, None);
        let stderr_tail = std::thread::spawn(move || read_stderr_tail(stderr, &source));
        let stdin: Stdin = Arc::new(Mutex::new(AgentStdin::take(&mut child, agent, &thread_id)));
        for line in &input {
            write_line(&stdin, line);
        }

        let message = assistant_message();
        let cancelled = Arc::new(AtomicBool::new(false));
        let interrupts = Arc::<Interrupts>::default();
        let exited = Arc::new(AtomicBool::new(false));
        let pending = Pending::default();
        let native_turn = NativeTurn::default();
        let steers = Steers::default();
        let queue = Queue::default();
        let tasks = Tasks::default();
        let controls = Controls::default();
        let idle = Arc::new(AtomicBool::new(false));
        let turn = Turn {
            server: Arc::clone(server),
            thread_id: thread_id.clone(),
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
            tasks: Arc::clone(&tasks),
            controls: Arc::clone(&controls),
            idle: Arc::clone(&idle),
            last_output: Arc::new(AtomicU64::new(now_ms())),
            root,
            session_id,
            model,
            vision_model,
            text,
            resumes,
            sent_context: sends_context,
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
                    native: native_turn,
                    steers,
                    queue,
                    tasks,
                    controls,
                    idle,
                    stale: AtomicBool::new(false),
                    root: turn.root.clone(),
                    attribution,
                },
            );
        crate::orchestration::turn::recovery::note_agent(
            &server.store,
            &turn.thread_id,
            pid,
            turn.scope.as_deref(),
        );
        // Stopped while it was starting, as by Stop or archiving the chat: it stops now it can.
        if self
            .stop_starting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&turn.thread_id)
        {
            self.cancel(&turn.thread_id, None);
        }
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
            server: Arc::clone(server),
            thread_id: turn.thread_id.clone(),
            pid,
            scope: turn.scope.clone(),
            exited: Arc::clone(&turn.exited),
            idle: Arc::clone(&turn.idle),
            pending: Arc::clone(&turn.pending),
            last_output: Arc::clone(&turn.last_output),
        };
        std::thread::spawn(move || watch(&watched));
        std::thread::spawn(move || ingest(turn, child, stdout, stderr_tail));
        Ok(())
    }
}

/// What `watch` checks one agent process by.
struct Watched {
    server: Arc<Server>,
    thread_id: String,
    pid: u32,
    scope: Option<String>,
    exited: Arc<AtomicBool>,
    idle: Arc<AtomicBool>,
    pending: Pending,
    last_output: Arc<AtomicU64>,
}

/// Watches one agent process until it ends. It retires the agent once it has been idle for
/// `IDLE_LIMIT` with no shell commands of its own still running; tells the user when a reply has
/// said nothing for `QUIET_AFTER`; and ends the turn of an agent that exited while something it
/// started still holds its output open. A reply runs as long as it needs.
fn watch(w: &Watched) {
    loop {
        std::thread::sleep(WATCH_EVERY);
        if w.exited.load(Ordering::SeqCst) {
            return;
        }
        if agent_exited(w.pid) {
            // The reader waits for the output to close, which a process the agent left behind
            // can hold open; stopping what is left closes it, and the turn ends with the exit.
            log::warn!(pid = w.pid; "agent exited with its output still open; stopping what it left");
            workspace::signal_process_group(w.pid, workspace::SIGKILL);
            if let Some(unit) = &w.scope {
                scope::stop(unit);
            }
            return;
        }
        if !w.idle.load(Ordering::SeqCst) {
            tell_quiet(w);
            continue;
        }
        // Idle time counts from the last reply, as idle compaction counts it.
        let retire = w.server.store.thread(&w.thread_id).is_some_and(|t| {
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

/// Tells the user the reply under way has said nothing for `QUIET_AFTER`, unless it waits on
/// them to answer an approval.
fn tell_quiet(w: &Watched) {
    let since = w.last_output.load(Ordering::Relaxed);
    let quiet_ms = u64::try_from(QUIET_AFTER.as_millis()).unwrap_or(u64::MAX);
    if now_ms().saturating_sub(since) < quiet_ms
        || !w
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    {
        return;
    }
    let Some(thread) = w.server.store.thread(&w.thread_id) else {
        return;
    };
    let Some(reply) = thread
        .messages
        .iter()
        .rev()
        .find(|m| m.role == Role::Assistant && m.status == MessageStatus::Streaming)
    else {
        return;
    };
    // A reply that started after the last output has been quiet only since it started.
    let since = since.max(reply.created_at);
    if now_ms().saturating_sub(since) < quiet_ms {
        return;
    }
    w.server.emit(AgentEventEnvelope {
        thread_id: w.thread_id.clone(),
        event: AgentEvent::Quiet {
            message_id: reply.id.clone(),
            since,
        },
    });
}

/// Whether the agent `pid` has exited and waits to be reaped.
#[cfg(target_os = "linux")]
fn agent_exited(pid: u32) -> bool {
    // The state follows the command name in parentheses, which may itself hold any character.
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        stat.rsplit_once(')')
            .is_some_and(|(_, rest)| rest.trim_start().starts_with('Z'))
    })
}

/// Whether the agent `pid` has exited and waits to be reaped; only Linux is checked.
#[cfg(not(target_os = "linux"))]
fn agent_exited(_pid: u32) -> bool {
    false
}
