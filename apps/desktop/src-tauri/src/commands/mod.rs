//! The Tauri commands the webview calls, grouped by domain. `lib.rs` registers each one.
//! Each group `use super::*;` for the shared imports and helpers below.

pub mod app;
pub mod codex;
pub mod computer;
pub mod files;
pub mod mcp;
pub mod plugins;
pub mod providers;
pub mod scheduled;
pub mod settings;
pub mod threads;

use std::sync::Arc;

use crate::{clipboard, logo, updates};

use patchwork_server::types::{
    AgentKind, Project, Settings, AgentStatus, ModelOption, ShellChoices, AccessMode, Attachment,
    Message, Thread, Folder, FileChange, ProviderUsage, ImportSource, ImportScan, TurnTrees,
    ScheduledTask, Plugin, CodexInfo,
};
use patchwork_server::workspace;
use patchwork_server::{NewThread, Res, Server, orchestration, provider};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};

type Srv<'a> = State<'a, Arc<Server>>;

/// Runs blocking work (CLI probes, network, disk scans) off the main thread.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())
}

fn open_folder(app: &AppHandle, dir: &std::path::Path) -> Res<()> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    open_in_viewer(app, dir)
}

/// Opens a file or folder in the user's default app.
#[cfg_attr(target_os = "linux", allow(unused_variables))]
fn open_in_viewer(app: &AppHandle, path: &std::path::Path) -> Res<()> {
    #[cfg(target_os = "linux")]
    {
        let mut child = patchwork_server::host_command("xdg-open")
            .arg(path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("Could not open {}: {e}", path.display()))?;
        std::thread::spawn(move || child.wait());
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .open_path(path.to_string_lossy(), None::<&str>)
            .map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub(crate) fn list_models(kind: AgentKind) -> Vec<ModelOption> {
    provider::list_models(kind)
}
