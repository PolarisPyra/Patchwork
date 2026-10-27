//! Settings → Permissions: the approval requests Patchwork answers itself, by the rules for the
//! thread's project (or for chats without one) and the rules for everywhere.

use std::path::{Path, PathBuf};

use crate::provider::api_tools::resolve;
use crate::provider::home_dir;
use crate::types::{
    Effect, PermissionKind, PermissionRule, PermissionScope, PermissionSettings, RuleRef,
    ScopeRules,
};

/// What an approval request asks to do, in terms a rule can match.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub kind: PermissionKind,
    /// Paths, command lines, URLs, or `server/tool` names.
    pub resources: Vec<String>,
}

impl Target {
    pub fn new<S: Into<String>>(
        kind: PermissionKind,
        resources: impl IntoIterator<Item = S>,
    ) -> Self {
        let resources = resources
            .into_iter()
            .map(Into::into)
            .filter(|r: &String| !r.trim().is_empty())
            .collect();
        Self { kind, resources }
    }
}

/// A rule's answer to a request.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub allow: bool,
    pub rule: RuleRef,
}

/// The rules' answer to `target` in a thread of `project` (None for a chat) working in `root`, or
/// None when the user is to be asked. Deny and ask rules win when they cover any part of the
/// request; allow rules have to cover all of it. Then the scope's default for the kind decides,
/// then the default for everywhere.
pub fn decide(
    settings: &PermissionSettings,
    project: Option<&str>,
    root: &Path,
    target: &Target,
) -> Option<Decision> {
    let empty = ScopeRules::default();
    let (local_scope, local) = match project {
        Some(id) => (
            PermissionScope::Project(id.into()),
            settings.projects.get(id).unwrap_or(&empty),
        ),
        None => (PermissionScope::Chats, &settings.chats),
    };
    let scopes = [
        (&local_scope, local),
        (&PermissionScope::Everywhere, &settings.everywhere),
    ];
    let (units, allowable) = units(target, root);
    let found = |scope: &PermissionScope, rule: &PermissionRule| RuleRef {
        scope: scope.clone(),
        rule_id: Some(rule.id.clone()),
    };

    for effect in [Effect::Deny, Effect::Ask] {
        for (scope, rules) in scopes {
            let hit = rules
                .rules
                .iter()
                .filter(|r| r.kind == target.kind && r.effect == effect)
                .find(|r| units.iter().any(|u| covers(r, u, root)));
            if let Some(rule) = hit {
                return (effect == Effect::Deny).then(|| Decision {
                    allow: false,
                    rule: found(scope, rule),
                });
            }
        }
    }

    if allowable {
        let allowing = |unit: &Unit| {
            scopes.iter().find_map(|(scope, rules)| {
                rules
                    .rules
                    .iter()
                    .filter(|r| r.kind == target.kind && r.effect == Effect::Allow)
                    .find(|r| covers(r, unit, root))
                    .map(|r| found(scope, r))
            })
        };
        let mut first = None;
        for unit in &units {
            match allowing(unit) {
                Some(rule) => {
                    first.get_or_insert(rule);
                }
                None => {
                    first = None;
                    break;
                }
            }
        }
        if let Some(rule) = first {
            return Some(Decision { allow: true, rule });
        }
    }

    for (scope, rules) in scopes {
        match rules.defaults.get(&target.kind) {
            Some(Effect::Ask) => return None,
            Some(effect) => {
                return Some(Decision {
                    allow: *effect == Effect::Allow,
                    rule: RuleRef {
                        scope: scope.clone(),
                        rule_id: None,
                    },
                });
            }
            None => {}
        }
    }
    None
}

/// The patterns "Always allow" saves for `target`: the thread's own folder or the folder outside
/// it, each command's leading words, the web host, or the MCP server. Empty when a rule could not
/// safely cover it, such as a command with a substitution or a redirect.
pub fn suggest(target: &Target, root: &Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |p: String| {
        if !p.is_empty() && !out.contains(&p) {
            out.push(p);
        }
    };
    match target.kind {
        PermissionKind::Edit | PermissionKind::Folder => {
            for resource in &target.resources {
                let path = folder_of(root, resource);
                if path.starts_with(root) {
                    add(".".into());
                } else if target.kind == PermissionKind::Folder {
                    add(shown(&path));
                } else {
                    add(shown(path.parent().unwrap_or(&path)));
                }
            }
        }
        PermissionKind::Command => {
            for line in &target.resources {
                let (segments, safe) = split_command(line);
                if !safe {
                    return Vec::new();
                }
                for words in segments {
                    add(command_prefix(&words));
                }
            }
        }
        PermissionKind::Web => target.resources.iter().for_each(|r| add(host(r))),
        PermissionKind::Mcp => target
            .resources
            .iter()
            .for_each(|r| add(r.split('/').next().unwrap_or_default().into())),
    }
    out
}

/// Adds the starting rules once, so deleting one does not bring it back.
pub fn seed(settings: &mut PermissionSettings) {
    if settings.seeded {
        return;
    }
    settings.seeded = true;
    settings.everywhere.rules.extend(default_rules());
}

/// The starting rules for everywhere: commands that only read run without asking, and commands
/// that delete, escalate, or rewrite history ask, whatever allow rule covers them. Settings →
/// Permissions puts them back with Restore defaults.
pub fn default_rules() -> Vec<PermissionRule> {
    let rule = |pattern: &str, effect: Effect| PermissionRule {
        id: format!("default:{pattern}"),
        kind: PermissionKind::Command,
        pattern: pattern.into(),
        effect,
    };
    let reads = [
        "cd",
        "ls",
        "pwd",
        "cat",
        "head",
        "tail",
        "wc",
        "grep",
        "rg",
        "echo",
        "which",
        "file",
        "stat",
        "tree",
        "du",
        "df",
        "git status",
        "git diff",
        "git log",
        "git show",
    ];
    let risky = ["sudo", "rm", "git push", "git reset", "git clean", "dd"];
    reads
        .iter()
        .map(|p| rule(p, Effect::Allow))
        .chain(risky.iter().map(|p| rule(p, Effect::Ask)))
        .collect()
}

/// One part of a request a rule has to cover.
#[derive(Debug)]
enum Unit {
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
fn units(target: &Target, root: &Path) -> (Vec<Unit>, bool) {
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
    };
    if units.is_empty() {
        units.push(Unit::Unknown);
    }
    (units, allowable)
}

fn covers(rule: &PermissionRule, unit: &Unit, root: &Path) -> bool {
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
            tool == pattern
                || tool
                    .strip_prefix(pattern)
                    .is_some_and(|rest| rest.starts_with('/'))
        }
        Unit::Unknown => false,
    }
}

/// The folder a folder request names: OpenCode asks for `dir/*`.
fn folder_of(root: &Path, resource: &str) -> PathBuf {
    let trimmed = resource.trim().trim_end_matches('*').trim_end_matches('/');
    match trimmed {
        "" if resource.trim().starts_with('/') => PathBuf::from("/"),
        "~" => home_dir().unwrap_or_default(),
        _ => resolve(root, trimmed),
    }
}

/// `path` with the home folder as `~`.
fn shown(path: &Path) -> String {
    match home_dir().and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf)) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// The lowercase host of a URL or a bare host name.
fn host(resource: &str) -> String {
    let rest = resource
        .trim()
        .split_once("://")
        .map_or(resource.trim(), |(_, rest)| rest);
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = host.split(':').next().unwrap_or_default();
    host.to_ascii_lowercase()
}

/// Programs whose first argument names what they do, so a rule names both.
const SUBCOMMANDS: [&str; 16] = [
    "git",
    "cargo",
    "npm",
    "pnpm",
    "yarn",
    "bun",
    "uv",
    "go",
    "docker",
    "kubectl",
    "gh",
    "pip",
    "rustup",
    "systemctl",
    "brew",
    "pacman",
];

/// The leading words a rule for `words` names: the program, and its subcommand for the programs
/// that have one.
fn command_prefix(words: &[String]) -> String {
    let Some(program) = words.first() else {
        return String::new();
    };
    let program = program.rsplit('/').next().unwrap_or(program);
    match words.get(1) {
        Some(sub)
            if SUBCOMMANDS.contains(&program)
                && sub
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == ':')
                && !sub.starts_with('-') =>
        {
            format!("{program} {sub}")
        }
        _ => program.into(),
    }
}

/// A shell line as its simple commands' words, split at `&&`, `||`, `;`, `|`, `&`, and new lines,
/// with leading `NAME=value` assignments dropped and `sh -c '…'` unwrapped. The flag is false
/// when the line does more than its commands show: a substitution, a subshell, or a redirect
/// other than to /dev/null or another descriptor.
fn split_command(line: &str) -> (Vec<Vec<String>>, bool) {
    let mut segments: Vec<Vec<String>> = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut safe = true;
    let mut chars = line.chars().peekable();
    let end_word = |word: &mut String, in_word: &mut bool, words: &mut Vec<String>| {
        if *in_word {
            words.push(std::mem::take(word));
            *in_word = false;
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                for c in chars.by_ref() {
                    if c == '\'' {
                        break;
                    }
                    word.push(c);
                }
            }
            '"' => {
                in_word = true;
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => word.extend(chars.next()),
                        '`' => {
                            safe = false;
                            word.push(c);
                        }
                        '$' if chars.peek() == Some(&'(') => {
                            safe = false;
                            word.push(c);
                        }
                        _ => word.push(c),
                    }
                }
            }
            '\\' => {
                in_word = true;
                word.extend(chars.next());
            }
            '`' | '(' | ')' => {
                safe = false;
                in_word = true;
                word.push(c);
            }
            '$' if chars.peek() == Some(&'(') => {
                safe = false;
                in_word = true;
                word.push(c);
            }
            '>' | '<' => {
                // `2>` names a descriptor, not a word.
                if in_word && word.chars().all(|c| c.is_ascii_digit()) {
                    word.clear();
                    in_word = false;
                }
                end_word(&mut word, &mut in_word, &mut words);
                if c == '<' {
                    safe = false;
                    continue;
                }
                if chars.peek() == Some(&'>') {
                    chars.next();
                }
                if chars.peek() == Some(&'&') {
                    chars.next();
                    if chars
                        .peek()
                        .is_some_and(|c| c.is_ascii_digit() || *c == '-')
                    {
                        chars.next();
                        continue;
                    }
                }
                while chars.peek().is_some_and(|c| *c == ' ' || *c == '\t') {
                    chars.next();
                }
                let mut file = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || matches!(c, ';' | '&' | '|') {
                        break;
                    }
                    file.push(c);
                    chars.next();
                }
                if file != "/dev/null" {
                    safe = false;
                }
            }
            ';' | '&' | '|' | '\n' => {
                end_word(&mut word, &mut in_word, &mut words);
                if matches!(c, '&' | '|') && chars.peek() == Some(&c) {
                    chars.next();
                }
                segments.push(std::mem::take(&mut words));
            }
            c if c.is_whitespace() => end_word(&mut word, &mut in_word, &mut words),
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    end_word(&mut word, &mut in_word, &mut words);
    segments.push(words);

    let mut out = Vec::new();
    for mut words in segments {
        let assignments = words.iter().take_while(|w| is_assignment(w)).count();
        words.drain(..assignments);
        if words.is_empty() {
            continue;
        }
        let shell = words[0].rsplit('/').next().unwrap_or_default();
        if words.len() == 3
            && matches!(shell, "sh" | "bash" | "zsh" | "dash")
            && matches!(words[1].as_str(), "-c" | "-lc" | "-ic")
        {
            let (inner, inner_safe) = split_command(&words[2]);
            safe &= inner_safe;
            out.extend(inner);
        } else {
            out.push(words);
        }
    }
    (out, safe)
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && !name.starts_with(|c: char| c.is_ascii_digit())
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}
