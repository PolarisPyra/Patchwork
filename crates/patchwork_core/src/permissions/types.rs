//! What agents may do without asking, by scope, whose types live in `patchwork_model`.

// Rust guideline compliant 2026-11-27

pub use patchwork_model::permissions::{
    Effect, PermissionKind, PermissionRule, PermissionScope, PermissionSettings, RuleRef,
    ScopeRules,
};
