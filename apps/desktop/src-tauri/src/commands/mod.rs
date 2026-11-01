//! The Tauri commands the webview calls, grouped by domain. `lib.rs` registers each one.
//! Each group `use super::*;` for the shared imports and the helpers in `support`.

pub mod agents;
pub mod app;
pub mod attachments;
pub mod computer;
pub mod files;
pub mod git;
pub mod imports;
pub mod logs;
pub mod mcp;
pub mod models;
pub mod projects;
pub mod scheduled;
pub mod settings;
pub mod skills;
pub mod terminal;
pub mod threads;
pub mod turns;
pub mod usage;
mod support;

use crate::{activation, clipboard, logo, updates};

use patchwork_server::import::{ImportSource, ImportScan};
use patchwork_server::plugins::Plugin;
use patchwork_server::projects::{Project, Folder};
use patchwork_server::provider::{AgentKind, AgentStatus, ModelOption, CodexInfo, ClaudeInfo};
use patchwork_server::scheduled::ScheduledTask;
use patchwork_server::settings::{Settings, ShellChoices};
use patchwork_server::threads::{AccessMode, Attachment, Message, Thread, TurnTrees};
use patchwork_server::usage::ProviderUsage;
use patchwork_server::workspace::FileChange;
use patchwork_server::workspace;
use patchwork_server::{Res, orchestration, provider};
use patchwork_server::threads::NewThread;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};

use support::*;
