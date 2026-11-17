//! The tools a chat's agent drives other chats with.
//!
//! It can list the project's chats (every project's, in a chat run from a chat app), read how far
//! one has got, start one on any agent, message one, and wait for its reply.
//! Offered on Patchwork's own server to every chat a person started, unless Settings → Chat
//! turns them off.

// Rust guideline compliant 2026-10-09

use serde_json::{Value, json};

use super::Thread;
use crate::Core;

/// Lists the chats in the caller's project.
pub const LIST: &str = "list_chats";

/// Starts a chat and sends it its first message.
pub const START: &str = "start_chat";

/// Sends a message to a chat that is not replying.
pub const SEND: &str = "send_to_chat";

/// Waits for a chat's reply to its last message.
pub const WAIT: &str = "wait_for_chat";

/// Reads how far a chat has got, found by its id or by what a person calls it.
pub const READ: &str = "read_chat";

/// Whether `tool` is one of these.
#[must_use]
pub fn is_tool(tool: &str) -> bool {
    [LIST, READ, START, SEND, WAIT].contains(&tool)
}

/// Whether `list_chats` shows `thread` the chats of every project: a chat run from a chat app,
/// where the person is away from Patchwork and asks about all of it.
pub(crate) fn sees_every_project(core: &Core, thread: &Thread) -> bool {
    !core.bound_chats(&thread.id).is_empty()
}

/// Whether the agent of `thread` is given the tools: a chat a person started, with them on.
pub(crate) fn offered(core: &Core, thread: &Thread) -> bool {
    thread.parent.is_none() && !core.store.settings().chat_tools_off
}

impl Core {
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
        let list = if sees_every_project(self, &thread) {
            "List the other chats in every Patchwork project, with each one's id, title, project, agent, model, whether it is replying and, when it is, what it is doing now. Use it to answer questions about what is running; read_chat tells how far one has got. Only the chats in this chat's project can be messaged or waited on."
        } else {
            "List the other chats in this project, with each one's id, title, agent, model, whether it is replying and, when it is, what it is doing now. read_chat tells how far one has got."
        };
        vec![
            json!({
                "name": LIST,
                "description": list,
                "inputSchema": {"type": "object", "properties": {}},
            }),
            json!({
                "name": READ,
                "description": "Read how far another chat has got: the task it was given, whether it is replying, waiting for an answer, done or failed, what it is doing now and for how long, its goal, its plan with the steps done, its recent steps, the files it changed and its latest messages. Use it to answer questions like \"what's going on in X\" or \"progress on Y\": pass name with whatever the person called the chat, in their words, whether that is its title, its project or both, or chat_id from list_chats. When several chats match, it returns them so you can pick one or ask. Answer the way a teammate gives a quick standup update, in a few short lines: what it is on, what is done, what is left, and anything stuck or waiting on the person.",
                "inputSchema": {"type": "object", "properties": {
                    "name": {"type": "string", "description": "What the person called the chat, such as \"margrete reverse engineering\"."},
                    "chat_id": {"type": "string", "description": "The chat's id, from list_chats. Use it instead of name when you have it."}
                }},
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
