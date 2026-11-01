//! Settings → Permissions: the approval requests Patchwork answers itself, by the rules for the
//! thread's project (or for chats without one) and the rules for everywhere.

mod command;
mod defaults;
mod matching;

pub use defaults::{seed, default_rules};

pub mod hook;
mod types;

pub use types::{
    Effect, PermissionKind, PermissionRule, PermissionScope, PermissionSettings, RuleRef,
    ScopeRules,
};

mod decide;

pub use decide::*;
