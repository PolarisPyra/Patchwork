//! Plugins: a folder with a `plugin.json`, the hooks it runs, and the apps it names. Skills belong
//! in the skills folder and MCP servers in Settings → MCP Servers, so a plugin carries neither.
//! The format is Codex's `.codex-plugin/plugin.json` where it can be, so a Codex bundle works as
//! it is. The app ships none. A plugin that only runs hooks is one `hooks.json`, named for its
//! folder; a fuller one puts its hooks in its `plugin.json` as `"hooks": { … }`, or in a file the
//! manifest names, such as Codex's `"hooks": "./hooks.json"`.
//!
//! Patchwork runs the turn hooks itself, for every agent. Each agent runs the rest on its own
//! events, such as before a tool call, so they get the event as JSON on stdin and can refuse it:
//! Claude Code reads them from its plugin folder, Codex from `-c` overrides, Antigravity from a
//! `hooks.json` it is pointed at, and Patchwork's own agent runs the tool hooks around its calls.
//! OpenCode has no hooks of this kind.

mod agent_hooks;
pub mod builtin;
mod events;
mod manifest;
mod turn_hooks;

pub use agent_hooks::{AgentHook, HookAction, agent_hooks, run_tool_hook};
use manifest::read_bundle;
pub use turn_hooks::{HookContext, run_hooks};

mod types;

pub use types::{HookEvent, Plugin, PluginApp, PluginHook, PluginHookHandler};

mod folder;

pub use folder::*;
