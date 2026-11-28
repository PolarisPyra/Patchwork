//! The methods a phone or the web client calls, each decoded into its `patchwork_protocol`
//! params, answered by the core's chat operations, and encoded in the shapes the clients have.
//! Only what the clients need to follow and drive a chat is here; every other method answers
//! that this version does not have it.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use patchwork_protocol::app::{AgentInfo, ProjectInfo, ServerInfo, Settings};
use patchwork_protocol::chats::{self as client, ChatParams, NewChatParams};
use patchwork_protocol::methods::{
    AnswerApproval, CreateChat, GetServerInfo, GetSettings, ListAgents, ListChats, ListMessages,
    ListProjects, ListProviders, SendTurn, StopTurn,
};
use patchwork_protocol::turns::{AnswerParams, SendParams, Sent};
use patchwork_protocol::{Method, decode, encode};
use serde_json::Value;

use super::wire::{agent_id, client_chat, client_message};
use super::{Handler, PairingStore, Touched, events};
use patchwork_core::chats::{AccessMode, NewChat};
use patchwork_core::provider::AgentKind;
use patchwork_core::Core;
use patchwork_platform::Error;

impl Handler for Core {
    fn pairing_store(&self) -> &dyn PairingStore {
        self.store()
    }

    fn call(self: Arc<Self>, method: &str, params: &Value) -> Result<Value, Error> {
        let core = &self;
        match method {
            GetServerInfo::NAME => answer::<GetServerInfo>(params, |_| {
                Ok(ServerInfo {
                    version: patchwork_platform::VERSION.to_string(),
                    platform: platform().to_string(),
                    home_dir: patchwork_platform::files::paths::home_dir(),
                    data_dir: patchwork_platform::dirs().data.clone(),
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
                    default_agent: agent_id(core.store().settings().default_agent()),
                    default_access_mode: AccessMode::default(),
                    show_thoughts: true,
                    terminal_shell: None,
                    terminal_font_size: 12,
                })
            }),
            ListAgents::NAME => answer::<ListAgents>(params, |_| {
                Ok(core
                    .store()
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
        events::client_pushes(self, touched)
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

/// The platform as Node names it, which the web client expects.
fn platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

fn projects(core: &Core) -> Vec<ProjectInfo> {
    core.store()
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
    let settings = core.store().settings();
    let agent = match new.agent {
        Some(id) => serde_json::from_value::<AgentKind>(Value::String(id.clone()))
            .map_err(|_unknown| format!("Patchwork has no agent {id}"))?,
        None => settings.default_agent(),
    };
    let access_mode = match new.access_mode.as_deref() {
        None => AccessMode::default(),
        Some(name) => client::AccessMode::named(name)
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
