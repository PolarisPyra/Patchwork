//! The integrated terminal's shells. What a shell prints goes to the window as `terminal-output`
//! events, and its end as a `terminal-exit` event.

use super::*;

use patchwork_server::terminal::StartedTerminal;

/// Starts a shell in `cwd` (the project folder, or None for the home folder) at the size
/// of its view. It starts on a worker thread, so the window keeps drawing meanwhile.
#[tauri::command]
pub(crate) async fn terminal_spawn(
    app: AppHandle,
    server: Srv<'_>,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
) -> Res<StartedTerminal> {
    let server = server.inner().clone();
    let exit_app = app.clone();
    blocking(move || {
        server.terminals.spawn(
            &server.store.settings(),
            cwd.as_deref().map(std::path::Path::new),
            cols,
            rows,
            move |output| {
                let _ = app.emit("terminal-output", output);
            },
            move |exit| {
                let _ = exit_app.emit("terminal-exit", exit);
            },
        )
    })
    .await?
}

/// Types `data` into the shell.
#[tauri::command]
pub(crate) fn terminal_write(server: Srv<'_>, id: u32, data: String) -> Res<()> {
    server.terminals.write(id, &data)
}

/// Follows the view's size, so full-screen programs lay out to it.
#[tauri::command]
pub(crate) fn terminal_resize(server: Srv<'_>, id: u32, cols: u16, rows: u16) -> Res<()> {
    server.terminals.resize(id, cols, rows)
}

/// Ends the shell; its `terminal-exit` event follows.
#[tauri::command]
pub(crate) fn terminal_kill(server: Srv<'_>, id: u32) {
    server.terminals.kill(id);
}
