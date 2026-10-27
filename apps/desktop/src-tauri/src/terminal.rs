//! The integrated terminal's shells, each in a pseudo-terminal. What a shell prints goes to the
//! window as `terminal-output` events, and its end as a `terminal-exit` event.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::{Duration, Instant};

use patchwork_server::{Res, Server};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

/// The running shells, by id.
#[derive(Default)]
pub struct Terminals {
    next: AtomicU32,
    open: Arc<Mutex<HashMap<u32, Shell>>>,
}

struct Shell {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    id: u32,
    /// The shell's file name, for the terminal's tab.
    name: String,
    cwd: String,
}

/// How long output gathers before it is sent, and the most sent at once.
const BATCH: Duration = Duration::from_millis(8);
const BATCH_BYTES: usize = 256 * 1024;

#[derive(Serialize, Clone)]
struct Output {
    id: u32,
    data: String,
}

#[derive(Serialize, Clone)]
struct Exit {
    id: u32,
    code: Option<u32>,
}

/// Starts a shell in `cwd` (the project folder, or None for the home folder) at the size
/// of its view. It starts on a worker thread, so the window keeps drawing meanwhile.
#[tauri::command]
pub async fn terminal_spawn(
    app: AppHandle,
    server: State<'_, Arc<Server>>,
    terminals: State<'_, Terminals>,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
) -> Res<Started> {
    let settings = server.store.settings();
    let open = terminals.open.clone();
    let id = terminals.next.fetch_add(1, Ordering::Relaxed) + 1;
    tauri::async_runtime::spawn_blocking(move || spawn(app, &settings, open, id, cwd, cols, rows))
        .await
        .map_err(|e| e.to_string())?
}

fn spawn(
    app: AppHandle,
    settings: &patchwork_server::types::Settings,
    open: Arc<Mutex<HashMap<u32, Shell>>>,
    id: u32,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
) -> Res<Started> {
    let begun = std::time::Instant::now();
    let launch = patchwork_server::terminal::launch(settings, cwd.as_deref().map(Path::new));
    let pair = native_pty_system()
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("Could not open a terminal: {e}"))?;
    let mut cmd = CommandBuilder::new(&launch.shell);
    cmd.args(&launch.args);
    cmd.cwd(&launch.cwd);
    cmd.env_clear();
    for (key, value) in &launch.env {
        cmd.env(key, value);
    }
    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("Could not start {}: {e}", launch.shell))?;
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    open.lock().unwrap_or_else(PoisonError::into_inner).insert(
        id,
        Shell {
            master: pair.master,
            writer,
            child,
        },
    );
    log::info!(
        "terminal {id}: started {} {} in {} ({} ms)",
        launch.shell,
        launch.args.join(" "),
        launch.cwd.display(),
        begun.elapsed().as_millis()
    );

    // The PTY hands over a few hundred bytes per read, so a noisy command made thousands of
    // events a second. Reads go to a second thread, which sends what gathers within BATCH as one.
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 16 * 1024];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    std::thread::spawn(move || {
        // A character split across two reads waits here for the rest of its bytes.
        let mut pending: Vec<u8> = Vec::new();
        while let Ok(first) = rx.recv() {
            pending.extend_from_slice(&first);
            let deadline = Instant::now() + BATCH;
            while pending.len() < BATCH_BYTES {
                match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(more) => pending.extend_from_slice(&more),
                    Err(_) => break,
                }
            }
            let whole = match std::str::from_utf8(&pending) {
                Ok(_) => pending.len(),
                Err(e) if e.error_len().is_none() => e.valid_up_to(),
                Err(_) => pending.len(),
            };
            let data = String::from_utf8_lossy(&pending[..whole]).into_owned();
            pending.drain(..whole);
            if !data.is_empty() {
                let _ = app.emit("terminal-output", Output { id, data });
            }
        }
        let shell = open
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
        let code = shell
            .and_then(|mut s| s.child.wait().ok())
            .map(|status| status.exit_code());
        log::info!("terminal {id}: exited with {code:?}");
        let _ = app.emit("terminal-exit", Exit { id, code });
    });

    let name = Path::new(&launch.shell)
        .file_name()
        .map_or(launch.shell.clone(), |n| n.to_string_lossy().into_owned());
    Ok(Started {
        id,
        name,
        cwd: launch.cwd.to_string_lossy().into_owned(),
    })
}

/// Types `data` into the shell.
#[tauri::command]
pub fn terminal_write(terminals: State<'_, Terminals>, id: u32, data: String) -> Res<()> {
    let mut open = terminals
        .open
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let shell = open.get_mut(&id).ok_or("The terminal has closed")?;
    shell
        .writer
        .write_all(data.as_bytes())
        .and_then(|()| shell.writer.flush())
        .map_err(|e| e.to_string())
}

/// Follows the view's size, so full-screen programs lay out to it.
#[tauri::command]
pub fn terminal_resize(terminals: State<'_, Terminals>, id: u32, cols: u16, rows: u16) -> Res<()> {
    let open = terminals
        .open
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let shell = open.get(&id).ok_or("The terminal has closed")?;
    shell
        .master
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())
}

/// Ends the shell; its `terminal-exit` event follows.
#[tauri::command]
pub fn terminal_kill(terminals: State<'_, Terminals>, id: u32) {
    let shell = terminals
        .open
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&id);
    if let Some(mut shell) = shell {
        let _ = shell.child.kill();
    }
}
