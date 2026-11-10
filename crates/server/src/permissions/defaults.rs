//! The rules Settings → Permissions starts with, which Restore defaults puts back.

// Rust guideline compliant 2026-10-09

use super::{Effect, PermissionKind, PermissionRule, PermissionSettings};

/// Adds the starting rules once, so deleting one does not bring it back.
pub fn seed(settings: &mut PermissionSettings) {
    if settings.seeded {
        return;
    }
    settings.seeded = true;
    settings.everywhere.rules.extend(default_rules());
}

/// The starting permission rules that apply everywhere.
///
/// The starting rules for everywhere: commands that only read run without asking, and commands
/// that delete, escalate, or rewrite history ask, whatever allow rule covers them. Settings →
/// Permissions puts them back with Restore defaults.
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
    let risky = ["sudo", "rm", "git push", "git reset", "git clean", "dd"];
    reads
        .iter()
        .map(|p| rule(p, Effect::Allow))
        .chain(risky.iter().map(|p| rule(p, Effect::Ask)))
        .collect()
}
