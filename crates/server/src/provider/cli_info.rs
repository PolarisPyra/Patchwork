//! Read-only commands of an installed agent CLI, for the Settings panes. The panes ask each
//! binary for its own diagnostics, so the output is what the CLI itself says rather than a copy.

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

/// A read-only command, the only kind the Settings panes run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoCommand {
    /// `doctor`: installation, config, auth and runtime health.
    Doctor,
    /// `mcp list`: the MCP servers the CLI has configured.
    McpList,
    /// `plugin list`: the plugins the CLI has installed.
    PluginList,
}

impl InfoCommand {
    /// The command a Settings pane names: `doctor`, `mcp` or `plugins`.
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "doctor" => Some(InfoCommand::Doctor),
            "mcp" => Some(InfoCommand::McpList),
            "plugins" => Some(InfoCommand::PluginList),
            _ => None,
        }
    }

    fn args(self) -> &'static [&'static str] {
        match self {
            InfoCommand::Doctor => &["doctor"],
            InfoCommand::McpList => &["mcp", "list"],
            InfoCommand::PluginList => &["plugin", "list"],
        }
    }
}

/// Longest a probe may take before it is killed. `doctor` is the slow one.
const TIMEOUT: Duration = Duration::from_secs(60);
/// Longest output kept from a command. The model catalog is nearly 700 KB of JSON, so this is
/// generous; it is a guard against a runaway command, not a display limit.
const MAX_PIPE: usize = 8 * 1024 * 1024;
/// Longest output shown for a command whose output is for a reader, such as `doctor`.
const MAX_OUTPUT: usize = 60_000;

/// Runs the CLI at `binary` with `args` and returns its stdout, then its stderr when stdout was
/// empty.
///
/// Both pipes are read on threads of their own while the process runs. Its output can be far
/// larger than a pipe holds, and a process whose pipe fills up stops, so waiting for it to exit
/// before reading anything would hang on exactly the commands worth reading: the model catalog is
/// nearly 700 KB.
pub fn run(binary: &Path, args: &[&str]) -> Result<String, String> {
    let mut child = crate::system::host_command(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not run {}: {e}", binary.display()))?;
    let out = child.stdout.take().map(drain);
    let err = child.stderr.take().map(drain);
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{} {} took too long",
                    binary.display(),
                    args.join(" ")
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("Could not run {}: {e}", binary.display())),
        }
    };
    // The threads end when the process closes its pipes, which it has.
    let stdout = out
        .map(|out| out.join().unwrap_or_default())
        .unwrap_or_default();
    let stderr = err
        .map(|err| err.join().unwrap_or_default())
        .unwrap_or_default();
    let text = if stdout.trim().is_empty() {
        stderr
    } else {
        stdout
    };
    if !status.success() && text.trim().is_empty() {
        return Err(format!("{} {} failed", binary.display(), args.join(" ")));
    }
    Ok(text)
}

/// Reads a pipe to its end, keeping the first `MAX_PIPE` bytes and dropping the rest, so a long
/// output neither stops the process nor fills memory.
fn drain<R: std::io::Read + Send + 'static>(mut pipe: R) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut kept: Vec<u8> = Vec::new();
        let mut buffer = [0u8; 16 * 1024];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let room = MAX_PIPE.saturating_sub(kept.len());
                    kept.extend_from_slice(&buffer[..n.min(room)]);
                }
            }
        }
        String::from_utf8_lossy(&kept).into_owned()
    })
}

/// Runs one of the read-only diagnostics commands on the CLI at `binary` and returns its output,
/// trimmed to what a reader wants to see.
pub fn command(binary: &Path, kind: InfoCommand) -> Result<String, String> {
    let text = run(binary, kind.args())?;
    if text.len() <= MAX_OUTPUT {
        return Ok(text);
    }
    let mut end = MAX_OUTPUT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Ok(format!("{}\n…", &text[..end]))
}
