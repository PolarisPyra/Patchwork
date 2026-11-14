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

/// The tool Patchwork's own agent collects the answer of a sub agent it started in the background
/// with.
pub const RESULT: &str = "sub_agent_result";

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
             researching a question. Your calls run one at a time, so to carry on with your own \
             work while it runs, set run_in_background: the call then returns its id at once, and \
             {RESULT} gives its answer when you need it."
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
    /// The call returns once the sub agent has started, and [`RESULT`] gives its answer.
    pub background: bool,
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
            background: args["run_in_background"] == true,
        })
    }
}

impl Server {
    /// The tools as Patchwork's own server lists them for the thread's agent: [`TOOL`], and for
    /// Patchwork's own agent [`RESULT`] with it. Empty when the thread's CLI runs its sub agents
    /// itself.
    pub(crate) fn delegate_tools(&self, thread_id: &str) -> Vec<Value> {
        let Some(thread) = self.store.thread(thread_id) else {
            return Vec::new();
        };
        let Some(pick) = sub_agent_of(&self.store, &self.store.settings(), &thread) else {
            return Vec::new();
        };
        let mut run = json!({
            "name": TOOL,
            "description": description(thread.agent, &pick),
            "inputSchema": {"type": "object", "properties": {
                "description": {"type": "string", "description": "What the sub agent does, in a few words."},
                "prompt": {"type": "string", "description": "The whole task, with everything the sub agent needs to know."}
            }, "required": ["description", "prompt"]},
        });
        // A CLI runs the calls of one message side by side, so only Patchwork's own agent, whose
        // calls run one at a time, needs a sub agent in the background.
        if thread.agent != AgentKind::Api {
            return vec![run];
        }
        run["inputSchema"]["properties"]["run_in_background"] = json!({
            "type": "boolean",
            "description": "Return at once with the sub agent's id, and carry on while it works.",
        });
        let result = json!({
            "name": RESULT,
            "description": "Wait for a sub agent started with run_in_background to finish, and return its answer. Call it once you need the answer, and before you finish your reply.",
            "inputSchema": {"type": "object", "properties": {
                "sub_agent_id": {"type": "string", "description": "The id run_sub_agent returned."}
            }, "required": ["sub_agent_id"]},
        });
        vec![run, result]
    }
}
