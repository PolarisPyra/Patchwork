//! What a token may do, by method.
//!
//! The server's own token has every scope; a paired device has the scopes it was given, and every
//! call it makes is checked against them.

// Rust guideline compliant 2026-10-09

/// Every scope a token can carry.
pub const SCOPES: [&str; 4] = [READ, WRITE, TERMINAL, ADMIN];

/// Reading chats, projects and settings.
pub const READ: &str = "read";
/// Sending messages, answering approvals, and changing chats and projects.
pub const WRITE: &str = "write";
/// The integrated terminal.
pub const TERMINAL: &str = "terminal";
/// Pairing and revoking devices, and the provider settings; it also covers every other scope.
pub const ADMIN: &str = "admin";

/// Methods, or method prefixes ending in `.`, that change what is stored or start work.
const MUTATING: &[&str] = &[
    "turn.",
    "approval.",
    "chats.create",
    "chats.update",
    "chats.delete",
    "projects.add",
    "projects.remove",
    "projects.reorder",
    "settings.set",
    "sourceControl.",
    "worktree.",
    "checkpoint.restore",
    "orchestrator.send",
    "orchestrator.answer",
    "issues.prompt",
];

/// Methods, or method prefixes ending in `.`, only an administrator may call.
const ADMIN_ONLY: &[&str] = &[
    "auth.",
    "providers.save",
    "providers.remove",
    "mcp.attach",
    "providers.models",
];

/// Whether `method` is `pattern`, or starts with it when it ends in `.`.
fn covers(pattern: &str, method: &str) -> bool {
    method == pattern || (pattern.ends_with('.') && method.starts_with(pattern))
}

/// The scope a method needs.
#[must_use]
pub fn scope_for(method: &str) -> &'static str {
    if method.starts_with("terminal.") {
        TERMINAL
    } else if ADMIN_ONLY.iter().any(|p| covers(p, method)) {
        ADMIN
    } else if MUTATING.iter().any(|p| covers(p, method)) {
        WRITE
    } else {
        READ
    }
}

/// Whether a token with `scopes` may call `method`. `admin` covers the rest, and every token
/// may read.
#[must_use]
pub fn may_call(scopes: &[String], method: &str) -> bool {
    let has = |scope: &str| scopes.iter().any(|s| s == scope);
    if has(ADMIN) {
        return true;
    }
    match scope_for(method) {
        READ => true,
        needed => has(needed),
    }
}

/// Whether every one of `scopes` is a scope a token can carry.
#[must_use]
pub fn known(scopes: &[String]) -> bool {
    scopes.iter().all(|s| SCOPES.contains(&s.as_str()))
}
