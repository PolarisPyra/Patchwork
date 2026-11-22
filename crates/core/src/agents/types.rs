//! An agent, the files it knows, and what its runs used.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

use crate::backgrounds::ChatBackground;
use crate::gateway::Platform;
use crate::permissions::{Effect, PermissionKind, PermissionRule};
use crate::provider::AgentKind;
use crate::chats::{AccessMode, TokenUsage};

/// A named agent with standing instructions, and the projects it may work in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    /// Made by Patchwork, never from the name: it names the agent's folder.
    pub id: String,
    pub name: String,
    /// What the agent is told at the start of each run, before the prompt.
    pub instructions: String,
    pub agent: AgentKind,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub access_mode: AccessMode,
    /// The ids of the projects its runs may work in. A run outside them works in a scratch
    /// folder of its own.
    pub projects: Vec<String>,
    pub created_at: i64,
    /// When it was archived; None while it is in the sidebar.
    #[serde(default)]
    pub archived_at: Option<i64>,
    /// What shows behind its page and its runs; None shows the one every chat shares.
    #[serde(default)]
    pub background: Option<ChatBackground>,
    /// The tools its runs may not use, refused on every provider.
    #[serde(default)]
    pub blocked: BlockedTools,
    /// The chat app whose messages it answers; None for an agent started only in Patchwork.
    #[serde(default)]
    pub chat_app: Option<Platform>,
    /// Its chat, which its page and its chat app's conversations carry on; None until the
    /// first message. Its routines and `/new` from a chat app start runs of their own.
    #[serde(default)]
    #[serde(alias = "threadId")]
    pub chat_id: Option<String>,
}

/// Why a call the agent blocks is refused, as the chat and the agent are told.
pub const BLOCKED_REASON: &str = "This agent is not allowed to use this tool";

/// The tools an agent's runs may not use.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockedTools {
    /// Whole kinds of call: commands, file edits, or web pages.
    #[serde(default)]
    pub kinds: Vec<PermissionKind>,
    /// MCP servers, by name, none of whose tools it may call.
    #[serde(default)]
    pub servers: Vec<String>,
}

impl BlockedTools {
    /// Whether it blocks nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty() && self.servers.is_empty()
    }

    /// The blocks as deny rules: a kind refuses everything of it, a server every tool it has.
    #[must_use]
    pub fn rules(&self) -> Vec<PermissionRule> {
        let rule = |kind, pattern: &str| PermissionRule {
            id: String::new(),
            kind,
            pattern: pattern.to_string(),
            effect: Effect::Deny,
        };
        self.kinds
            .iter()
            .map(|&kind| rule(kind, "*"))
            .chain(self.servers.iter().map(|s| rule(PermissionKind::Mcp, s)))
            .collect()
    }
}

/// A file in an agent's knowledge folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeFile {
    pub name: String,
    pub size: u64,
}

/// One run of an agent, and the tokens its replies used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRun {
    pub chat_id: String,
    pub title: String,
    pub started_at: u64,
    pub usage: TokenUsage,
}

/// An agent's runs, newest first, and the tokens they used in all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentUsage {
    pub runs: Vec<AgentRun>,
    pub total: TokenUsage,
}

/// A role a new agent can start in, which fills in its instructions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartingPoint {
    pub label: &'static str,
    /// Empty for the choice that leaves the agent's instructions to the person.
    pub instructions: &'static str,
}

/// The roles a new agent is offered, in the order they are listed.
pub const STARTING_POINTS: [StartingPoint; 6] = [
    StartingPoint {
        label: "Research and digging into topics",
        instructions: "You research topics for me. Search widely, read the sources, and come back with a short summary that cites them and says what you are unsure of.",
    },
    StartingPoint {
        label: "Keeping an eye on things for me",
        instructions: "You keep an eye on things I point you at: builds, logs, folders and pages. Check them, and tell me only what changed or needs me.",
    },
    StartingPoint {
        label: "Reviewing code changes",
        instructions: "You review code changes. Read the diff and the code around it, then report real bugs first, each with its file and line, and smaller issues after.",
    },
    StartingPoint {
        label: "Fixing bugs and chores",
        instructions: "You fix bugs and do chores in my projects. Reproduce the problem, make the smallest change that fixes it, run the project's checks, and say what you changed.",
    },
    StartingPoint {
        label: "Writing docs and reports",
        instructions: "You write docs and reports. Read what is there first, match its style, and keep it short and plain.",
    },
    StartingPoint {
        label: "Something else",
        instructions: "",
    },
];
