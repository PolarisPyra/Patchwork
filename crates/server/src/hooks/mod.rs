//! Hooks: the bundles in the hooks folder, each a folder with a `hooks.json` or a `plugin.json`,
//! the hooks it runs, and the apps it names.
//!
//! Skills belong in the skills folder and MCP servers in Settings → MCP Servers, so a bundle
//! carries neither. The format is Codex's `.codex-plugin/plugin.json` where it can be, so a Codex
//! bundle works as it is. The app ships none. A bundle that only runs hooks is one `hooks.json`,
//! named for its folder; a fuller one puts its hooks in its `plugin.json` as `"hooks": { … }`, or
//! in a file the manifest names, such as Codex's `"hooks": "./hooks.json"`.
//!
//! Patchwork runs the turn hooks itself, for every agent. Each agent runs the rest on its own
//! events, such as before a tool call, so they get the event as JSON on stdin and can refuse it:
//! Claude Code reads them from its plugin folder, Codex from `-c` overrides, Antigravity from a
//! `hooks.json` it is pointed at, and Patchwork's own agent runs the tool hooks around its calls.
//! OpenCode has no hooks of its own, so Patchwork runs the ones for before a tool call when
//! OpenCode asks it about the call.

// Rust guideline compliant 2026-10-09

mod agent_hooks;
mod events;
mod manifest;
mod process;
mod turn_hooks;

#[doc(inline)]
pub use agent_hooks::{AgentHook, HookAction, agent_hooks, run_tool_hook};
use manifest::read_bundle;
#[doc(inline)]
pub use turn_hooks::{HookContext, run_hooks};

mod types;

#[doc(inline)]
pub use types::{HookEvent, Bundle, BundleApp, Hook, HookHandler};

mod folder;

#[doc(inline)]
pub use folder::{dir, init, list};
