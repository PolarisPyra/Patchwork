//! Running the tools a chat's agent drives other chats with (see `crate::threads::chat_tools`):
//! each acts on a chat in the caller's project as a person would from the chat list.

// Rust guideline compliant 2026-10-09

use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use serde_json::{Value, json};

use super::wait::Unanswered;
use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::orchestration::Runner;
use crate::provider::AgentKind;
use crate::system::now_ms;
use crate::threads::chat_tools::{self, LIST, READ, SEND, START, WAIT};
use crate::threads::{MessageStatus, NewThread, Role, Standing, StepStatus, Thread};
use crate::{Error, Core};

/// The longest `wait_for_chat` waits. Under the 65 minutes a CLI waits on one of Patchwork's own
/// tools, so the caller hears that it ran out rather than a dropped call.
const MAX_WAIT: Duration = Duration::from_secs(60 * 60);

/// The words of a name that say nothing about which chat it is, as in 'what's going on in X'.
const IGNORED: &[&str] = &[
    "a",
    "about",
    "an",
    "and",
    "at",
    "chat",
    "doing",
    "for",
    "going",
    "happening",
    "how",
    "in",
    "is",
    "it",
    "my",
    "of",
    "on",
    "progress",
    "project",
    "s",
    "status",
    "task",
    "the",
    "update",
    "what",
    "whats",
    "with",
];

/// The most chats `read_chat` offers when a name matches several equally.
const CANDIDATES: usize = 8;

/// The most characters of one message `read_chat` shows.
const MESSAGE_LIMIT: usize = 1500;

/// How many of a chat's latest messages `read_chat` shows.
const MESSAGES: usize = 6;

/// How many of a reply's latest steps `read_chat` shows.
const STEPS: usize = 12;

/// A text argument, trimmed, or empty when it is missing.
fn text<'a>(args: &'a Value, key: &str) -> &'a str {
    args[key].as_str().map(str::trim).unwrap_or_default()
}

/// A text argument that must be given.
fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, Error> {
    let value = text(args, key);
    if value.is_empty() {
        return Err(format!("Give the {key}").into());
    }
    Ok(value)
}

/// Whether the chat is replying or about to.
fn busy(runner: &Runner, thread_id: &str) -> bool {
    runner.is_starting(thread_id) || runner.is_running(thread_id)
}

impl Runner {
    /// Runs the chat tool `tool` for the agent of `thread_id`.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is not offered the tools, an argument is missing or names
    /// a chat outside the caller's project, or the chat cannot be started or messaged.
    pub(crate) fn run_chat_tool(
        &self,
        core: &Arc<Core>,
        thread_id: &str,
        tool: &str,
        args: &Value,
        stopped: &dyn Fn() -> bool,
    ) -> Result<String, Error> {
        let caller = core
            .store
            .thread(thread_id)
            .ok_or("This chat is no longer here")?;
        if !chat_tools::offered(core, &caller) {
            return Err("This chat cannot drive other chats".into());
        }
        match tool {
            LIST => Ok(self.list_chats(core, &caller)),
            READ => self.read_chat(core, &caller, args),
            START => start_chat(core, &caller, args),
            SEND => {
                let target = target(core, &caller, args)?;
                if busy(self, &target.id) {
                    return Err("That chat is still replying; wait for it first".into());
                }
                core.send_message(
                    target.id,
                    required(args, "prompt")?.to_string(),
                    Vec::new(),
                    false,
                )?;
                Ok("Sent. Call wait_for_chat for the reply.".into())
            }
            WAIT => self.wait_for_chat(core, &caller, args, stopped),
            _ => Err(format!("Patchwork has no tool {tool}").into()),
        }
    }

    /// The other chats in the caller's project, newest first. A chat run from a chat app sees
    /// every project's chats, each with its project.
    fn list_chats(&self, core: &Core, caller: &Thread) -> String {
        let everywhere = chat_tools::sees_every_project(core, caller);
        let projects = core.store.settings().projects;
        let chats: Vec<Value> = readable(core, caller)
            .into_iter()
            .map(|t| {
                let replying = busy(self, &t.id);
                let mut chat = json!({
                    "chat_id": t.id,
                    "title": t.title,
                    "agent": t.agent,
                    "model": t.model,
                    "replying": replying,
                });
                if everywhere {
                    let project = t
                        .project_id
                        .as_ref()
                        .and_then(|id| projects.iter().find(|p| &p.id == id))
                        .map(|p| p.name.clone());
                    chat["project"] = json!(project);
                }
                if replying {
                    chat["doing"] = json!(t.doing());
                }
                chat
            })
            .collect();
        if chats.is_empty() {
            return if everywhere {
                "There are no other chats.".into()
            } else {
                "There are no other chats in this project.".into()
            };
        }
        Value::Array(chats).to_string()
    }

    /// How far the chat `args` names has got, or the chats its name fits equally well.
    fn read_chat(&self, core: &Core, caller: &Thread, args: &Value) -> Result<String, Error> {
        let projects = core.store.settings().projects;
        let project = |t: &Thread| {
            t.project_id
                .as_ref()
                .and_then(|id| projects.iter().find(|p| &p.id == id))
                .map(|p| p.name.clone())
        };
        let chats = readable(core, caller);
        let id = text(args, "chat_id");
        let chat = if id.is_empty() {
            let wanted: Vec<String> = words(required(args, "name")?)
                .into_iter()
                .filter(|w| !IGNORED.contains(&w.as_str()))
                .collect();
            if wanted.is_empty() {
                return Err("Give the chat's title or project as the name".into());
            }
            let mut fits: Vec<(usize, bool, u64, &Thread)> = chats
                .iter()
                .filter_map(|t| {
                    let have = words(&format!("{} {}", t.title, project(t).unwrap_or_default()));
                    let score = wanted
                        .iter()
                        .filter(|w| have.iter().any(|h| alike(w, h)))
                        .count();
                    (score > 0).then(|| (score, busy(self, &t.id), last_active(t), t))
                })
                .collect();
            fits.sort_by_key(|f| std::cmp::Reverse((f.0, f.1, f.2)));
            let best: Vec<&Thread> = fits
                .iter()
                .take_while(|f| f.0 == fits[0].0)
                .map(|f| f.3)
                .collect();
            match best.as_slice() {
                [] => {
                    return Err(
                        "No chat has a title or project like that; list_chats lists them".into(),
                    );
                }
                [only] => *only,
                several => {
                    let candidates: Vec<Value> = several
                        .iter()
                        .take(CANDIDATES)
                        .map(|t| {
                            json!({
                                "chat_id": t.id,
                                "title": t.title,
                                "project": project(t),
                                "replying": busy(self, &t.id),
                            })
                        })
                        .collect();
                    return Ok(json!({
                        "several_chats_match": candidates,
                        "next": "Call read_chat with the chat_id of the one meant, or ask which.",
                    })
                    .to_string());
                }
            }
        } else {
            chats
                .iter()
                .find(|t| t.id == id)
                .ok_or("No chat with that id that this chat can read")?
        };
        Ok(self.progress(chat, project(chat).as_deref()).to_string())
    }

    /// What `read_chat` tells of `chat`: its task, where it stands, and what it has done.
    fn progress(&self, chat: &Thread, project: Option<&str>) -> Value {
        let replying = busy(self, &chat.id);
        let standing = chat.standing();
        let state = match standing {
            Standing::Waiting => "waiting for an answer",
            _ if replying => "replying",
            Standing::Working => "replying",
            Standing::Failed => "failed",
            Standing::Done => "done",
        };
        let sent: Vec<_> = chat.messages.iter().filter(|m| !m.queued).collect();
        let task = sent
            .iter()
            .find(|m| m.role == Role::User)
            .map(|m| clip(&m.text, MESSAGE_LIMIT));
        let reply = sent.iter().rfind(|m| m.role == Role::Assistant);
        let mut report = json!({
            "chat_id": chat.id,
            "title": chat.title,
            "project": project,
            "agent": chat.agent,
            "model": chat.model,
            "state": state,
            "task": task,
        });
        if replying {
            report["doing"] = json!(chat.doing());
            if let Some(reply) = reply {
                report["replying_for_minutes"] =
                    json!(now_ms().saturating_sub(reply.created_at) / 60_000);
            }
        }
        if let Some(ask) = chat.ask() {
            report["waiting_on"] = json!({"asks": ask.title, "detail": ask.detail});
        }
        if let Some(goal) = &chat.goal {
            report["goal"] = json!({"objective": goal.objective, "status": goal.status});
        }
        if let Some(plan) = sent.iter().rev().map(|m| &m.plan).find(|p| !p.is_empty()) {
            let done = plan.iter().filter(|s| s.status == StepStatus::Done).count();
            let steps: Vec<String> = plan
                .iter()
                .map(|s| format!("{}: {}", step_word(s.status), s.text))
                .collect();
            report["plan"] = json!({"done": done, "of": plan.len(), "steps": steps});
        }
        if let Some(reply) = reply {
            let steps: Vec<String> = reply
                .tools
                .iter()
                .filter(|t| t.parent.is_none())
                .map(|t| format!("{} {}: {}", t.name, t.summary.trim(), step_word(t.status)))
                .collect();
            let steps = &steps[steps.len().saturating_sub(STEPS)..];
            if !steps.is_empty() {
                report["recent_steps"] = json!(steps);
            }
            let running: Vec<&str> = reply
                .background
                .iter()
                .filter(|t| t.status.is_live())
                .map(|t| t.description.as_str())
                .collect();
            if !running.is_empty() {
                report["background_tasks"] = json!(running);
            }
        }
        if let Some(files) = sent
            .iter()
            .rev()
            .filter(|m| m.role == Role::Assistant)
            .map(|m| &m.files)
            .find(|f| !f.is_empty())
        {
            report["files_changed"] = json!(files);
        }
        let messages: Vec<Value> = sent[sent.len().saturating_sub(MESSAGES)..]
            .iter()
            .map(|m| {
                let from = if m.role == Role::User {
                    "person"
                } else {
                    "agent"
                };
                // A reply still being written shows its newest part.
                let text = if m.status == MessageStatus::Streaming {
                    clip_start(&m.text, MESSAGE_LIMIT)
                } else {
                    clip(&m.text, MESSAGE_LIMIT)
                };
                json!({"from": from, "text": text, "error": m.error})
            })
            .collect();
        report["messages"] = json!(messages);
        let queued = chat.messages.iter().filter(|m| m.queued).count();
        if queued > 0 {
            report["queued_messages"] = json!(queued);
        }
        report
    }

    /// Waits for the reply to the last message of the chat `args` names, while the caller's
    /// turn runs. The chat itself is left running when the caller stops waiting.
    fn wait_for_chat(
        &self,
        core: &Arc<Core>,
        caller: &Thread,
        args: &Value,
        stopped: &dyn Fn() -> bool,
    ) -> Result<String, Error> {
        let target = target(core, caller, args)?;
        let flags = {
            let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
            let turn = turns
                .get(&caller.id)
                .ok_or("This turn is no longer running")?;
            [Arc::clone(&turn.cancelled), Arc::clone(&turn.exited)]
        };
        let gone = || stopped() || flags.iter().any(|f| f.load(Ordering::SeqCst));
        let asked = target
            .messages
            .iter()
            .rfind(|m| m.role == Role::User && !m.queued)
            .map(|m| m.id.clone())
            .ok_or("That chat has no messages yet")?;
        let reply = self
            .wait_for_reply(core, &target.id, Some(&asked), &gone, MAX_WAIT)
            .map_err(|why| {
                Error::from(match why {
                    Unanswered::Stopped => "Stopped waiting with this turn",
                    Unanswered::TimedOut => "That chat was still replying after an hour",
                    Unanswered::NotStarted => "That chat has not started replying",
                })
            })?;
        Ok(json!({
            "status": reply.status,
            "reply": reply.text.trim(),
            "error": reply.error,
            "files_changed": reply.files,
            "checks": reply.checks.as_ref().map(|c| c.status),
            "review": reply.review.as_ref().map(|r| json!({"status": r.status, "text": r.text})),
        })
        .to_string())
    }
}

/// The chats the caller can list and read: the others a person started and kept, in the caller's
/// project, or in every project for a chat run from a chat app.
fn readable(core: &Core, caller: &Thread) -> Vec<Thread> {
    let everywhere = chat_tools::sees_every_project(core, caller);
    core.store
        .threads()
        .into_iter()
        .filter(|t| {
            t.id != caller.id
                && t.parent.is_none()
                && t.archived_at.is_none()
                && (everywhere || t.project_id == caller.project_id)
        })
        .collect()
}

/// The lowercase words of `text`, split at anything that is not a letter or digit.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Whether two words name the same thing: equal, or one starts the other and the shorter has at
/// least four letters, so "margretes" fits "margrete" and "engineering" fits "engineer".
fn alike(a: &str, b: &str) -> bool {
    a == b
        || (a.chars().count().min(b.chars().count()) >= 4 && (a.starts_with(b) || b.starts_with(a)))
}

/// When anything last happened in `chat`.
fn last_active(chat: &Thread) -> u64 {
    chat.messages
        .last()
        .map_or(chat.created_at, |m| m.finished_at.unwrap_or(m.created_at))
}

/// A step's status as `read_chat` words it.
fn step_word(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Pending => "to do",
        StepStatus::Running => "running",
        StepStatus::Done => "done",
        StepStatus::Error => "failed",
    }
}

/// `text` trimmed to its first `limit` characters.
fn clip(text: &str, limit: usize) -> String {
    let text = text.trim();
    match text.char_indices().nth(limit) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_string(),
    }
}

/// `text` trimmed to its last `limit` characters.
fn clip_start(text: &str, limit: usize) -> String {
    let text = text.trim();
    let count = text.chars().count();
    match text.char_indices().nth(count.saturating_sub(limit)) {
        Some((at, _)) if at > 0 => format!("…{}", &text[at..]),
        _ => text.to_string(),
    }
}

/// The chat `args` names, which must be another chat a person started in the caller's project.
fn target(core: &Core, caller: &Thread, args: &Value) -> Result<Thread, Error> {
    let id = required(args, "chat_id")?;
    if id == caller.id {
        return Err("That is this chat".into());
    }
    core.store
        .thread(id)
        .filter(|t| t.parent.is_none() && t.project_id == caller.project_id)
        .ok_or_else(|| "No chat with that id in this project".into())
}

/// Starts a chat in the caller's project with the caller's access, and sends it the prompt.
fn start_chat(core: &Arc<Core>, caller: &Thread, args: &Value) -> Result<String, Error> {
    let prompt = required(args, "prompt")?.to_string();
    let agent = match text(args, "agent") {
        "" => caller.agent,
        name => serde_json::from_value::<AgentKind>(json!(name.to_lowercase()))
            .map_err(|e| format!("Patchwork has no agent {name}: {e}"))?,
    };
    let given = |key: &str| Some(text(args, key).to_string()).filter(|s| !s.is_empty());
    let thread = core.create_thread(NewThread {
        id: None,
        project_id: caller.project_id.clone(),
        agent,
        model: given("model"),
        access_mode: caller.access_mode,
        effort: given("effort"),
        compact_window: None,
        plan_mode: false,
    })?;
    let thread = core.store.thread(&thread.id).unwrap_or(thread);
    let id = thread.id.clone();
    core.emit(AgentEventEnvelope {
        thread_id: id.clone(),
        event: AgentEvent::ThreadCreated { thread },
    });
    // A chat that cannot start leaves no empty chat behind.
    if let Err(e) = core.send_message(id.clone(), prompt, Vec::new(), false) {
        core.delete_thread(&id);
        core.emit(AgentEventEnvelope {
            thread_id: id,
            event: AgentEvent::ThreadDeleted,
        });
        return Err(e);
    }
    Ok(json!({"chat_id": id}).to_string())
}
