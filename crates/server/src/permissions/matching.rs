//! Whether a rule's patterns cover a request: paths inside folders, hosts, and command prefixes, compared unit by unit.

use std::path::{Path, PathBuf};
use crate::system::fs::resolve;
use crate::system::fs::home_dir;
use super::{PermissionKind, PermissionRule};
use super::command::split_command;
use super::{Target, opencode_name};

/// One part of a request a rule has to cover.
#[derive(Debug, PartialEq)]
pub(super) enum Unit {
    Path(PathBuf),
    /// A simple command's words.
    Command(Vec<String>),
    Host(String),
    /// An MCP `server/tool`.
    Tool(String),
    /// A request with nothing to match, which only `*` covers.
    Unknown,
}

/// The parts of `target`, and whether allow rules may answer it at all.
pub(super) fn units(target: &Target, root: &Path) -> (Vec<Unit>, bool) {
    let mut allowable = true;
    let mut units: Vec<Unit> = match target.kind {
        PermissionKind::Edit => target
            .resources
            .iter()
            .map(|r| Unit::Path(resolve(root, r)))
            .collect(),
        PermissionKind::Folder => target
            .resources
            .iter()
            .map(|r| Unit::Path(folder_of(root, r)))
            .collect(),
        PermissionKind::Command => target
            .resources
            .iter()
            .flat_map(|line| {
                let (segments, safe) = split_command(line);
                allowable &= safe;
                segments.into_iter().map(Unit::Command)
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
    (units, allowable)
}

pub(super) fn covers(rule: &PermissionRule, unit: &Unit, root: &Path) -> bool {
    let pattern = rule.pattern.trim();
    if pattern == "*" {
        return true;
    }
    match unit {
        Unit::Path(path) => match pattern {
            "." => path.starts_with(root),
            "~" => home_dir().is_some_and(|home| path.starts_with(home)),
            _ => path.starts_with(resolve(root, pattern.trim_end_matches(['*', '/']))),
        },
        Unit::Command(words) => {
            let wanted: Vec<&str> = pattern.split_whitespace().collect();
            !wanted.is_empty()
                && words.len() >= wanted.len()
                && wanted
                    .iter()
                    .zip(words)
                    .enumerate()
                    .all(|(i, (want, word))| {
                        // The program may be named by its path.
                        let word = if i == 0 {
                            word.rsplit('/').next().unwrap_or(word)
                        } else {
                            word
                        };
                        *want == word
                    })
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
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = host.split(':').next().unwrap_or_default();
    host.to_ascii_lowercase()
}
