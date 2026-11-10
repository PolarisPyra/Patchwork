//! Hooks Patchwork carries itself.
//!
//! A plugin names one as `{ "type": "builtin", "name": … }`; an agent CLI starts it as `patchwork
//! --hook <name> <agent>` with the call on stdin, and Patchwork's own agent runs it in-process.

// Rust guideline compliant 2026-10-09

mod bounded_waits;
mod shell;

mod hooks;

#[doc(inline)]
pub use hooks::{BuiltinHook, HOOK_ARG, run};
