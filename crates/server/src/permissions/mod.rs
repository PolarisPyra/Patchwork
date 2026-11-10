//! Settings → Permissions: the approval requests Patchwork answers itself, by the rules for the
//! thread's project (or for chats without one) and the rules for everywhere.

// Rust guideline compliant 2026-10-09

mod command;
mod defaults;
mod matching;

#[doc(inline)]
pub use defaults::{seed, default_rules};

pub mod hook;
mod types;

#[doc(inline)]
pub use types::{
    Effect, PermissionKind, PermissionRule, PermissionScope, PermissionSettings, RuleRef,
    ScopeRules,
};

mod decide;

#[doc(inline)]
pub use decide::{
    Decision, Guarded, Target, Unasked, decide, guarded, opencode_name, same_guards, same_request,
    suggest, unasked,
};
