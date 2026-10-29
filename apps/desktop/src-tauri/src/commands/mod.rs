//! The Tauri commands the webview calls, grouped by domain. `lib.rs` registers each one.
//! Each group `use super::*;` for the shared imports and helpers below.

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
pub mod threads;
pub mod turns;
pub mod usage;

use std::sync::Arc;

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
use patchwork_server::{Res, Server, orchestration, provider};
use patchwork_server::threads::NewThread;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};

type Srv<'a> = State<'a, Arc<Server>>;

/// Runs blocking work (CLI probes, network, disk scans) off the main thread.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())
}

async fn open_folder(app: &AppHandle, dir: &std::path::Path) -> Res<()> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    open_in_viewer(app, dir).await
}

/// Opens a file or folder in the user's default app.
#[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
async fn open_in_viewer(app: &AppHandle, path: &std::path::Path) -> Res<()> {
    let token = activation::token(app).await;
    #[cfg(target_os = "linux")]
    {
        patchwork_server::system::launch::xdg_open(path.as_os_str(), token.as_deref())
            .map_err(|e| format!("Could not open {}: {e}", path.display()))
    }
    #[cfg(not(target_os = "linux"))]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .open_path(path.to_string_lossy(), None::<&str>)
            .map_err(|e| e.to_string())
    }
}

/// Shows `path` selected in the file manager.
#[allow(unused_variables)]
fn reveal_in_file_manager(app: &AppHandle, path: &std::path::Path, token: Option<&str>) -> Res<()> {
    #[cfg(target_os = "linux")]
    {
        patchwork_server::system::launch::reveal(path, token).map_err(String::from)
    }
    #[cfg(not(target_os = "linux"))]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .reveal_item_in_dir(path)
            .map_err(|e| e.to_string())
    }
}

/// Opens `name` in the folder `dir` in the user's editor.
async fn edit_file(app: &AppHandle, dir: String, name: String) -> Res<()> {
    let token = activation::token(app).await;
    blocking(move || workspace::open_in_editor(&dir, &name, token.as_deref()))
        .await?
        .map_err(String::from)
}
