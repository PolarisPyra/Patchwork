//! The tools a chat's agent drives other chats with.
//!
//! It can list the project's chats, start one on any agent, message one, and wait for its reply.
//! Offered on Patchwork's own server to every chat a person started, unless Settings → Chat
//! turns them off.

// Rust guideline compliant 2026-10-09

use serde_json::{Value, json};

use super::Thread;
use crate::Server;

/// Lists the chats in the caller's project.
pub const LIST: &str = "list_chats";

/// Starts a chat and sends it its first message.
pub const START: &str = "start_chat";

/// Sends a message to a chat that is not replying.
pub const SEND: &str = "send_to_chat";

/// Waits for a chat's reply to its last message.
pub const WAIT: &str = "wait_for_chat";

/// Whether `tool` is one of these.
#[must_use]
pub fn is_tool(tool: &str) -> bool {
    [LIST, START, SEND, WAIT].contains(&tool)
}

/// Whether the agent of `thread` is given the tools: a chat a person started, with them on.
pub(crate) fn offered(server: &Server, thread: &Thread) -> bool {
    thread.parent.is_none() && !server.store.settings().chat_tools_off
}

impl Server {
    /// The tools as Patchwork's own server lists them for the thread's agent. Empty when the
    /// thread is not offered them.
    pub(crate) fn chat_tools(&self, thread_id: &str) -> Vec<Value> {
        let Some(thread) = self.store.thread(thread_id) else {
            return Vec::new();
        };
        if !offered(self, &thread) {
            return Vec::new();
        }
        let id = json!({"type": "string", "description": "The chat's id, from list_chats or start_chat."});
        vec![
            json!({
                "name": LIST,
                "description": "List the other chats in this project, with each one's id, title, agent, model and whether it is replying.",
                "inputSchema": {"type": "object", "properties": {}},
            }),
            json!({
                "name": START,
                "description": "Start a new chat in this project, on any agent the user has set up, and send it its first message. It runs on its own, side by side with this one, and the user sees it in their chat list. It sees nothing of this conversation, so the prompt must hold everything it needs. Returns the chat's id; call wait_for_chat with it for the reply.",
                "inputSchema": {"type": "object", "properties": {
                    "prompt": {"type": "string", "description": "The whole task, with everything the chat needs to know."},
                    "agent": {"type": "string", "enum": ["claude", "codex", "antigravity", "opencode", "api"], "description": "The agent to run the chat on. Defaults to this chat's agent."},
                    "model": {"type": "string", "description": "The model id. Defaults to the agent's default model."},
                    "effort": {"type": "string", "description": "The reasoning effort, such as low, medium or high. Defaults to the agent's."}
                }, "required": ["prompt"]},
            }),
            json!({
                "name": SEND,
                "description": "Send a message to another chat in this project that is not replying. Call wait_for_chat for its reply.",
                "inputSchema": {"type": "object", "properties": {
                    "chat_id": id.clone(),
                    "prompt": {"type": "string", "description": "The message."}
                }, "required": ["chat_id", "prompt"]},
            }),
            json!({
                "name": WAIT,
                "description": "Wait until another chat in this project finishes replying to its last message, for up to an hour, and return its reply with the files it changed and how its checks and review went.",
                "inputSchema": {"type": "object", "properties": {
                    "chat_id": id
                }, "required": ["chat_id"]},
            }),
        ]
    }
}
