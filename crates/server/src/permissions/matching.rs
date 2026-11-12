//! Whether a rule's patterns cover a request: paths inside folders, hosts, and command prefixes, compared unit by unit.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};
use crate::system::fs::{real, resolve};
use crate::system::fs::home_dir;
use super::{Effect, PermissionKind, PermissionRule};
use super::command::{any_program, does_more, handed_code, may_run, split_writes};
use super::{Target, opencode_name};

/// One part of a request a rule has to cover.
#[derive(Debug, PartialEq)]
pub(super) enum Unit {
    Path(PathBuf),
    /// A simple command's words.
    Command(Vec<String>),
    /// Code a command hands an interpreter to run, as `python -c` does.
    Code(String),
    Host(String),
    /// An MCP `server/tool`.
    Tool(String),
    /// A request with nothing to match, which only `*` covers.
    Unknown,
}

/// The parts of `target`, whether allow rules may answer it at all, and the files its commands
/// write by redirection, which Edit rules answer for.
pub(super) fn units(target: &Target, root: &Path) -> (Vec<Unit>, bool, Vec<String>) {
    let mut allowable = true;
    let mut writes = Vec::new();
    let mut units: Vec<Unit> = match target.kind {
        PermissionKind::Edit => target
            .resources
            .iter()
            // Where the path really is: a link in the project can lead out of it.
            .map(|r| Unit::Path(real(&resolve(root, r))))
            .collect(),
        PermissionKind::Folder => target
            .resources
            .iter()
            .map(|r| Unit::Path(real(&folder_of(root, r))))
            .collect(),
        PermissionKind::Command => target
            .resources
            .iter()
            .flat_map(|line| {
                let (segments, safe, written) = split_writes(line);
                allowable &= safe;
                writes.extend(written);
                let code: Vec<Unit> = segments
                    .iter()
                    .filter_map(|words| handed_code(words, line))
                    .map(Unit::Code)
                    .collect();
                segments.into_iter().map(Unit::Command).chain(code)
            })
            .collect(),
        PermissionKind::Web => target
            .resources
            .iter()
            .map(|r| Unit::Host(host(r)))
            .collect(),
        PermissionKind::Mcp => target
            .resources
            .iter()
            .map(|r| Unit::Tool(r.clone()))
            .collect(),
        // The screen is one resource, named `screen`, and a rule either covers it or not.
        PermissionKind::Screen => target
            .resources
            .iter()
            .map(|r| Unit::Tool(r.clone()))
            .collect(),
    };
    if units.is_empty() {
        units.push(Unit::Unknown);
    }
    (units, allowable, writes)
}

pub(super) fn covers(rule: &PermissionRule, unit: &Unit, root: &Path) -> bool {
    let pattern = rule.pattern.trim();
    if pattern == "*" {
        return true;
    }
    match unit {
        Unit::Path(path) => match pattern {
            "." => path.starts_with(real(root)),
            "~" => home_dir().is_some_and(|home| path.starts_with(real(&home))),
            _ => path.starts_with(real(&resolve(root, pattern.trim_end_matches(['*', '/'])))),
        },
        Unit::Command(words) => {
            let wanted: Vec<&str> = pattern.split_whitespace().collect();
            if rule.effect != Effect::Allow && words.first().is_some_and(|w| any_program(w)) {
                return true;
            }
            !wanted.is_empty()
                && words.len() >= wanted.len()
                && wanted
                    .iter()
                    .zip(words)
                    .enumerate()
                    .all(|(i, (want, word))| {
                        // The program may be named by its path, which a rule that refuses or
                        // asks still covers. One that allows does not: `./ls` is not `ls`.
                        let word = if i == 0 && rule.effect != Effect::Allow {
                            word.rsplit('/').next().unwrap_or(word)
                        } else {
                            word
                        };
                        *want == word
                    })
                // Past the words the rule names, the command must still only do what it says.
                && (rule.effect != Effect::Allow
                    || !does_more(&words[0], &words[wanted.len()..]))
        }
        // A rule that allows the interpreter allows what it is handed. One that refuses or asks
        // about a command answers for code that may run that command.
        Unit::Code(code) => {
            rule.effect == Effect::Allow
                || pattern
                    .split_whitespace()
                    .next()
                    .is_some_and(|program| may_run(code, program))
        }
        Unit::Host(host) => {
            let pattern = pattern.trim_start_matches("*.").to_ascii_lowercase();
            !host.is_empty() && (*host == pattern || host.ends_with(&format!(".{pattern}")))
        }
        Unit::Tool(tool) => {
            let opencode_id = match pattern.split_once('/') {
                Some((server, name)) => {
                    format!("{}_{}", opencode_name(server), opencode_name(name))
                }
                None => opencode_name(pattern),
            };
            tool == pattern
                || tool
                    .strip_prefix(pattern)
                    .is_some_and(|rest| rest.starts_with('/'))
                // OpenCode names a tool `<server>_<tool>`.
                || !tool.contains('/')
                    && (*tool == opencode_id
                        || (!pattern.contains('/')
                            && tool
                                .strip_prefix(&opencode_id)
                                .is_some_and(|rest| rest.starts_with('_'))))
        }
        Unit::Unknown => false,
    }
}

/// The folder a folder request names: OpenCode asks for `dir/*`.
pub(super) fn folder_of(root: &Path, resource: &str) -> PathBuf {
    let trimmed = resource.trim().trim_end_matches('*').trim_end_matches('/');
    match trimmed {
        "" if resource.trim().starts_with('/') => PathBuf::from("/"),
        "~" => home_dir().unwrap_or_default(),
        _ => resolve(root, trimmed),
    }
}

/// `path` with the home folder as `~`.
pub(super) fn shown(path: &Path) -> String {
    match home_dir().and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf)) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// The lowercase host of a URL or a bare host name.
pub(super) fn host(resource: &str) -> String {
    let rest = resource
        .trim()
        .split_once("://")
        .map_or(resource.trim(), |(_, rest)| rest);
    // Clients disagree on where the host ends in a URL with a backslash or a blank in it, so
    // such a URL names no host a rule can answer for.
    if rest.contains(|c: char| c == '\\' || c.is_whitespace() || c.is_control()) {
        return String::new();
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = host.split(':').next().unwrap_or_default();
    host.to_ascii_lowercase()
}
