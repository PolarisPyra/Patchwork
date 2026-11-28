//! The methods the app answers, each tying its name to its params and answer.
//!
//! Pairing, devices and the event log are answered by the server itself; see [`crate::devices`].

// Rust guideline compliant 2026-10-09

use crate::{Method, NoParams};
use crate::app::{AgentInfo, ProjectInfo, ServerInfo, Settings};
use crate::chats::{Chat, ChatParams, NewChatParams};
use crate::messages::Message;
use crate::turns::{AnswerParams, SendParams, Sent};

/// Declares a method marker and its [`Method`] impl.
macro_rules! method {
    ($(#[$doc:meta])* $marker:ident, $name:literal, $params:ty, $output:ty) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy)]
        pub struct $marker;

        impl Method for $marker {
            const NAME: &'static str = $name;
            type Params = $params;
            type Output = $output;
        }
    };
}

method!(
    /// `server.info`: the app's version and folders.
    GetServerInfo, "server.info", NoParams, ServerInfo
);
method!(
    /// `projects.list`: the projects in their sidebar order.
    ListProjects, "projects.list", NoParams, Vec<ProjectInfo>
);
method!(
    /// `chats.list`: every chat a person started, without subagents' chats.
    ListChats, "chats.list", NoParams, Vec<Chat>
);
method!(
    /// `messages.list`: one chat's messages, oldest first.
    ListMessages, "messages.list", ChatParams, Vec<Message>
);
method!(
    /// `chats.create`: a new chat, on the defaults where params leave them out.
    CreateChat, "chats.create", NewChatParams, Chat
);
method!(
    /// `turn.send`: a message to a chat, which starts, steers or queues a turn.
    SendTurn, "turn.send", SendParams, Sent
);
method!(
    /// `turn.stop`: stops a chat's running turn.
    StopTurn, "turn.stop", ChatParams, ()
);
method!(
    /// `approval.answer`: allows or denies an approval, or answers a question.
    AnswerApproval, "approval.answer", AnswerParams, ()
);
method!(
    /// `settings.get`: the settings a client shows.
    GetSettings, "settings.get", NoParams, Settings
);
method!(
    /// `agents.list`: the agents the user turned on.
    ListAgents, "agents.list", NoParams, Vec<AgentInfo>
);
method!(
    /// `providers.list`: API providers; the app lists none to clients yet.
    ListProviders, "providers.list", NoParams, Vec<serde_json::Value>
);
