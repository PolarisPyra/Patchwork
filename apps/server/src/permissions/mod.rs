//! Settings → Permissions: the approval requests Patchwork answers itself, by the rules for the
//! thread's project (or for chats without one) and the rules for everywhere.

mod command;
mod defaults;
mod matching;

use command::{command_prefix, split_command};
pub use defaults::{seed, default_rules};
use matching::{Unit, units, covers, folder_of, shown, host};

pub mod hook;
mod types;

pub use types::{
    Effect, PermissionKind, PermissionRule, PermissionScope, PermissionSettings, RuleRef,
    ScopeRules,
};

use std::path::{Path, PathBuf};

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

/// The rules' answer to a request the access mode would run without asking.
#[derive(Debug, Clone, PartialEq)]
pub enum Unasked {
    /// No rule stops it.
    Run,
    /// A deny rule, or the kind's deny default, refuses it.
    Refuse(Decision),
    /// An ask rule covers it, so the user is asked anyway.
    Ask,
}

/// What `decide` found.
enum Answer {
    Given(Decision),
    AskRule,
    Open,
}

/// The rules' answer to `target` in a thread of `project` (None for a chat) working in `root`, or
/// None when the user is to be asked. Deny and ask rules win when they cover any part of the
/// request; allow rules have to cover all of it, and reading in a `skills` folder needs none.
/// Then the scope's default for the kind decides, then the default for everywhere.
pub fn decide(
    settings: &PermissionSettings,
    project: Option<&str>,
    root: &Path,
    target: &Target,
    skills: &[PathBuf],
) -> Option<Decision> {
    match answer(settings, project, root, target, skills) {
        Answer::Given(decision) => Some(decision),
        Answer::AskRule | Answer::Open => None,
    }
}

/// The rules' answer to `target` when the access mode would run it without asking: deny rules
/// and a deny default still refuse it, and ask rules ask unless `asks` is false, as in full
/// access, which never asks.
pub fn unasked(
    settings: &PermissionSettings,
    project: Option<&str>,
    root: &Path,
    target: &Target,
    skills: &[PathBuf],
    asks: bool,
) -> Unasked {
    match answer(settings, project, root, target, skills) {
        Answer::Given(decision) if !decision.allow => Unasked::Refuse(decision),
        Answer::AskRule if asks => Unasked::Ask,
        _ => Unasked::Run,
    }
}

/// The deny and ask rules of one kind that hold in a thread, for an agent that has to be told at
/// launch what to ask about: `opencode run` (1.x), which cannot ask Patchwork mid-turn. Codex,
/// Claude, and Antigravity read the rules at each call instead (see `permissions::hook`).
#[derive(Debug, Clone, PartialEq)]
pub struct Guarded {
    pub kind: PermissionKind,
    pub deny: Vec<String>,
    pub ask: Vec<String>,
    /// Whether the kind is denied where no rule covers it, so every request of it is guarded.
    pub denied: bool,
}

impl Guarded {
    /// Whether every request of the kind is guarded.
    pub fn all(&self) -> bool {
        self.denied || self.patterns().any(|p| p == "*")
    }

    /// The deny and ask rules' patterns.
    pub fn patterns(&self) -> impl Iterator<Item = &str> {
        self.deny.iter().chain(&self.ask).map(String::as_str)
    }

    /// Whether a request of the kind may be refused, rather than only asked about.
    pub fn refuses(&self) -> bool {
        self.denied || !self.deny.is_empty()
    }
}

/// The kinds a thread of `project` (None for a chat) has deny or ask rules for, or denies by
/// default.
pub fn guarded(settings: &PermissionSettings, project: Option<&str>) -> Vec<Guarded> {
    let empty = ScopeRules::default();
    let local = match project {
        Some(id) => settings.projects.get(id).unwrap_or(&empty),
        None => &settings.chats,
    };
    let scopes = [local, &settings.everywhere];
    [
        PermissionKind::Edit,
        PermissionKind::Command,
        PermissionKind::Folder,
        PermissionKind::Web,
        PermissionKind::Mcp,
    ]
    .into_iter()
    .filter_map(|kind| {
        let (mut deny, mut ask) = (Vec::<String>::new(), Vec::<String>::new());
        for rule in scopes.iter().flat_map(|s| &s.rules) {
            let pattern = rule.pattern.trim();
            let list = match rule.effect {
                Effect::Deny => &mut deny,
                Effect::Ask => &mut ask,
                Effect::Allow => continue,
            };
            if rule.kind == kind && !pattern.is_empty() && !list.iter().any(|p| p == pattern) {
                list.push(pattern.into());
            }
        }
        let denied = scopes.iter().find_map(|s| s.defaults.get(&kind)) == Some(&Effect::Deny);
        (denied || !deny.is_empty() || !ask.is_empty()).then_some(Guarded {
            kind,
            deny,
            ask,
            denied,
        })
    })
    .collect()
}

/// Whether `a` and `b` guard the same calls in every chat and project, so agents launched with
/// one need not start again for the other.
pub fn same_guards(a: &PermissionSettings, b: &PermissionSettings) -> bool {
    let mut projects = a.projects.keys().chain(b.projects.keys());
    guarded(a, None) == guarded(b, None)
        && projects.all(|id| guarded(a, Some(id)) == guarded(b, Some(id)))
}

fn answer(
    settings: &PermissionSettings,
    project: Option<&str>,
    root: &Path,
    target: &Target,
    skills: &[PathBuf],
) -> Answer {
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
    // Reading in its own folder is not working outside it.
    if target.kind == PermissionKind::Folder
        && !target.resources.is_empty()
        && target
            .resources
            .iter()
            .all(|r| folder_of(root, r).starts_with(root))
    {
        return Answer::Open;
    }
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
                if effect == Effect::Ask {
                    return Answer::AskRule;
                }
                return Answer::Given(Decision {
                    allow: false,
                    rule: found(scope, rule),
                });
            }
        }
    }

    if allowable {
        let allowing = |unit: &Unit| {
            // Agents read their skills without asking.
            if target.kind == PermissionKind::Folder
                && let Unit::Path(path) = unit
                && skills.iter().any(|folder| path.starts_with(folder))
            {
                return Some(RuleRef {
                    scope: PermissionScope::Everywhere,
                    rule_id: None,
                });
            }
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
            return Answer::Given(Decision { allow: true, rule });
        }
    }

    for (scope, rules) in scopes {
        match rules.defaults.get(&target.kind) {
            Some(Effect::Ask) => return Answer::Open,
            Some(effect) => {
                return Answer::Given(Decision {
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
    Answer::Open
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
        PermissionKind::Screen => {
            for resource in &target.resources {
                add(resource.clone());
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

/// Whether `b` asks for what `a` did: the same kind and the same commands, paths, hosts, or tools.
/// A request that names nothing, as Codex's file changes do, matches any of its kind.
pub fn same_request(a: &Target, b: &Target, root: &Path) -> bool {
    a.kind == b.kind && (b.resources.is_empty() || units(a, root).0 == units(b, root).0)
}

/// A server or tool name as OpenCode puts it in a tool's id: anything but letters, digits, `_`,
/// and `-` becomes `_`.
pub fn opencode_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
