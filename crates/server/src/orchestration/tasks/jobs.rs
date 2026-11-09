//! The commands Patchwork's own agent runs. Patchwork runs them rather than the agent's process,
//! each in a range of its own, so a command still running at its time limit is not stopped: it
//! moves to the background as a job, listed under its message, which the model reads with
//! `job_output` and stops with `job_kill`, and the user stops from the list. What a command
//! leaves running after it exits is held too, and every job is stopped when the app quits.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::Server;
use crate::harness::tools::{BASH_TIMEOUT, MAX_BASH_TIMEOUT, bash_command, outcome};
use crate::orchestration::AgentEvent;
use crate::orchestration::output::stream::publish;
use crate::system::contained::{self, Range, STOP_GRACE};
use crate::system::now_ms;
use crate::threads::{BackgroundTask, TaskStatus};

/// The tools of Patchwork's own agent that run here.
pub(in crate::orchestration) const TOOLS: [&str; 4] =
    ["bash", "job_output", "job_kill", "job_list"];

/// Output a job keeps, its latest.
const KEEP: usize = 1_000_000;

/// How long a command that has exited gets for what it left running to let go of its output.
const PIPE_GRACE: Duration = Duration::from_secs(1);

/// How long `job_output` waits for a job to end, by default and at most.
const WAIT_MS: u64 = 30_000;
const MAX_WAIT_MS: u64 = 600_000;

/// Every command Patchwork's own agent has started that is still held.
#[derive(Default)]
pub struct Jobs {
    jobs: Mutex<HashMap<String, Arc<Job>>>,
    next: AtomicU64,
}

/// The call a tool runs for.
pub(in crate::orchestration) struct Origin {
    pub(in crate::orchestration) thread_id: String,
    /// The message the call is shown in, where a job it starts is listed.
    pub(in crate::orchestration) message_id: String,
    pub(in crate::orchestration) tool_id: String,
    pub(in crate::orchestration) root: String,
}

struct Job {
    id: String,
    thread_id: String,
    tool_id: String,
    command: String,
    started_at: u64,
    range: Range,
    state: Mutex<State>,
    /// Signalled on new output, a pipe closing, and the command's exit.
    changed: Condvar,
}

#[derive(Default)]
struct State {
    output: Vec<u8>,
    /// Bytes cut from the start of `output` to keep it to [`KEEP`].
    cut: usize,
    /// How far into the output the model has read, counted from its start.
    read: usize,
    /// Output pipes still open.
    open: u8,
    /// The command's exit code once it has exited: None inside when a signal ended it.
    #[expect(
        clippy::option_option,
        reason = "None until it exits, then its code, which a signal leaves out"
    )]
    exit: Option<Option<i32>>,
    /// The message the job is listed under, once it is in the background.
    listed: Option<String>,
    /// Stopped by the model, the user, or quitting.
    stopped: bool,
    /// Its end has been published.
    ended: bool,
}

impl State {
    fn push(&mut self, bytes: &[u8]) {
        self.output.extend_from_slice(bytes);
        // Cut only once it is twice the limit, so the copy happens rarely.
        if self.output.len() > KEEP * 2 {
            let cut = self.output.len() - KEEP;
            self.output.drain(..cut);
            self.cut += cut;
        }
    }

    /// The output the model has not read yet, which it has read once this returns. A character
    /// still arriving is left for the next read.
    fn take_new(&mut self) -> String {
        let start = self.read.saturating_sub(self.cut);
        let bytes = &self.output[start..];
        let end = match std::str::from_utf8(bytes) {
            Err(e) if e.error_len().is_none() && self.exit.is_none() => e.valid_up_to(),
            _ => bytes.len(),
        };
        let mut text = String::new();
        if self.read < self.cut {
            text = format!("[{} earlier bytes were dropped]\n", self.cut - self.read);
        }
        text.push_str(&String::from_utf8_lossy(&bytes[..end]));
        self.read = self.cut + start + end;
        text
    }
}

impl Job {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Copies a pipe into the job's output on its own thread.
    fn read(self: &Arc<Self>, mut pipe: impl Read + Send + 'static) {
        let job = Arc::clone(self);
        std::thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            while let Ok(n) = pipe.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                job.lock().push(&chunk[..n]);
                job.changed.notify_all();
            }
            job.lock().open -= 1;
            job.changed.notify_all();
        });
    }

    /// Waits for the command on its own thread, and publishes the end of a listed job.
    fn watch(self: &Arc<Self>, server: Arc<Server>, mut child: Child) {
        let job = Arc::clone(self);
        std::thread::spawn(move || {
            let code = child.wait().ok().and_then(|s| s.code());
            let mut state = job.lock();
            state.exit = Some(code);
            job.changed.notify_all();
            job.end(&server, &mut state);
        });
    }

    /// Lists the job under `message_id`, as a background task that is running, or has just
    /// ended. Published under the lock, so its end is never published before it.
    fn list(&self, server: &Server, message_id: &str) {
        let mut state = self.lock();
        state.listed = Some(message_id.into());
        let task = self.task(TaskStatus::Running);
        publish(
            server,
            &self.thread_id,
            AgentEvent::BackgroundTaskStarted {
                message_id: message_id.into(),
                task,
            },
        );
        if state.exit.is_some() {
            self.end(server, &mut state);
        }
    }

    /// Publishes the end of a listed job, once.
    fn end(&self, server: &Server, state: &mut State) {
        let Some(message_id) = state.listed.clone().filter(|_| !state.ended) else {
            return;
        };
        state.ended = true;
        let status = match state.exit {
            _ if state.stopped => TaskStatus::Stopped,
            Some(Some(0)) => TaskStatus::Done,
            _ => TaskStatus::Failed,
        };
        let task = self.task(status);
        publish(
            server,
            &self.thread_id,
            AgentEvent::BackgroundTaskEnded { message_id, task },
        );
    }

    fn task(&self, status: TaskStatus) -> BackgroundTask {
        BackgroundTask {
            tool_id: self.tool_id.clone(),
            task_id: Some(self.id.clone()),
            run_id: None,
            kind: "command".into(),
            description: self.command.clone(),
            status,
            started_at: self.started_at,
            summary: None,
            output: None,
        }
    }

    /// Stops the command and all it started. False when it had already exited.
    fn stop(&self) -> bool {
        let mut state = self.lock();
        if state.exit.is_some() {
            return false;
        }
        state.stopped = true;
        drop(state);
        self.range.stop();
        true
    }

    /// The output the model has not read, and where the job stands.
    fn report(&self) -> String {
        let mut state = self.lock();
        let text = state.take_new();
        let id = &self.id;
        let status = match state.exit {
            None if state.stopped => format!("[job {id} is stopping]"),
            None => format!("[job {id} is still running]"),
            Some(_) if state.stopped => format!("[job {id} was stopped]"),
            Some(Some(code)) => format!("[job {id} exited with code {code}]"),
            Some(None) => format!("[job {id} was ended by a signal]"),
        };
        let text = text.trim_end();
        if text.is_empty() {
            status
        } else {
            format!("{text}\n\n{status}")
        }
    }
}

impl Jobs {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Arc<Job>>> {
        self.jobs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The thread's job `id`.
    fn get(&self, thread_id: &str, id: &str) -> Option<Arc<Job>> {
        self.lock()
            .get(id)
            .filter(|job| job.thread_id == thread_id)
            .cloned()
    }

    /// Stops the job the call `tool_id` started, as the user asks from its listing. None when
    /// no job of the thread came from that call; false when it had already ended.
    pub(in crate::orchestration) fn stop_listed(
        &self,
        thread_id: &str,
        tool_id: &str,
    ) -> Option<bool> {
        let job = self
            .lock()
            .values()
            .find(|job| job.thread_id == thread_id && job.tool_id == tool_id)
            .cloned()?;
        Some(job.stop())
    }

    /// Stops every job and what each left running, waiting up to [`STOP_GRACE`] for them, so
    /// none outlives the app. A listed job still running is published as stopped.
    pub(in crate::orchestration) fn stop_all(&self, server: &Server) {
        let jobs: Vec<Arc<Job>> = self.lock().values().cloned().collect();
        for job in &jobs {
            if !job.stop() && job.range.running() {
                job.range.stop();
            }
        }
        // Past the grace, what is left has been sent SIGKILL.
        let deadline = Instant::now() + STOP_GRACE + Duration::from_millis(200);
        while Instant::now() < deadline && jobs.iter().any(|job| job.lock().exit.is_none()) {
            std::thread::sleep(Duration::from_millis(50));
        }
        for job in &jobs {
            job.end(server, &mut job.lock());
        }
    }

    /// The thread's jobs in the background, one a line.
    fn list(&self, thread_id: &str) -> String {
        let mut jobs: Vec<Arc<Job>> = self
            .lock()
            .values()
            .filter(|job| job.thread_id == thread_id && job.lock().listed.is_some())
            .cloned()
            .collect();
        if jobs.is_empty() {
            return "No background jobs".into();
        }
        jobs.sort_by_key(|job| job.id.parse::<u64>().unwrap_or(0));
        jobs.iter()
            .map(|job| {
                let state = job.lock();
                let status = match state.exit {
                    None if state.stopped => "stopping".into(),
                    None => "running".into(),
                    Some(_) if state.stopped => "stopped".into(),
                    Some(Some(code)) => format!("exited with code {code}"),
                    Some(None) => "ended by a signal".into(),
                };
                format!("{}\t{status}\t{}", job.id, job.command)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Runs one of [`TOOLS`] for the call `from`. `stopped` turns true once the turn is stopped.
pub(in crate::orchestration) fn call(
    server: &Arc<Server>,
    from: &Origin,
    tool: &str,
    args: &Value,
    stopped: &dyn Fn() -> bool,
) -> Result<String, Error> {
    let jobs = &server.runner.jobs;
    let named = || {
        let id = args["job_id"]
            .as_str()
            .map(String::from)
            .or_else(|| args["job_id"].as_u64().map(|n| n.to_string()))
            .ok_or_else(|| "job_id is required".to_string())?;
        jobs.get(&from.thread_id, &id)
            .ok_or_else(|| format!("There is no job {id}"))
    };
    match tool {
        "bash" => bash(server, from, args, stopped),
        "job_output" => {
            let job = named()?;
            if args["wait"] == true {
                let wait = args["timeout_ms"]
                    .as_u64()
                    .unwrap_or(WAIT_MS)
                    .min(MAX_WAIT_MS);
                let deadline = Instant::now() + Duration::from_millis(wait);
                let mut state = job.lock();
                while state.exit.is_none() && Instant::now() < deadline && !stopped() {
                    state = job
                        .changed
                        // Wakes to check `stopped` even when the job sends no change.
                        .wait_timeout(state, Duration::from_millis(50))
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                }
            }
            Ok(job.report())
        }
        "job_kill" => {
            let job = named()?;
            Ok(if job.stop() {
                format!("Requested cancellation of job {}", job.id)
            } else {
                format!("Job {} had already finished", job.id)
            })
        }
        "job_list" => Ok(jobs.list(&from.thread_id)),
        _ => Err(format!("Unknown tool {tool}").into()),
    }
}

/// Runs the bash tool's command. One still running at its time limit, or asked to run in the
/// background, moves to the background as a job; one the turn's stop reaches first is stopped.
fn bash(
    server: &Arc<Server>,
    from: &Origin,
    args: &Value,
    stopped: &dyn Fn() -> bool,
) -> Result<String, Error> {
    let command = args["command"]
        .as_str()
        .ok_or_else(|| "command is required".to_string())?;
    let timeout = args["timeout"]
        .as_u64()
        .unwrap_or(BASH_TIMEOUT)
        .clamp(1, MAX_BASH_TIMEOUT);
    let cmd = bash_command(command, Path::new(&from.root))?;
    let (mut child, range) = contained::spawn(cmd, Stdio::null())
        .map_err(|e| format!("Failed to run the command: {e}"))?;
    let (Some(out), Some(err)) = (child.stdout.take(), child.stderr.take()) else {
        range.stop();
        return Err("The command's output is unavailable".into());
    };
    let jobs = &server.runner.jobs;
    let job = Arc::new(Job {
        id: (jobs.next.fetch_add(1, Ordering::Relaxed) + 1).to_string(),
        thread_id: from.thread_id.clone(),
        tool_id: from.tool_id.clone(),
        command: command.into(),
        started_at: now_ms(),
        range,
        state: Mutex::new(State {
            open: 2,
            ..State::default()
        }),
        changed: Condvar::new(),
    });
    jobs.lock().insert(job.id.clone(), Arc::clone(&job));
    job.read(out);
    job.read(err);
    job.watch(Arc::clone(server), child);
    if args["run_in_background"] == true {
        job.list(server, &from.message_id);
        return Ok(format!(
            "Started background job {}. Read its output with job_output.",
            job.id
        ));
    }
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let mut exited_at = None;
    let mut state = job.lock();
    loop {
        if let Some(code) = state.exit {
            let at = *exited_at.get_or_insert_with(Instant::now);
            // A process the command left running can hold the pipes open; its output so far is
            // kept, and it stays held until the app quits.
            if state.open == 0 || at.elapsed() >= PIPE_GRACE {
                let text = state.take_new();
                drop(state);
                if !job.range.running() {
                    jobs.lock().remove(&job.id);
                }
                return outcome(&text, code);
            }
        } else if stopped() {
            drop(state);
            job.stop();
            return Err("The turn was stopped, and the command with it".into());
        } else if Instant::now() >= deadline {
            let text = state.take_new();
            drop(state);
            job.list(server, &from.message_id);
            let text = text.trim_end();
            return Ok(format!(
                "{text}\n\n[still running after {timeout} s; moved to background job {}]",
                job.id
            )
            .trim_start()
            .into());
        }
        state = job
            .changed
            // Wakes to check the deadline even when the job sends no change.
            .wait_timeout(state, Duration::from_millis(50))
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
}
