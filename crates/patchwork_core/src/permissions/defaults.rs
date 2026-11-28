//! The rules Settings → Permissions starts with, which Restore defaults puts back.

// Rust guideline compliant 2026-10-09

use super::{Effect, PermissionKind, PermissionRule, PermissionSettings};

/// Starting rules that earlier versions added and this one no longer has.
const RETIRED: [&str; 6] = [
    "default:git push",
    "default:git reset",
    "default:git clean",
    "default:sudo",
    "default:rm",
    "default:dd",
];

/// Adds the starting rules once, so deleting one does not bring it back, and drops the retired
/// ones.
pub fn seed(settings: &mut PermissionSettings) {
    settings
        .everywhere
        .rules
        .retain(|r| !RETIRED.contains(&r.id.as_str()));
    if settings.seeded {
        return;
    }
    settings.seeded = true;
    settings.everywhere.rules.extend(default_rules());
}

/// The starting permission rules that apply everywhere.
///
/// The starting rules for everywhere: commands that only read run without asking. Every other
/// command asks only by the access mode or the user's own rules. Settings → Permissions puts them
/// back with Restore defaults.
#[must_use]
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
    reads.iter().map(|p| rule(p, Effect::Allow)).collect()
}
