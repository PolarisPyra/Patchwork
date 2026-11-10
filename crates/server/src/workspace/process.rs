//! Signals to process groups and the user's editor. All functions are blocking.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::path::Path;
use std::process::Stdio;

#[cfg(target_os = "linux")]
use crate::system::new_id;

/// Sends `signal` to the process group led by `pid`. Agent children are spawned
/// as group leaders, so this also reaches their own subprocesses.
#[cfg(unix)]
pub fn signal_process_group(pid: u32, signal: i32) {
    // SAFETY: kill(2) has no memory-safety preconditions; a group that already
    // exited just yields an ignored error.
    unsafe { libc::kill(-(pid as libc::pid_t), signal) };
}

/// Ends the process and the processes it started, as `taskkill /T` finds them.
///
/// Windows has no signals a console program can be sent, so either one does this. Agents started
/// through a `.cmd` shim run under `cmd.exe`, so killing the shim alone would leave the agent
/// running.
#[cfg(not(unix))]
pub fn signal_process_group(pid: u32, _signal: i32) {
    let _ = crate::system::host_command("taskkill")
        .args(["/T", "/F", "/PID"])
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Signals for `signal_process_group`. Windows libc has no SIGKILL, and the call ignores the
/// signal there, so the values only need to exist.
#[cfg(unix)]
pub use libc::{SIGKILL, SIGTERM};
#[cfg(not(unix))]
pub const SIGKILL: i32 = 9;
#[cfg(not(unix))]
pub const SIGTERM: i32 = 15;

/// Opens the file `rel` of the project at `root` in an editor.
///
/// # Errors
///
/// Returns an error when none of the known editors or the system opener can be started.
pub fn open_in_editor(root: &str, rel: &str, token: Option<&str>) -> Result<(), Error> {
    let path = Path::new(root).join(rel);
    let fallback = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    let candidates = ["idea", "code", "code-insiders", "cursor", "zed", fallback];
    for program in candidates {
        let Some(bin) = editor_program(program) else {
            continue;
        };
        let mut cmd = crate::system::host_command(bin);
        crate::system::launch::pass_token(&mut cmd, token);
        let spawned = cmd
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if let Ok(mut child) = spawned {
            // Reap the launcher in the background so it does not linger as a zombie.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            #[cfg(target_os = "linux")]
            raise_editor(program, &path);
            return Ok(());
        }
    }
    Err(format!("No editor found (tried {})", candidates.join(", ")).into())
}

/// The editor `program` on PATH. On Windows the editors' launchers are `.cmd` files, which only a
/// search with the system's extensions finds, and installers that skip PATH are looked for where
/// they install.
fn editor_program(program: &str) -> Option<std::path::PathBuf> {
    if let Some(found) = crate::system::find_program(program) {
        return Some(found);
    }
    if !cfg!(windows) {
        return Some(program.into());
    }
    let folder = match program {
        "code" => r"Microsoft VS Code\bin\code.cmd",
        "code-insiders" => r"Microsoft VS Code Insiders\bin\code-insiders.cmd",
        "cursor" => r"cursor\resources\app\bin\cursor.cmd",
        "zed" => r"Zed\bin\zed.exe",
        _ => return None,
    };
    let local = std::env::var_os("LOCALAPPDATA").map(|d| Path::new(&d).join("Programs"));
    let system = std::env::var_os("ProgramFiles").map(std::path::PathBuf::from);
    [local, system]
        .into_iter()
        .flatten()
        .map(|dir| dir.join(folder))
        .find(|p| p.is_file())
}

/// On KDE, brings the editor's window to the front once it shows the file. KWin's focus-stealing
/// prevention keeps a window behind when the app that raises it has no activation token, and a
/// running editor that is handed the file never gets one. A KWin script may activate any
/// window, so one watches for the editor window whose title names the file, for a few seconds.
#[cfg(target_os = "linux")]
fn raise_editor(program: &str, path: &Path) {
    let on_kde = std::env::var("XDG_CURRENT_DESKTOP")
        .is_ok_and(|desktop| desktop.split(':').any(|d| d == "KDE"));
    if !on_kde {
        return;
    }
    let Some(file) = path.file_name().map(|f| f.to_string_lossy().into_owned()) else {
        return;
    };
    // `xdg-open` picks the app, so any window but Patchwork's own may be the editor.
    let app = if program == "xdg-open" {
        String::new()
    } else {
        program.to_lowercase()
    };
    let script = format!(
        r"const file = {file}, app = {app}, own = {own};
let done = false;
function raise(w) {{
  if (done || !w.normalWindow || w.pid === own || !w.caption.includes(file)) return;
  const names = [w.resourceName, w.resourceClass, w.desktopFileName].map((n) => String(n).toLowerCase());
  if (app && !names.some((n) => n.includes(app))) return;
  done = true;
  w.minimized = false;
  workspace.activeWindow = w;
}}
function watch(w) {{
  raise(w);
  w.captionChanged.connect(() => raise(w));
}}
workspace.windowList().forEach(watch);
workspace.windowAdded.connect(watch);
",
        file = serde_json::to_string(&file).unwrap_or_default(),
        app = serde_json::to_string(&app).unwrap_or_default(),
        own = std::process::id(),
    );
    let name = format!("patchwork-raise-{}", new_id());
    let script_path = std::env::temp_dir().join(format!("{name}.js"));
    if std::fs::write(&script_path, script).is_err() {
        return;
    }
    let kwin = |method: &str, args: &[&str]| {
        crate::system::host_command("gdbus")
            .args([
                "call",
                "--session",
                "-d",
                "org.kde.KWin",
                "-o",
                "/Scripting",
                "-m",
            ])
            .arg(format!("org.kde.kwin.Scripting.{method}"))
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    std::thread::spawn(move || {
        let script = script_path.to_string_lossy();
        if kwin("loadScript", &[&script, &name]) && kwin("start", &[]) {
            // Long enough for a cold editor to start; the script stops watching when unloaded.
            std::thread::sleep(std::time::Duration::from_secs(8));
            kwin("unloadScript", &[&name]);
        }
        let _ = std::fs::remove_file(&script_path);
    });
}
