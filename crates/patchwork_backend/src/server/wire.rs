//! The app's side of the server: the methods a phone or the web client calls, each decoded into
//! its `patchwork_rpc_protocol` params, answered by the core's chat operations, and encoded in the
//! shapes the web client's types (`packages/contracts`) have. Only what the clients need to
//! follow and drive a chat is here; every other method answers that this version does not have
//! it.

// Rust guideline compliant 2026-10-09

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

use patchwork_rpc_protocol::app::{AgentInfo, ProjectInfo, ServerInfo, Settings};
use patchwork_rpc_protocol::chats::{self as client, ChatParams, NewChatParams};
use patchwork_rpc_protocol::messages::{self as wire, ApprovalState, PlanStepStatus, ToolStatus};
use patchwork_rpc_protocol::methods::{
    AnswerApproval, CreateChat, GetServerInfo, GetSettings, ListAgents, ListChats, ListMessages,
    ListProjects, ListProviders, SendTurn, StopTurn,
};
use patchwork_rpc_protocol::pushes::{ChatGone, MessageGone, Push};
use patchwork_rpc_protocol::turns::{AnswerParams, SendParams, Sent};
use patchwork_rpc_protocol::{Method, decode, encode};
use serde_json::Value;

use patchwork_rpc_server::{Backend, PairingStore, Touched};
use crate::chats::{
    AccessMode, ApprovalStatus, Chat, Message, MessageStatus, NewChat, Part, Role, StepStatus,
};
use crate::events::ChatEvent;
use crate::provider::AgentKind;
use crate::{Core, Error};

impl Backend for Core {
    fn pairing_store(&self) -> &dyn PairingStore {
        &self.store
    }

    fn call(self: Arc<Self>, method: &str, params: &Value) -> Result<Value, Error> {
        let core = &self;
        match method {
            GetServerInfo::NAME => answer::<GetServerInfo>(params, |_| {
                Ok(ServerInfo {
                    version: crate::system::VERSION.to_string(),
                    platform: platform().to_string(),
                    home_dir: crate::system::files::paths::home_dir(),
                    data_dir: crate::system::dirs().data.clone(),
                })
            }),
            ListProjects::NAME => answer::<ListProjects>(params, |_| Ok(projects(core))),
            ListChats::NAME => answer::<ListChats>(params, |_| {
                Ok(core
                    .list_chats()
                    .iter()
                    .filter(|c| c.parent.is_none())
                    .map(|c| client_chat(core, c))
                    .collect())
            }),
            ListMessages::NAME => answer::<ListMessages>(params, |ChatParams { chat_id }| {
                let chat = core.chat(&chat_id)?;
                Ok(chat
                    .messages
                    .iter()
                    .map(|m| client_message(&chat.id, m))
                    .collect())
            }),
            CreateChat::NAME => answer::<CreateChat>(params, |new| create_chat(core, new)),
            SendTurn::NAME => answer::<SendTurn>(params, |SendParams { chat_id, text }| {
                let message = core.chats().send(&chat_id, text, Vec::new(), false)?;
                Ok(Sent {
                    message_id: message.id,
                    steered: message.steer,
                    queued: message.queued,
                })
            }),
            StopTurn::NAME => answer::<StopTurn>(params, |ChatParams { chat_id }| {
                core.chats().stop(&chat_id);
                Ok(())
            }),
            AnswerApproval::NAME => answer::<AnswerApproval>(params, |p: AnswerParams| {
                let answers: HashMap<String, Vec<String>> =
                    p.answers.unwrap_or_default().into_iter().collect();
                core.chats().answer(
                    &p.chat_id,
                    &p.approval_id,
                    p.allow.unwrap_or(false),
                    answers,
                    None,
                )
            }),
            GetSettings::NAME => answer::<GetSettings>(params, |_| {
                Ok(Settings {
                    default_agent: agent_id(core.store.settings().default_agent()),
                    default_access_mode: access_mode(AccessMode::default()),
                    show_thoughts: true,
                    terminal_shell: None,
                    terminal_font_size: 12,
                })
            }),
            ListAgents::NAME => answer::<ListAgents>(params, |_| {
                Ok(core
                    .store
                    .settings()
                    .agents
                    .iter()
                    .map(|agent| AgentInfo {
                        id: agent_id(*agent),
                        name: agent.identity().name.to_string(),
                        installed: true,
                        version: None,
                        signed_in: None,
                        path: None,
                    })
                    .collect())
            }),
            ListProviders::NAME => answer::<ListProviders>(params, |_| Ok(Vec::new())),
            _ => Err(format!("{method} is not available in this version of Patchwork").into()),
        }
    }

    fn pushes(&self, touched: &Touched) -> Vec<(&'static str, Value)> {
        client_pushes(self, touched)
            .iter()
            .filter_map(|push| Some((push.name(), push.data().ok()?)))
            .collect()
    }
}

/// Decodes `params` for `M`, runs `op` on them, and encodes its answer.
fn answer<M: Method>(
    params: &Value,
    op: impl FnOnce(M::Params) -> Result<M::Output, Error>,
) -> Result<Value, Error> {
    let output = op(decode::<M>(params)?)?;
    Ok(encode::<M>(&output)?)
}

/// The pushes that bring a client up to date after the change `touched` describes.
fn client_pushes(core: &Core, touched: &Touched) -> Vec<Push> {
    let deleted = || {
        vec![Push::ChatDeleted(ChatGone {
            id: touched.chat_id.clone(),
        })]
    };
    if touched.deleted {
        return deleted();
    }
    let Some(chat) = core.store.chat(&touched.chat_id) else {
        return deleted();
    };
    // A subagent's chat is not listed, so its changes are not news to a client.
    if chat.parent.is_some() {
        return Vec::new();
    }
    let mut pushes = vec![Push::ChatUpserted(client_chat(core, &chat))];
    pushes.extend(
        chat.messages
            .iter()
            .filter(|m| touched.messages.contains(&m.id))
            .map(|m| Push::MessageUpserted(client_message(&chat.id, m))),
    );
    pushes.extend(touched.removed.iter().map(|id| {
        Push::MessageDeleted(MessageGone {
            chat_id: chat.id.clone(),
            id: id.clone(),
        })
    }));
    pushes
}

/// What a chat event changed, for the pushes.
pub(super) fn touched_by(chat_id: &str, event: &ChatEvent) -> Touched {
    let mut touched = Touched {
        chat_id: chat_id.to_string(),
        ..Touched::default()
    };
    match event {
        ChatEvent::MessageRemoved { message_id } => touched.removed.push(message_id.clone()),
        ChatEvent::ChatDeleted => touched.deleted = true,
        // The rest change a message clients show, or else the chat, which is pushed anew.
        _ => touched
            .messages
            .extend(event.client_message_id().map(String::from)),
    }
    touched
}

/// The platform as Node names it, which the web client expects.
fn platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

fn projects(core: &Core) -> Vec<ProjectInfo> {
    core.store
        .settings()
        .projects
        .iter()
        .enumerate()
        .map(|(position, p)| ProjectInfo {
            id: p.id.clone(),
            name: p.name.clone(),
            path: p.path.clone(),
            position,
            created_at: 0,
            exists: p.host().is_some() || Path::new(&p.path).is_dir(),
        })
        .collect()
}

/// `chats.create`: a chat in the project, on the agent and model given or the settings' defaults.
fn create_chat(core: &Arc<Core>, new: NewChatParams) -> Result<client::Chat, Error> {
    let settings = core.store.settings();
    let agent = match new.agent {
        Some(id) => serde_json::from_value::<AgentKind>(Value::String(id.clone()))
            .map_err(|_unknown| format!("Patchwork has no agent {id}"))?,
        None => settings.default_agent(),
    };
    let access_mode = match new.access_mode.as_deref() {
        None => AccessMode::default(),
        Some(name) => client::AccessMode::named(name)
            .map(core_access_mode)
            .ok_or_else(|| format!("Bad params for {}: no access mode {name}", CreateChat::NAME))?,
    };
    let model = new
        .model
        .or_else(|| settings.tokens.default_models.get(&agent).cloned());
    // Announced as starting a chat elsewhere is, so the window and other clients list it.
    let chat = core.chats().create(NewChat {
        id: None,
        project_id: new.project_id,
        agent,
        model,
        access_mode,
        effort: new.effort,
        compact_window: None,
    })?;
    Ok(client_chat(core, &chat))
}

/// An agent's id as the web client has it: its serde name.
fn agent_id(agent: AgentKind) -> String {
    match serde_json::to_value(agent) {
        Ok(Value::String(id)) => id,
        _ => String::new(),
    }
}

/// An access mode as the web client names it.
fn access_mode(mode: AccessMode) -> client::AccessMode {
    match mode {
        AccessMode::Supervised => client::AccessMode::Supervised,
        AccessMode::AutoAcceptEdits => client::AccessMode::AutoAcceptEdits,
        AccessMode::Auto => client::AccessMode::Auto,
        AccessMode::FullAccess => client::AccessMode::FullAccess,
    }
}

/// The app's access mode for one a client names.
fn core_access_mode(mode: client::AccessMode) -> AccessMode {
    match mode {
        client::AccessMode::Supervised => AccessMode::Supervised,
        client::AccessMode::AutoAcceptEdits => AccessMode::AutoAcceptEdits,
        client::AccessMode::Auto => AccessMode::Auto,
        client::AccessMode::FullAccess => AccessMode::FullAccess,
    }
}

/// A chat as the web client's `Chat` type has it.
fn client_chat(core: &Core, chat: &Chat) -> client::Chat {
    let running = core.runner.is_running(&chat.id) || core.runner.is_starting(&chat.id);
    client::Chat {
        id: chat.id.clone(),
        project_id: chat.project_id.clone(),
        title: chat.title.clone(),
        agent: agent_id(chat.agent),
        model: chat.model.clone(),
        access_mode: access_mode(chat.access_mode),
        session_id: chat.session_id.clone(),
        effort: chat.effort.clone(),
        status: chat.client_status(running),
        position: chat.position,
        created_at: chat.created_at,
        updated_at: crate::chats::tools::last_active(chat),
        archived_at: chat.archived_at,
        pinned: chat.pinned,
        worktree_path: None,
        branch: None,
        checkpoint_ref: None,
    }
}

/// A message as the web client's `Message` type has it.
fn client_message(chat_id: &str, m: &Message) -> wire::Message {
    let status = if m.queued {
        wire::MessageStatus::Queued
    } else {
        match m.status {
            MessageStatus::Streaming => wire::MessageStatus::Streaming,
            MessageStatus::Complete => wire::MessageStatus::Complete,
            MessageStatus::Error => wire::MessageStatus::Failed,
            MessageStatus::Cancelled | MessageStatus::Interrupted => wire::MessageStatus::Stopped,
        }
    };
    wire::Message {
        id: m.id.clone(),
        chat_id: chat_id.to_string(),
        role: match m.role {
            Role::User => wire::Role::User,
            Role::Assistant => wire::Role::Assistant,
        },
        parts: parts(m),
        status,
        files: m.files.clone(),
        created_at: m.created_at,
        ended_at: m.finished_at,
        usage: m.usage.map(|u| wire::Usage {
            input: u.input,
            output: u.output,
            cache_read: u.cache_read,
            cache_write: u.cache_write,
            context: u.context,
            window: u.context_window,
        }),
        interrupted: m.status == MessageStatus::Interrupted,
    }
}

/// A message's parts in the web client's kinds. Messages saved before parts were kept, and the
/// person's own, have only their text, tools and approvals.
fn parts(m: &Message) -> Vec<wire::Part> {
    let tool = |id: &str| {
        m.tools
            .iter()
            .find(|t| t.id == id)
            .map(|t| wire::Part::Tool {
                id: t.id.clone(),
                name: t.name.clone(),
                summary: t.summary.clone(),
                status: match t.status {
                    StepStatus::Pending | StepStatus::Running => ToolStatus::Running,
                    StepStatus::Done => ToolStatus::Done,
                    StepStatus::Error => ToolStatus::Failed,
                },
                output: t.output.clone().unwrap_or_default(),
            })
    };
    let approval = |id: &str| {
        m.approvals
            .iter()
            .find(|a| a.id == id)
            .map(|a| wire::Part::Approval {
                id: a.id.clone(),
                title: a.title.clone(),
                detail: a.detail.clone(),
                questions: a
                    .questions
                    .iter()
                    .map(|q| wire::Question {
                        id: q.id.clone(),
                        header: q.header.clone(),
                        question: q.question.clone(),
                        options: q
                            .options
                            .iter()
                            .map(|o| wire::QuestionOption {
                                label: o.label.clone(),
                                description: o.description.clone(),
                            })
                            .collect(),
                        multi_select: q.multi_select,
                    })
                    .collect(),
                state: match a.status {
                    ApprovalStatus::Pending => ApprovalState::Pending,
                    ApprovalStatus::Allowed => ApprovalState::Allowed,
                    ApprovalStatus::Denied => ApprovalState::Denied,
                },
                answers: a
                    .answers
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<BTreeMap<_, _>>(),
            })
    };
    let mut parts = Vec::new();
    if m.parts.is_empty() {
        if !m.text.is_empty() {
            parts.push(wire::Part::Text {
                text: m.text.clone(),
            });
        }
        parts.extend(m.tools.iter().filter_map(|t| tool(&t.id)));
        parts.extend(m.approvals.iter().filter_map(|a| approval(&a.id)));
    }
    for part in &m.parts {
        match part {
            Part::Text { text } | Part::Plan { text } => {
                parts.push(wire::Part::Text { text: text.clone() });
            }
            Part::Thinking { text, .. } => {
                parts.push(wire::Part::Reasoning { text: text.clone() });
            }
            Part::Tool { id } => parts.extend(tool(id)),
            Part::Approval { id } => parts.extend(approval(id)),
            Part::Compaction { .. } => {}
        }
    }
    if !m.plan.is_empty() {
        let steps = m
            .plan
            .iter()
            .map(|s| wire::PlanStep {
                text: s.text.clone(),
                status: match s.status {
                    StepStatus::Pending | StepStatus::Error => PlanStepStatus::Pending,
                    StepStatus::Running => PlanStepStatus::InProgress,
                    StepStatus::Done => PlanStepStatus::Completed,
                },
            })
            .collect();
        parts.push(wire::Part::Plan { steps });
    }
    if let Some(error) = &m.error {
        parts.push(wire::Part::Error {
            text: error.clone(),
        });
    }
    parts
}

#[cfg(test)]
mod tests {
    //! Pins the JSON phones and the web client read, and which pushes each event makes.

    use std::path::PathBuf;
    use std::sync::Arc;

    use serde_json::json;

    use super::{client_chat, client_message, platform, touched_by};
    use crate::Core;
    use crate::chats::{
        AccessMode, Approval, ApprovalStatus, Chat, ChatParent, Message, MessageStatus, Part,
        PlanStep, Question, QuestionOption, Role, StepStatus, TokenUsage, ToolCall,
    };
    use crate::projects::Project;
    use patchwork_rpc_server::{Backend, Touched};
    use crate::events::ChatEvent;
    use crate::provider::AgentKind;

    fn chat_json(core: &Core, chat: &Chat) -> serde_json::Value {
        serde_json::to_value(client_chat(core, chat)).expect("JSON")
    }

    fn message_json(chat_id: &str, m: &Message) -> serde_json::Value {
        serde_json::to_value(client_message(chat_id, m)).expect("JSON")
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("patchwork-wire-{}", crate::system::new_id()));
        std::fs::create_dir_all(&dir).expect("the temporary folder is made");
        dir
    }

    fn core() -> (Arc<Core>, PathBuf) {
        let dir = temp_dir();
        (Core::for_tests(&dir, Box::new(|_| {})), dir)
    }

    fn message(role: Role, status: MessageStatus) -> Message {
        let mut m = Message::new(role, "hello".into(), status);
        m.id = "m-1".into();
        m.created_at = 5;
        m
    }

    fn chat(messages: Vec<Message>) -> Chat {
        let mut chat = Chat::new("Title".into(), AgentKind::Claude, AccessMode::Supervised);
        chat.id = "c-1".into();
        chat.created_at = 1;
        chat.messages = messages.into_iter().map(Arc::new).collect();
        chat
    }

    /// The answer to `method` as the bytes sent, or the error's message.
    fn call(core: &Arc<Core>, method: &str, params: &serde_json::Value) -> Result<String, String> {
        Arc::clone(core)
            .call(method, params)
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    /// A chat with every kind of part, usage and an interrupted message, for the byte pins.
    fn full_chat() -> Chat {
        let mut m = message(Role::Assistant, MessageStatus::Interrupted);
        m.tools.push(
            serde_json::from_value::<ToolCall>(json!({
                "id": "t-1",
                "name": "Bash",
                "summary": "ls",
                "status": "done",
                "output": "a.txt",
            }))
            .expect("a tool call"),
        );
        let mut approval = Approval::pending("Ask".into(), "which".into());
        approval.id = "a-1".into();
        approval.status = ApprovalStatus::Allowed;
        approval.questions.push(Question {
            id: "q-1".into(),
            header: "Lib".into(),
            question: "Which?".into(),
            options: vec![QuestionOption {
                label: "serde".into(),
                description: "the usual".into(),
            }],
            multi_select: true,
        });
        approval.answers.insert("q-1".into(), vec!["serde".into()]);
        m.approvals.push(approval);
        m.parts = vec![
            Part::Text { text: "one".into() },
            Part::Thinking {
                text: "hmm".into(),
                started_at: 1,
                ended_at: None,
            },
            Part::Tool { id: "t-1".into() },
            Part::Approval { id: "a-1".into() },
            Part::Compaction {
                summary: None,
                done: true,
            },
            Part::Plan {
                text: "plan".into(),
            },
        ];
        m.plan.push(PlanStep {
            text: "Do".into(),
            status: StepStatus::Done,
        });
        m.files.push("a.txt".into());
        m.usage = Some(TokenUsage {
            input: 1,
            output: 2,
            cache_read: 3,
            cache_write: 4,
            context: 5,
            context_window: Some(6),
            ..TokenUsage::default()
        });
        let mut queued = message(Role::User, MessageStatus::Complete);
        queued.id = "m-2".into();
        queued.queued = true;
        let mut chat = chat(vec![m, queued]);
        chat.project_id = Some("p-1".into());
        chat.model = Some("opus".into());
        chat.effort = Some("high".into());
        chat.session_id = Some("s-1".into());
        chat.access_mode = AccessMode::FullAccess;
        chat.position = -3;
        chat.archived_at = Some(7);
        chat.pinned = true;
        chat
    }

    const FULL_CHAT: &str = r#"{"id":"c-1","projectId":"p-1","title":"Title","agent":"claude","model":"opus","accessMode":"fullAccess","sessionId":"s-1","effort":"high","status":"idle","position":-3,"createdAt":1,"updatedAt":5,"archivedAt":7,"pinned":true,"worktreePath":null,"branch":null,"checkpointRef":null}"#;

    const FULL_MESSAGES: &str = r#"[{"id":"m-1","chatId":"c-1","role":"assistant","parts":[{"kind":"text","text":"one"},{"kind":"reasoning","text":"hmm"},{"kind":"tool","id":"t-1","name":"Bash","summary":"ls","status":"done","output":"a.txt"},{"kind":"approval","id":"a-1","title":"Ask","detail":"which","questions":[{"id":"q-1","header":"Lib","question":"Which?","options":[{"label":"serde","description":"the usual"}],"multiSelect":true}],"state":"allowed","answers":{"q-1":["serde"]}},{"kind":"text","text":"plan"},{"kind":"plan","steps":[{"text":"Do","status":"completed"}]}],"status":"stopped","files":["a.txt"],"createdAt":5,"endedAt":null,"usage":{"input":1,"output":2,"cacheRead":3,"cacheWrite":4,"context":5,"window":6},"interrupted":true},{"id":"m-2","chatId":"c-1","role":"user","parts":[{"kind":"text","text":"hello"}],"status":"queued","files":[],"createdAt":5,"endedAt":null,"usage":null}]"#;

    #[test]
    fn list_bytes_are_pinned() {
        let (core, dir) = core();
        let mut subagent = full_chat();
        subagent.id = "c-2".into();
        subagent.parent = Some(ChatParent {
            chat_id: "c-1".into(),
            tool_id: "t-1".into(),
        });
        core.store
            .insert_chats(vec![full_chat(), subagent])
            .expect("the chats are kept");
        assert_eq!(
            call(&core, "chats.list", &json!({})),
            Ok(format!("[{FULL_CHAT}]"))
        );
        assert_eq!(
            call(&core, "messages.list", &json!({ "chatId": "c-1" })),
            Ok(FULL_MESSAGES.to_string())
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn push_bytes_are_pinned() {
        let (core, dir) = core();
        core.store
            .insert_chats(vec![full_chat()])
            .expect("the chat is kept");
        let pushes = |touched: Touched| {
            core.pushes(&touched)
                .into_iter()
                .map(|(name, data)| format!("{name} {data}"))
                .collect::<Vec<_>>()
        };
        let message = FULL_MESSAGES
            .strip_prefix('[')
            .and_then(|s| s.split_once(r#",{"id":"m-2""#))
            .map(|(first, _)| first.to_string())
            .expect("the first message");
        assert_eq!(
            pushes(Touched {
                chat_id: "c-1".into(),
                messages: vec!["m-1".into()],
                removed: vec!["m-0".into()],
                deleted: false,
            }),
            vec![
                format!("chat.upserted {FULL_CHAT}"),
                format!("message.upserted {message}"),
                r#"message.deleted {"chatId":"c-1","id":"m-0"}"#.to_string(),
            ]
        );
        let deleted = vec![r#"chat.deleted {"id":"c-9"}"#.to_string()];
        assert_eq!(
            pushes(Touched {
                chat_id: "c-9".into(),
                ..Touched::default()
            }),
            deleted
        );
        assert_eq!(
            pushes(Touched {
                chat_id: "c-1".into(),
                deleted: true,
                ..Touched::default()
            }),
            vec![r#"chat.deleted {"id":"c-1"}"#.to_string()]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn settings_agents_and_projects_bytes_are_pinned() {
        let (core, dir) = core();
        core.store
            .update_settings(|s| {
                s.agents = vec![AgentKind::Codex, AgentKind::Claude];
                s.projects = vec![
                    Project {
                        id: "p-1".into(),
                        name: "Here".into(),
                        path: dir.to_string_lossy().into_owned(),
                    },
                    Project {
                        id: "p-2".into(),
                        name: "Gone".into(),
                        path: "/no/such/folder".into(),
                    },
                ];
            })
            .expect("the settings are saved");
        assert_eq!(
            call(&core, "settings.get", &json!({})),
            Ok(r#"{"defaultAgent":"codex","defaultAccessMode":"fullAccess","showThoughts":true,"terminalShell":null,"terminalFontSize":12}"#.to_string())
        );
        assert_eq!(
            call(&core, "agents.list", &json!(null)),
            Ok(r#"[{"id":"codex","name":"Codex","installed":true,"version":null,"signedIn":null,"path":null},{"id":"claude","name":"Claude Code","installed":true,"version":null,"signedIn":null,"path":null}]"#.to_string())
        );
        assert_eq!(
            call(&core, "providers.list", &json!({})),
            Ok("[]".to_string())
        );
        assert_eq!(
            call(&core, "projects.list", &json!({})),
            Ok(format!(
                r#"[{{"id":"p-1","name":"Here","path":{},"position":0,"createdAt":0,"exists":true}},{{"id":"p-2","name":"Gone","path":"/no/such/folder","position":1,"createdAt":0,"exists":false}}]"#,
                json!(dir.to_string_lossy())
            ))
        );
        let info = format!(
            r#"{{"version":{},"platform":{},"homeDir":{},"dataDir":{}}}"#,
            json!(crate::system::VERSION),
            json!(platform()),
            json!(crate::system::files::paths::home_dir()),
            json!(crate::system::dirs().data),
        );
        assert_eq!(call(&core, "server.info", &json!({})), Ok(info));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn create_bytes_are_pinned() {
        let (core, dir) = core();
        let answer = call(
            &core,
            "chats.create",
            &json!({ "agent": "codex", "accessMode": "auto", "model": "gpt", "effort": "low" }),
        )
        .expect("the chat is made");
        let value: serde_json::Value = serde_json::from_str(&answer).expect("JSON");
        let id = value["id"].as_str().expect("an id");
        let created = value["createdAt"].as_u64().expect("a time");
        assert_eq!(
            answer,
            format!(
                r#"{{"id":"{id}","projectId":null,"title":"New chat","agent":"codex","model":"gpt","accessMode":"auto","sessionId":null,"effort":"low","status":"idle","position":-{created},"createdAt":{created},"updatedAt":{created},"archivedAt":null,"pinned":false,"worktreePath":null,"branch":null,"checkpointRef":null}}"#
            )
        );
        assert!(core.store.chat(id).is_some());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn errors_are_pinned() {
        let (core, dir) = core();
        let error = |method: &str, params| call(&core, method, &params).expect_err("an error");
        assert_eq!(
            error("chats.update", json!({})),
            "chats.update is not available in this version of Patchwork"
        );
        assert_eq!(
            error("messages.list", json!({})),
            "Bad params for messages.list: chatId is required"
        );
        assert_eq!(
            error("turn.send", json!({ "chatId": "c-1" })),
            "Bad params for turn.send: text is required"
        );
        assert_eq!(
            error("turn.stop", json!(null)),
            "Bad params for turn.stop: chatId is required"
        );
        assert_eq!(
            error("approval.answer", json!({ "chatId": "c-1" })),
            "Bad params for approval.answer: approvalId is required"
        );
        assert_eq!(
            error("chats.create", json!({ "agent": "nobody" })),
            "Patchwork has no agent nobody"
        );
        assert_eq!(
            error("chats.create", json!({ "accessMode": "wild" })),
            "Bad params for chats.create: no access mode wild"
        );
        assert_eq!(
            call(&core, "turn.stop", &json!({ "chatId": "c-9" })),
            Ok("null".to_string())
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn chat_status_reads_the_last_message() {
        let (core, dir) = core();
        let status = |messages| chat_json(&core, &chat(messages))["status"].clone();
        assert_eq!(status(Vec::new()), "idle");
        assert_eq!(
            status(vec![message(Role::Assistant, MessageStatus::Complete)]),
            "idle"
        );
        assert_eq!(
            status(vec![message(Role::Assistant, MessageStatus::Error)]),
            "error"
        );
        // A message after the failed one clears it, whatever its role.
        assert_eq!(
            status(vec![
                message(Role::Assistant, MessageStatus::Error),
                message(Role::User, MessageStatus::Complete),
            ]),
            "idle"
        );
        // Without a running turn a streaming message reads as idle.
        assert_eq!(
            status(vec![message(Role::Assistant, MessageStatus::Streaming)]),
            "idle"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn chat_json_has_the_client_fields() {
        let (core, dir) = core();
        let value = chat_json(&core, &chat(Vec::new()));
        assert_eq!(
            value,
            json!({
                "id": "c-1",
                "projectId": null,
                "title": "Title",
                "agent": "claude",
                "model": null,
                "accessMode": "supervised",
                "sessionId": null,
                "effort": null,
                "status": "idle",
                "position": 0,
                "createdAt": 1,
                "updatedAt": 1,
                "archivedAt": null,
                "pinned": false,
                "worktreePath": null,
                "branch": null,
                "checkpointRef": null,
            })
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn message_status_names_match_the_client() {
        let status = |status, queued| {
            let mut m = message(Role::Assistant, status);
            m.queued = queued;
            message_json("c-1", &m)["status"].clone()
        };
        assert_eq!(status(MessageStatus::Streaming, false), "streaming");
        assert_eq!(status(MessageStatus::Complete, false), "complete");
        assert_eq!(status(MessageStatus::Error, false), "failed");
        assert_eq!(status(MessageStatus::Cancelled, false), "stopped");
        assert_eq!(status(MessageStatus::Interrupted, false), "stopped");
        assert_eq!(status(MessageStatus::Complete, true), "queued");
        let interrupted = message_json("c-1", &message(Role::User, MessageStatus::Interrupted));
        assert_eq!(interrupted["interrupted"], true);
    }

    #[test]
    fn message_json_has_the_client_fields() {
        let mut m = message(Role::Assistant, MessageStatus::Error);
        m.tools.push(
            serde_json::from_value::<ToolCall>(json!({
                "id": "t-1",
                "name": "Bash",
                "summary": "ls",
                "status": "running",
            }))
            .expect("a tool call"),
        );
        let mut approval = Approval::pending("Run command".into(), "ls".into());
        approval.id = "a-1".into();
        m.approvals.push(approval);
        m.plan.push(PlanStep {
            text: "Look".into(),
            status: StepStatus::Running,
        });
        m.error = Some("boom".into());
        m.finished_at = Some(9);
        assert_eq!(
            message_json("c-1", &m),
            json!({
                "id": "m-1",
                "chatId": "c-1",
                "role": "assistant",
                "parts": [
                    { "kind": "text", "text": "hello" },
                    {
                        "kind": "tool",
                        "id": "t-1",
                        "name": "Bash",
                        "summary": "ls",
                        "status": "running",
                        "output": "",
                    },
                    {
                        "kind": "approval",
                        "id": "a-1",
                        "title": "Run command",
                        "detail": "ls",
                        "questions": [],
                        "state": "pending",
                        "answers": {},
                    },
                    { "kind": "plan", "steps": [{ "text": "Look", "status": "inProgress" }] },
                    { "kind": "error", "text": "boom" },
                ],
                "status": "failed",
                "files": [],
                "createdAt": 5,
                "endedAt": 9,
                "usage": null,
            })
        );
    }

    #[test]
    fn events_touch_what_they_change() {
        let touched = |event: ChatEvent| touched_by("c-1", &event);
        let messages = |ids: &[&str]| Touched {
            chat_id: "c-1".into(),
            messages: ids.iter().map(ToString::to_string).collect(),
            ..Touched::default()
        };
        let id = || "m-1".to_string();
        assert_eq!(
            touched(ChatEvent::MessageStarted {
                message: message(Role::User, MessageStatus::Complete)
            }),
            messages(&["m-1"])
        );
        assert_eq!(
            touched(ChatEvent::TextDelta {
                message_id: id(),
                text: "x".into()
            }),
            messages(&["m-1"])
        );
        assert_eq!(
            touched(ChatEvent::TurnCompleted {
                message_id: id(),
                status: MessageStatus::Complete,
                error: None
            }),
            messages(&["m-1"])
        );
        assert_eq!(
            touched(ChatEvent::Compacting { message_id: id() }),
            messages(&["m-1"])
        );
        // Changes clients do not show push the chat alone.
        assert_eq!(
            touched(ChatEvent::Retrying {
                message_id: id(),
                reason: "x".into(),
                attempt: None,
                max_attempts: None
            }),
            messages(&[])
        );
        assert_eq!(
            touched(ChatEvent::Cursor {
                message_id: id(),
                cursor: "x".into()
            }),
            messages(&[])
        );
        assert_eq!(
            touched(ChatEvent::TitleUpdated { title: "x".into() }),
            messages(&[])
        );
        assert_eq!(touched(ChatEvent::ChatAppsChanged), messages(&[]));
        assert_eq!(
            touched(ChatEvent::MessageRemoved { message_id: id() }),
            Touched {
                chat_id: "c-1".into(),
                removed: vec![id()],
                ..Touched::default()
            }
        );
        assert_eq!(
            touched(ChatEvent::ChatDeleted),
            Touched {
                chat_id: "c-1".into(),
                deleted: true,
                ..Touched::default()
            }
        );
    }
}
