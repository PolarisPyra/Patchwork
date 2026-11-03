//! The integrated terminal: the shell it starts, with its arguments, folder and environment, and
//! the running shells, each in a pseudo-terminal.

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::{Duration, Instant};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use ts_rs::TS;

use crate::Res;
use crate::settings::{DetectLocale, Settings};

/// A shell the terminal started.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "terminal.ts")]
#[serde(rename_all = "camelCase")]
pub struct StartedTerminal {
    pub id: u32,
    /// The shell's file name, such as "zsh", for the terminal's tab.
    pub name: String,
    pub cwd: String,
}

/// What a shell printed, as the `terminal-output` event carries it.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "terminal.ts")]
pub struct TerminalOutput {
    pub id: u32,
    pub data: String,
}

/// A shell's end, as the `terminal-exit` event carries it.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "terminal.ts")]
pub struct TerminalExit {
    pub id: u32,
    pub code: Option<u32>,
}

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

/// How long output gathers before it is sent, and the most sent at once.
const BATCH: Duration = Duration::from_millis(8);
const BATCH_BYTES: usize = 256 * 1024;
/// How often a shell whose output has ended is checked for having exited.
const REAP_EVERY: Duration = Duration::from_millis(50);

impl Terminals {
    /// Starts a shell in `cwd` (the project folder, or None for the home folder) at the size of
    /// its view. What it prints goes to `on_output`, and its end to `on_exit`. It blocks while the
    /// shell starts, so call it off the main thread.
    pub fn spawn(
        &self,
        settings: &Settings,
        cwd: Option<&Path>,
        cols: u16,
        rows: u16,
        on_output: impl Fn(TerminalOutput) + Send + 'static,
        on_exit: impl FnOnce(TerminalExit) + Send + 'static,
    ) -> Res<StartedTerminal> {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let open = self.open.clone();
        let begun = Instant::now();
        let launch = launch(settings, cwd);
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
                    on_output(TerminalOutput { id, data });
                }
            }
            // The shell stays in the map, where `kill` reaches it, until it has exited: its
            // output can end before it does.
            let code = loop {
                let mut shells = open.lock().unwrap_or_else(PoisonError::into_inner);
                let Some(shell) = shells.get_mut(&id) else {
                    break None;
                };
                match shell.child.try_wait() {
                    Ok(None) => {}
                    exited => {
                        shells.remove(&id);
                        break exited.ok().flatten().map(|status| status.exit_code());
                    }
                }
                drop(shells);
                std::thread::sleep(REAP_EVERY);
            };
            log::info!("terminal {id}: exited with {code:?}");
            on_exit(TerminalExit { id, code });
        });

        let name = Path::new(&launch.shell)
            .file_name()
            .map_or(launch.shell.clone(), |n| n.to_string_lossy().into_owned());
        Ok(StartedTerminal {
            id,
            name,
            cwd: launch.cwd.to_string_lossy().into_owned(),
        })
    }

    /// Types `data` into the shell.
    pub fn write(&self, id: u32, data: &str) -> Res<()> {
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        let shell = open.get_mut(&id).ok_or("The terminal has closed")?;
        shell
            .writer
            .write_all(data.as_bytes())
            .and_then(|()| shell.writer.flush())
            .map_err(|e| e.to_string())
    }

    /// Follows the view's size, so full-screen programs lay out to it.
    pub fn resize(&self, id: u32, cols: u16, rows: u16) -> Res<()> {
        let open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
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

    /// Ends the shell; its `on_exit` follows.
    pub fn kill(&self, id: u32) {
        // The shell stays in the map until its reader thread has reaped it.
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(shell) = open.get_mut(&id) {
            let _ = shell.child.kill();
        }
    }
}

/// A shell to start.
pub struct Launch {
    pub shell: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
}

/// What `inherit_env: false` keeps of Patchwork's environment: enough to log in and reach the
/// desktop session.
const MINIMAL_ENV: [&str; 12] = [
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "PATH",
    "LANG",
    "TMPDIR",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
    "SystemRoot",
];

/// Variables Patchwork sets on itself, which a shell should not start with.
const OWN_ENV: [&str; 1] = ["WEBKIT_DISABLE_DMABUF_RENDERER"];

/// The shell for a terminal in `workspace`, the project folder when there is one.
pub fn launch(settings: &Settings, workspace: Option<&Path>) -> Launch {
    let terminal = &settings.terminal;
    // On Windows the shell agents default to is Git Bash, which suits commands a model writes; the
    // terminal stays in PowerShell unless a shell was chosen.
    let chosen = settings.shell.as_deref();
    let shell = if cfg!(windows) && chosen.is_none() {
        None
    } else {
        crate::system::effective_shell(chosen)
    };
    let shell = shell.unwrap_or_else(|| {
        if cfg!(windows) {
            "powershell.exe".into()
        } else {
            "/bin/sh".into()
        }
    });
    let args = terminal.shell_args.clone().unwrap_or_else(|| {
        if cfg!(target_os = "macos") {
            vec!["-l".into()]
        } else {
            Vec::new()
        }
    });

    // The process's environment, or the minimal one.
    let mut env: BTreeMap<String, String> = crate::system::host_env()
        .into_iter()
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
        .filter(|(key, _)| {
            terminal.inherit_env || cfg!(windows) || MINIMAL_ENV.contains(&key.as_str())
        })
        .filter(|(key, _)| !OWN_ENV.contains(&key.as_str()))
        .collect();

    // The settings' variables, resolved against that environment; null removes one.
    let folder = workspace
        .map(|w| w.to_string_lossy().into_owned())
        .unwrap_or_default();
    let resolved: Vec<(String, Option<String>)> = terminal
        .env
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                value.as_deref().map(|v| resolve(v, &env, &folder)),
            )
        })
        .collect();
    for (key, value) in resolved {
        match value {
            Some(value) => env.insert(key, value),
            None => env.remove(&key),
        };
    }

    // The terminal's own variables.
    env.insert("TERM".into(), "xterm-256color".into());
    env.insert("COLORTERM".into(), "truecolor".into());
    env.insert("TERM_PROGRAM".into(), "patchwork".into());
    env.insert("TERM_PROGRAM_VERSION".into(), crate::VERSION.into());
    let lang = env.get("LANG").cloned().unwrap_or_default();
    let utf8 =
        lang.to_ascii_lowercase().contains("utf-8") || lang.to_ascii_lowercase().contains("utf8");
    let set_lang = match terminal.detect_locale {
        DetectLocale::On => true,
        DetectLocale::Auto => !utf8,
        DetectLocale::Off => false,
    };
    if set_lang {
        let language = lang
            .split(['.', '@'])
            .next()
            .filter(|l| l.contains('_'))
            .unwrap_or("en_US");
        env.insert("LANG".into(), format!("{language}.UTF-8"));
    }

    let home = crate::system::fs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let base = workspace
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.clone());
    let cwd = terminal
        .cwd
        .as_deref()
        .filter(|c| !c.trim().is_empty())
        .map(|c| base.join(resolve(c, &env, &folder)))
        .filter(|c| c.is_dir())
        .unwrap_or(base);
    env.insert("PWD".into(), cwd.to_string_lossy().into_owned());
    Launch {
        shell,
        args,
        cwd,
        env,
    }
}

/// `${env:NAME}` and `${workspaceFolder}` in a setting; any other `${…}` is left as written.
fn resolve(value: &str, env: &BTreeMap<String, String>, folder: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let Some(len) = rest[start..].find('}') else {
            rest = &rest[start..];
            break;
        };
        let name = &rest[start + 2..start + len];
        match name.strip_prefix("env:") {
            Some(var) => out.push_str(env.get(var).map_or("", String::as_str)),
            None if name == "workspaceFolder" => out.push_str(folder),
            None => out.push_str(&rest[start..=start + len]),
        }
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    out
}
