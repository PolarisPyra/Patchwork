//! Running the tools a chat's agent drives other chats with (see `crate::chats::tools`):
//! each acts on a chat in the caller's project as a person would from the chat list. A chat
//! app's dispatcher reaches the chats of every project, and has the tools in `dispatch` too.

// Rust guideline compliant 2026-10-09

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use super::dispatch;
use super::wait::Unanswered;
use crate::orchestration::Runner;
use crate::provider::{AgentKind, clip};
use crate::system::now_ms;
use crate::chats::tools::{
    self, ANSWER, LIST, NOTIFY, PROJECTS, READ, SEND, START, STOP, WAIT, fit, named,
};
use crate::chats::{NewChat, Role, Standing, StepStatus, Chat};
use crate::{Error, Core};

/// The longest `wait_for_chat` waits. Under the 65 minutes a CLI waits on one of Patchwork's own
/// tools, so the caller hears that it ran out rather than a dropped call.
const MAX_WAIT: Duration = Duration::from_secs(60 * 60);

/// The most chats `read_chat` offers when a name matches several equally.
const CANDIDATES: usize = 8;

/// The most characters of one message `read_chat` shows.
const MESSAGE_LIMIT: usize = 1500;

/// The most characters of a chat's latest reply `list_chats` shows.
const LATEST_LIMIT: usize = 300;

/// How many of a chat's latest messages `read_chat` shows.
const MESSAGES: usize = 6;

/// How many of a reply's latest steps `read_chat` shows.
const STEPS: usize = 12;

/// A text argument, trimmed, or empty when it is missing.
pub(in crate::orchestration) fn text<'a>(args: &'a Value, key: &str) -> &'a str {
    args[key].as_str().map(str::trim).unwrap_or_default()
}

/// A text argument that must be given.
pub(super) fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, Error> {
    let value = text(args, key);
    if value.is_empty() {
        return Err(format!("Give the {key}").into());
    }
    Ok(value)
}

/// Whether the chat is replying or about to.
pub(super) fn busy(runner: &Runner, chat_id: &str) -> bool {
    runner.is_starting(chat_id) || runner.is_running(chat_id)
}

impl Runner {
    /// Runs the chat tool `tool` for the agent of `chat_id`.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is not offered the tools, an argument is missing or names
    /// a chat outside the caller's project, or the chat cannot be started or messaged.
    pub(crate) fn run_chat_tool(
        &self,
        core: &Arc<Core>,
        chat_id: &str,
        tool: &str,
        args: &Value,
        stopped: &dyn Fn() -> bool,
    ) -> Result<String, Error> {
        let caller = core
            .store
            .chat(chat_id)
            .ok_or("This chat is no longer here")?;
        if !tools::offered(core, &caller) {
            return Err("This chat cannot drive other chats".into());
        }
        match tool {
            LIST => self.list_chats(core, &caller, args),
            READ => self.read_chat(core, &caller, args),
            START => start_chat(core, &caller, args),
            SEND => {
                let target = target(core, &caller, args)?;
                if busy(self, &target.id) {
                    return Err("That chat is still replying; wait for it first".into());
                }
                core.chats().send(
                    &target.id,
                    required(args, "prompt")?.to_string(),
                    Vec::new(),
                    false,
                )?;
                Ok(if dispatch::watch(core, &caller, &target.id) {
                    "Sent. The person is told when it finishes or asks something.".into()
                } else {
                    "Sent. Call wait_for_chat for the reply.".into()
                })
            }
            WAIT => self.wait_for_chat(core, &caller, args, stopped),
            PROJECTS | STOP | ANSWER | NOTIFY if !core.is_dispatcher(&caller.id) => {
                Err(format!("Patchwork has no tool {tool}").into())
            }
            PROJECTS => Ok(dispatch::list_projects(self, core)),
            STOP => {
                let target = target(core, &caller, args)?;
                if !busy(self, &target.id) {
                    return Err("That chat is not replying".into());
                }
                core.chats().stop(&target.id);
                Ok("Stopped.".into())
            }
            ANSWER => {
                let target = target(core, &caller, args)?;
                core.answer_for_dispatcher(&caller.id, &target.id, text(args, "answer"))
            }
            NOTIFY => {
                let target = target(core, &caller, args)?;
                if !busy(self, &target.id) {
                    return Err("That chat is not replying; read_chat shows where it stands".into());
                }
                if !dispatch::watch(core, &caller, &target.id) {
                    return Err("No chat app conversation is open on this chat".into());
                }
                Ok("The person is told when it finishes or asks something. Do not wait.".into())
            }
            _ => Err(format!("Patchwork has no tool {tool}").into()),
        }
    }

    /// The other chats in the caller's project, newest first. A chat run from a chat app sees
    /// every project's chats, each with its project.
    fn list_chats(&self, core: &Core, caller: &Chat, args: &Value) -> Result<String, Error> {
        let everywhere = tools::sees_every_project(core, caller);
        let projects = core.store.settings().projects;
        let project = |t: &Chat| {
            t.project_id
                .as_ref()
                .and_then(|id| projects.iter().find(|p| &p.id == id))
                .map(|p| p.name.clone())
        };
        let mut chats = readable(core, caller);
        let wanted = named(text(args, "project"));
        if everywhere && !wanted.is_empty() {
            let score = |t: &Chat| fit(&wanted, &project(t).unwrap_or_default());
            let best = chats.iter().map(score).max().unwrap_or(0);
            if best == 0 {
                let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();
                return Err(format!(
                    "No project with chats is called that. The projects are: {}",
                    names.join(", ")
                )
                .into());
            }
            chats.retain(|t| score(t) == best);
        }
        // The chats replying now come first.
        chats.sort_by_key(|t| !busy(self, &t.id));
        let chats: Vec<Value> = chats
            .iter()
            .map(|t| {
                let mut chat = self.overview(t);
                if everywhere {
                    chat["project"] = json!(project(t));
                }
                chat
            })
            .collect();
        if chats.is_empty() {
            return Ok(if everywhere {
                "There are no other chats.".into()
            } else {
                "There are no other chats in this project.".into()
            });
        }
        Ok(Value::Array(chats).to_string())
    }

    /// What `list_chats` tells of `chat`: where it stands, what it is doing, and the end of its
    /// latest reply.
    fn overview(&self, chat: &Chat) -> Value {
        let replying = busy(self, &chat.id);
        let mut overview = json!({
            "chat_id": chat.id,
            "title": chat.title,
            "agent": chat.agent,
            "model": chat.model,
            "replying": replying,
            "state": self.state(chat),
        });
        if replying {
            overview["doing"] = json!(chat.doing());
        }
        let reply = chat
            .messages
            .iter()
            .rfind(|m| m.role == Role::Assistant && !m.queued && !m.text.trim().is_empty());
        if let Some(reply) = reply {
            // A reply still being written shows its newest part.
            overview["latest"] = json!(if reply.is_streaming() {
                clip_start(&reply.text, LATEST_LIMIT)
            } else {
                clip(&reply.text, LATEST_LIMIT)
            });
        }
        overview
    }

    /// Where `chat` stands, in a word or two.
    fn state(&self, chat: &Chat) -> &'static str {
        match chat.standing_while(busy(self, &chat.id)) {
            Standing::Waiting => "waiting for an answer",
            Standing::Working => "replying",
            Standing::Failed => "failed",
            Standing::Done => "done",
        }
    }

    /// How far the chat `args` names has got, or the chats its name fits equally well.
    fn read_chat(&self, core: &Core, caller: &Chat, args: &Value) -> Result<String, Error> {
        let projects = core.store.settings().projects;
        let project = |t: &Chat| {
            t.project_id
                .as_ref()
                .and_then(|id| projects.iter().find(|p| &p.id == id))
                .map(|p| p.name.clone())
        };
        let chats = readable(core, caller);
        let id = text(args, "chat_id");
        let chat = if id.is_empty() {
            let name = required(args, "name")?;
            if named(name).is_empty() {
                return Err("Give the chat's title or project as the name".into());
            }
            let best = tools::matching(core, &chats, name);
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
                            let mut chat = self.overview(t);
                            chat["project"] = json!(project(t));
                            chat
                        })
                        .collect();
                    return Ok(json!({
                        "several_chats_match": candidates,
                        "next": "When the person asked about all of these, such as a whole project, report on each from what is here. Otherwise call read_chat with the chat_id of the one meant, or ask which.",
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
        core.chat_app_visits(&caller.id, &chat.id);
        Ok(self.progress(chat, project(chat).as_deref()).to_string())
    }

    /// What `read_chat` tells of `chat`: its task, where it stands, and what it has done.
    fn progress(&self, chat: &Chat, project: Option<&str>) -> Value {
        let replying = busy(self, &chat.id);
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
            "state": self.state(chat),
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
                let text = if m.is_streaming() {
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
        caller: &Chat,
        args: &Value,
        stopped: &dyn Fn() -> bool,
    ) -> Result<String, Error> {
        let target = target(core, caller, args)?;
        let flags = self.turn_flags(&caller.id)?;
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
fn readable(core: &Core, caller: &Chat) -> Vec<Chat> {
    let everywhere = tools::sees_every_project(core, caller);
    core.store
        .chats()
        .into_iter()
        .filter(|t| {
            t.id != caller.id
                && t.parent.is_none()
                && t.archived_at.is_none()
                && (everywhere || t.project_id == caller.project_id)
                && reachable(caller, t)
        })
        .collect()
}

/// Whether the caller may see and drive chat `t`: an agent's run reaches only its own agent's
/// runs, so it can't hand work to a chat outside the agent.
fn reachable(caller: &Chat, t: &Chat) -> bool {
    caller.agent_id.is_none() || t.agent_id == caller.agent_id
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

/// `text` trimmed to its last `limit` characters.
fn clip_start(text: &str, limit: usize) -> String {
    let text = text.trim();
    let count = text.chars().count();
    match text.char_indices().nth(count.saturating_sub(limit)) {
        Some((at, _)) if at > 0 => format!("…{}", &text[at..]),
        _ => text.to_string(),
    }
}

/// The chat `args` names, which must be another chat a person started: in the caller's project,
/// or in any for a dispatcher.
fn target(core: &Core, caller: &Chat, args: &Value) -> Result<Chat, Error> {
    let id = required(args, "chat_id")?;
    if id == caller.id {
        return Err("That is this chat".into());
    }
    let everywhere = core.is_dispatcher(&caller.id);
    let chat = core
        .store
        .chat(id)
        .filter(|t| {
            t.parent.is_none()
                && (everywhere || t.project_id == caller.project_id)
                && reachable(caller, t)
        })
        .ok_or(if everywhere {
            "No chat with that id; list_chats lists them"
        } else {
            "No chat with that id in this project"
        })?;
    core.chat_app_visits(&caller.id, &chat.id);
    Ok(chat)
}

/// Starts a chat with the caller's access and sends it the prompt: in the caller's project on
/// the caller's agent, or for a dispatcher in the project named, on the person's usual agent.
fn start_chat(core: &Arc<Core>, caller: &Chat, args: &Value) -> Result<String, Error> {
    let prompt = required(args, "prompt")?.to_string();
    let dispatcher = core.is_dispatcher(&caller.id);
    let settings = core.store.settings();
    let agent = match text(args, "agent") {
        "" if dispatcher => settings.default_agent(),
        "" => caller.agent,
        name => serde_json::from_value::<AgentKind>(json!(name.to_lowercase()))
            .map_err(|e| format!("Patchwork has no agent {name}: {e}"))?,
    };
    let given = |key: &str| Some(text(args, key).to_string()).filter(|s| !s.is_empty());
    let (project_id, model) = if dispatcher {
        (
            dispatch::project(&settings, text(args, "project"))?,
            given("model").or_else(|| settings.tokens.default_models.get(&agent).cloned()),
        )
    } else {
        (caller.project_id.clone(), given("model"))
    };
    // A chat an agent's run starts is part of that run, under the same limits.
    let agent_id = caller.agent_id.clone().or_else(|| {
        let parent = caller.parent.as_ref()?;
        core.store.chat(&parent.chat_id)?.agent_id
    });
    let new = NewChat {
        id: None,
        project_id,
        agent,
        model,
        access_mode: caller.access_mode,
        effort: given("effort"),
        compact_window: None,
    };
    let chat = core
        .chats()
        .start(new, prompt, Vec::new(), |chat| match agent_id {
            Some(agent_id) => core
                .store
                .update_chat(&chat.id, |t| t.agent_id = Some(agent_id)),
            None => Ok(()),
        })?;
    let id = chat.id;
    core.chat_app_visits(&caller.id, &id);
    if dispatch::watch(core, caller, &id) {
        return Ok(json!({
            "chat_id": id,
            "note": "Started. The person is told here when it finishes or asks something.",
        })
        .to_string());
    }
    Ok(json!({"chat_id": id}).to_string())
}
