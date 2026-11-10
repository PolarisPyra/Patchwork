//! What agents may do without asking, by scope.

// Rust guideline compliant 2026-10-09

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

/// What agents may do without asking: for every project and chat, for chats without a project,
/// and for each project.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PermissionSettings {
    pub everywhere: ScopeRules,
    pub chats: ScopeRules,
    /// By project id.
    pub projects: HashMap<String, ScopeRules>,
    /// Set once the starting rules were added, so deleting one does not bring it back.
    pub seeded: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScopeRules {
    /// What each kind of request gets when no rule matches it; a missing kind inherits.
    pub defaults: BTreeMap<PermissionKind, Effect>,
    pub rules: Vec<PermissionRule>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRule {
    pub id: String,
    pub kind: PermissionKind,
    /// A folder, a command's leading words, a web host, or an MCP server; `*` is anything, and
    /// `.` is the thread's own folder.
    pub pattern: String,
    pub effect: Effect,
}

/// The kinds of request a rule can answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionKind {
    Edit,
    Command,
    Folder,
    Web,
    Mcp,
    /// The screen: a screenshot, or the pointer and keyboard.
    Screen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Allow,
    Ask,
    Deny,
}

/// Where a permission rule applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "projectId", rename_all = "lowercase")]
pub enum PermissionScope {
    Everywhere,
    Chats,
    Project(String),
}

/// The rule that answered an approval: a rule by id, or the scope's default for the kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleRef {
    pub scope: PermissionScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
}
