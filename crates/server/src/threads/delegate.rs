//! The tool a chat's agent starts a sub agent on another agent with.
//!
//! A CLI only runs its own models, so a chat whose sub agents are set to another agent (see
//! `granular`) is offered [`TOOL`] on Patchwork's own server. Patchwork runs the work as a chat of
//! that agent's, and hands its answer back as the tool's result.
//!
//! Patchwork's own agent has no sub agents of its own, so it is offered the tool in every chat, and
//! its sub agents run on itself unless the chat names another agent.

// Rust guideline compliant 2026-10-09

use serde_json::{Value, json};

use super::granular::{AgentPick, sub_agent_of};
use crate::provider::AgentKind;
use crate::{Error, Server};

/// The tool a chat's agent starts a sub agent on another agent with.
pub const TOOL: &str = "run_sub_agent";

/// What Codex shows a call to [`TOOL`] as: it names every MCP call "MCP", and this is its summary.
const CODEX_SUMMARY: &str = "patchwork.run_sub_agent";

/// Whether a call an agent reports, by its name and summary, is one to [`TOOL`]. Each CLI names
/// an MCP tool its own way, and all but Codex end the name with the tool's.
#[must_use]
pub fn is_call(name: &str, summary: &str) -> bool {
    name.ends_with(TOOL) || summary == CODEX_SUMMARY
}

/// What the call's card is titled from its summary, which Codex leaves as the tool's own name.
#[must_use]
pub fn shown(summary: &str) -> &str {
    if summary == CODEX_SUMMARY {
        ""
    } else {
        summary
    }
}

/// What the agent of `caller` is told the tool does, for sub agents that run at `pick`.
fn description(caller: AgentKind, pick: &AgentPick) -> String {
    let model = pick
        .model
        .as_deref()
        .map(|model| format!(" ({model})"))
        .unwrap_or_default();
    // Patchwork's own agent runs its calls one at a time and has no sub agents of its own.
    if caller == AgentKind::Api {
        let on = if pick.agent == caller {
            format!("a fresh run of this agent{model}, with the same tools")
        } else {
            format!("{:?}{model}, with tools of its own", pick.agent)
        };
        return format!(
            "Hand a self-contained piece of work to a sub agent and wait for its answer: {on}, \
             working in the same folder. It sees nothing of this conversation, so the prompt must \
             hold everything it needs, and only its last reply comes back. Use it for work whose \
             steps would crowd this conversation, such as searching a large codebase or \
             researching a question."
        );
    }
    format!(
        "Run a sub agent on {:?}{model} and wait for its answer. The user set this chat's sub \
         agents to run there: whenever you would hand work to a sub agent, call this instead of \
         your own sub agent or task tool. The sub agent works in the same folder with tools of its \
         own and sees nothing of this conversation, so the prompt must hold everything it needs. \
         Several calls in one message run side by side.",
        pick.agent
    )
}

/// What the agent asked a sub agent to do, read from the tool's arguments.
#[derive(Debug)]
pub struct Ask {
    /// A few words for the sub agent's card.
    pub description: String,
    pub prompt: String,
}

impl Ask {
    /// Reads the request from the arguments of a tool call.
    ///
    /// # Errors
    ///
    /// Returns an error when the arguments give no prompt.
    pub fn parse(args: &Value) -> Result<Self, Error> {
        let text = |key: &str| args[key].as_str().map(str::trim).unwrap_or_default();
        let prompt = text("prompt");
        if prompt.is_empty() {
            return Err("Give the sub agent a prompt".into());
        }
        Ok(Self {
            description: text("description").to_string(),
            prompt: prompt.to_string(),
        })
    }
}

impl Server {
    /// The tool as Patchwork's own server lists it for the thread's agent. None when the
    /// thread's CLI runs its sub agents itself.
    pub(crate) fn delegate_tool(&self, thread_id: &str) -> Option<Value> {
        let thread = self.store.thread(thread_id)?;
        let pick = sub_agent_of(&self.store, &self.store.settings(), &thread)?;
        Some(json!({
            "name": TOOL,
            "description": description(thread.agent, &pick),
            "inputSchema": {"type": "object", "properties": {
                "description": {"type": "string", "description": "What the sub agent does, in a few words."},
                "prompt": {"type": "string", "description": "The whole task, with everything the sub agent needs to know."}
            }, "required": ["description", "prompt"]},
        }))
    }
}
