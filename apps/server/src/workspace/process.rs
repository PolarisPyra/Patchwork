//! Signals to process groups and the user's editor. All functions are blocking.

use crate::Error;
use std::path::Path;
use std::process::Stdio;

use crate::types::new_id;

/// Sends `signal` to the process group led by `pid`. Agent children are spawned
/// as group leaders, so this also reaches their own subprocesses.
#[cfg(unix)]
pub fn signal_process_group(pid: u32, signal: i32) {
    // SAFETY: kill(2) has no memory-safety preconditions; a group that already
    // exited just yields an ignored error.
    unsafe { libc::kill(-(pid as libc::pid_t), signal) };
}

#[cfg(not(unix))]
pub fn signal_process_group(_pid: u32, _signal: i32) {}

/// Signals for `signal_process_group`. Windows libc has no SIGKILL; the call is a no-op
/// there, so the values only need to exist.
#[cfg(unix)]
pub use libc::{SIGKILL, SIGTERM};
#[cfg(not(unix))]
pub const SIGKILL: i32 = 9;
#[cfg(not(unix))]
pub const SIGTERM: i32 = 15;

pub fn open_in_editor(root: &str, rel: &str, token: Option<&str>) -> Result<(), Error> {
    let path = Path::new(root).join(rel);
    let fallback = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let candidates = ["idea", "code", "zed", fallback];
    for program in candidates {
        let mut cmd = crate::host_command(program);
        crate::launch::pass_token(&mut cmd, token);
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
        r#"const file = {file}, app = {app}, own = {own};
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
"#,
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
        crate::host_command("gdbus")
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
