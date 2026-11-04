//! The integrated terminal's shells. They belong to the app rather than the window: a window
//! attaches to one and gets its screen, then its output and its end through a channel, and a
//! window that reloads attaches again.

use super::*;

use patchwork_server::terminal::{
    AttachedTerminal, FrameSink, TerminalFrame, TerminalInfo, TerminalShell,
};

/// The shells a terminal can start, the user's first. Finding them can wait on the first read of
/// the user's shell, so it runs on a worker thread.
#[tauri::command]
pub(crate) async fn terminal_shells() -> Res<Vec<TerminalShell>> {
    blocking(patchwork_server::terminal::shells).await
}

/// The terminals that are open, for a window that loads to attach to.
#[tauri::command]
pub(crate) fn terminal_list(server: Srv<'_>) -> Vec<TerminalInfo> {
    server.terminals.list()
}

/// Starts `shell`, else the user's, in `cwd` (the project folder, or None for the home folder)
/// at the size of its view, attached to this window. It starts on a worker thread, so the window
/// keeps drawing meanwhile.
#[tauri::command]
pub(crate) async fn terminal_spawn(
    webview: tauri::Webview,
    server: Srv<'_>,
    cwd: Option<String>,
    shell: Option<String>,
    cols: u16,
    rows: u16,
    on_frame: Channel<TerminalFrame>,
) -> Res<AttachedTerminal> {
    let server = server.inner().clone();
    let page = webview.label().to_string();
    blocking(move || {
        let launch = patchwork_server::terminal::launch(
            &server.store.settings(),
            cwd.as_deref().map(std::path::Path::new),
            shell.as_deref(),
        )?;
        server
            .terminals
            .spawn(launch, cols, rows, &page, sink(on_frame))
    })
    .await?
}

/// Attaches this window to terminal `id`, which it types into from then on.
#[tauri::command]
pub(crate) fn terminal_attach(
    webview: tauri::Webview,
    server: Srv<'_>,
    id: u32,
    on_frame: Channel<TerminalFrame>,
) -> Res<AttachedTerminal> {
    server.terminals.attach(id, webview.label(), sink(on_frame))
}

/// Stops sending terminal `id` to `attachment`.
#[tauri::command]
pub(crate) fn terminal_detach(server: Srv<'_>, id: u32, attachment: u32) {
    server.terminals.detach(id, attachment);
}

/// Types `data` into the shell.
#[tauri::command]
pub(crate) fn terminal_write(server: Srv<'_>, id: u32, attachment: u32, data: String) -> Res<()> {
    server.terminals.write(id, attachment, &data)
}

/// Follows the view's size, so full-screen programs lay out to it.
#[tauri::command]
pub(crate) fn terminal_resize(
    server: Srv<'_>,
    id: u32,
    attachment: u32,
    cols: u16,
    rows: u16,
) -> Res<()> {
    server.terminals.resize(id, attachment, cols, rows)
}

/// Closes terminal `id`, ending its shell and everything it started. That can take a few
/// seconds, so it runs on a worker thread.
#[tauri::command]
pub(crate) async fn terminal_close(server: Srv<'_>, id: u32) -> Res<()> {
    let server = server.inner().clone();
    blocking(move || server.terminals.close(id)).await?
}

fn sink(channel: Channel<TerminalFrame>) -> FrameSink {
    Box::new(move |frame| channel.send(frame.clone()).is_ok())
}
