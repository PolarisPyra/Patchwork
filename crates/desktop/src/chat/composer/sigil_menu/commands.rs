//! What the menu can list: the slash commands, the files and skills of a project, and how well each matches what is typed.

// Rust guideline compliant 2026-10-09

use std::collections::HashSet;
use std::ops::Range;
use patchwork_server::provider::{AgentKind, ProviderSlashCommand};
use crate::ui::sigils::{SigilKind, Sigils, default_sigil};

/// Most files the menu offers at once.
pub(super) const FILE_LIMIT: usize = 8;

/// When a command runs: as soon as it is picked, when the message is sent, or by the agent.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Runs {
    Pick,
    Send,
    Agent,
}

/// `SLASH_COMMANDS`: the commands Patchwork offers for every agent.
pub(super) const SLASH_COMMANDS: [(&str, &str, Runs); 7] = [
    (
        "/model",
        "Switch the response model for this chat",
        Runs::Pick,
    ),
    ("/plan", "Switch this chat into plan mode", Runs::Pick),
    (
        "/default",
        "Switch this chat back to normal mode",
        Runs::Pick,
    ),
    (
        "/goal",
        "Keep working until a condition is met",
        Runs::Agent,
    ),
    ("/new", "Start a fresh chat in this project", Runs::Send),
    (
        "/clear",
        "Delete this chat's messages and start the agent fresh",
        Runs::Send,
    ),
    (
        "/compact",
        "Summarise the conversation to free context",
        Runs::Send,
    ),
];

/// The command Patchwork runs itself when `text` is the whole message, if it is one.
pub fn local_command(text: &str) -> Option<&'static str> {
    SLASH_COMMANDS
        .iter()
        .find(|&&(name, _, runs)| runs != Runs::Agent && name == text)
        .map(|&(name, _, _)| name)
}

/// How well a command name matches the typed query (without the sigil): lower is better, and
/// `None` drops the command. Prefixes rank above matches that start a word, which rank above any
/// match.
pub(super) fn command_score(name: &str, query: &str) -> Option<u8> {
    let bare = name.get(1..).unwrap_or_default();
    if bare == query {
        Some(0)
    } else if bare.starts_with(query) {
        Some(2)
    } else if bare
        .match_indices(query)
        .any(|(i, _)| i > 0 && bare[..i].ends_with(['-', '_', '/']))
    {
        Some(4)
    } else {
        bare.contains(query).then_some(6)
    }
}

/// The "@word", "$word", "&word" or "/word" the caret is at the end of, at the start of a line
/// or after a space.
#[derive(Clone)]
pub(super) struct Token {
    pub(super) kind: SigilKind,
    /// What was typed, with the default sigil in place of the user's.
    pub(super) text: String,
    pub(super) range: Range<usize>,
    /// The token is the whole draft, so a pick replaces the draft and can run a local command.
    pub(super) whole: bool,
}

impl Token {
    pub(super) fn query(&self) -> String {
        self.text[1..].to_lowercase()
    }
}

pub(super) fn token(content: &str, caret: usize, sigils: &Sigils) -> Option<Token> {
    let before = content.get(..caret)?;
    let start = before
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map_or(0, |(i, c)| i + c.len_utf8());
    let raw = &before[start..];
    let first = raw.chars().next()?;
    let kind = sigils.kind_of(first)?;
    Some(Token {
        kind,
        text: format!("{}{}", default_sigil(kind), &raw[first.len_utf8()..]),
        range: start..caret,
        whole: content == raw,
    })
}

/// One entry: a path, or a command, skill or agent with the default sigil.
pub(super) struct Item {
    pub(super) name: String,
    pub(super) description: String,
}

/// Folders, written with a trailing "/", are listed with the files. Paths starting with what was
/// typed come first, then the shallower ones, so a folder drilled into lists what is directly in
/// it.
pub(super) fn file_items(files: &[String], query: &str) -> Vec<Item> {
    let mut seen = HashSet::new();
    let folders: Vec<&str> = files
        .iter()
        .flat_map(|path| path.match_indices('/').map(move |(i, _)| &path[..=i]))
        .filter(|folder| seen.insert(*folder))
        .collect();
    let mut paths: Vec<&str> = folders
        .into_iter()
        .chain(files.iter().map(String::as_str))
        .filter(|path| path.to_lowercase().contains(query))
        .collect();
    paths.sort_by_key(|path| {
        (
            !path.to_lowercase().starts_with(query),
            path.trim_end_matches('/').split('/').count(),
        )
    });
    paths
        .into_iter()
        .take(FILE_LIMIT)
        .map(|path| Item {
            name: path.to_string(),
            description: String::new(),
        })
        .collect()
}

/// Patchwork's commands, then the agent's own, leaving out the ones Patchwork has and Claude's
/// copies of Patchwork's skills, which are under "$".
pub(super) fn commands(agent: AgentKind, provider: &[ProviderSlashCommand]) -> Vec<Item> {
    let builtin: Vec<&str> = SLASH_COMMANDS
        .iter()
        .map(|(name, _, _)| &name[1..])
        .collect();
    let fallback = if agent == AgentKind::OpenCode {
        "OpenCode command"
    } else {
        "Claude Code command"
    };
    SLASH_COMMANDS
        .iter()
        // Patchwork does not run /goal in OpenCode.
        .filter(|(name, _, _)| agent != AgentKind::OpenCode || *name != "/goal")
        .map(|&(name, description, _)| Item {
            name: name.into(),
            description: description.into(),
        })
        .chain(provider.iter().filter_map(|c| {
            let name = c.name.strip_prefix('/').unwrap_or(&c.name);
            (!builtin.contains(&name) && !name.starts_with("claude:")).then(|| Item {
                name: format!("/{name}"),
                description: c.description.clone().unwrap_or_else(|| fallback.into()),
            })
        }))
        .collect()
}

/// The entries whose names match `query`, best first.
pub(super) fn scored(items: Vec<Item>, query: &str) -> Vec<Item> {
    let mut scored: Vec<(u8, Item)> = items
        .into_iter()
        .filter_map(|item| Some((command_score(&item.name.to_lowercase(), query)?, item)))
        .collect();
    scored.sort_by_key(|(score, _)| *score);
    scored.into_iter().map(|(_, item)| item).collect()
}
