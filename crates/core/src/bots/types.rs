//! A bot, the files it knows, and what its runs used.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

use crate::backgrounds::ChatBackground;
use crate::provider::AgentKind;
use crate::threads::{AccessMode, TokenUsage};

/// A named agent with standing instructions, and the projects it may work in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bot {
    /// Made by Patchwork, never from the name: it names the bot's folder.
    pub id: String,
    pub name: String,
    /// What the bot is told at the start of each run, before the prompt.
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
}

/// A file in a bot's knowledge folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeFile {
    pub name: String,
    pub size: u64,
}

/// One run of a bot, and the tokens its replies used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotRun {
    pub thread_id: String,
    pub title: String,
    pub started_at: u64,
    pub usage: TokenUsage,
}

/// A bot's runs, newest first, and the tokens they used in all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BotUsage {
    pub runs: Vec<BotRun>,
    pub total: TokenUsage,
}

/// A role a new bot can start in, which fills in its instructions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartingPoint {
    pub label: &'static str,
    /// Empty for the choice that leaves the bot's instructions to the person.
    pub instructions: &'static str,
}

/// The roles a new bot is offered, in the order they are listed.
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
