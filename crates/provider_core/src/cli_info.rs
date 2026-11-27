//! Read-only commands of an installed agent CLI, for the Settings panes.
//!
//! The panes ask each binary for its own diagnostics, so the output is what the CLI itself says
//! rather than a copy.

// Rust guideline compliant 2026-10-09

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use patchwork_platform::Error;

use crate::ModelOption;

/// A read-only command, the only kind the Settings panes run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoCommand {
    /// `doctor`: installation, config, auth and runtime health.
    Doctor,
    /// `mcp list`: the MCP servers the CLI has configured.
    McpList,
    /// `plugin list`: the plugins the CLI has installed.
    PluginList,
    /// `debug paths`: OpenCode's data, config, cache, state and log folders.
    Paths,
    /// `debug agents`: OpenCode's agents, as JSON.
    Agents,
    /// `changelog`: Antigravity's notes for its latest release.
    Changelog,
}

impl InfoCommand {
    /// The command a Settings pane names.
    ///
    /// Recognizes `doctor`, `mcp`, `plugins`, `paths`, `agents`, or `changelog`.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "doctor" => Some(InfoCommand::Doctor),
            "mcp" => Some(InfoCommand::McpList),
            "plugins" => Some(InfoCommand::PluginList),
            "paths" => Some(InfoCommand::Paths),
            "agents" => Some(InfoCommand::Agents),
            "changelog" => Some(InfoCommand::Changelog),
            _ => None,
        }
    }

    fn args(self) -> &'static [&'static str] {
        match self {
            InfoCommand::Doctor => &["doctor"],
            InfoCommand::McpList => &["mcp", "list"],
            InfoCommand::PluginList => &["plugin", "list"],
            InfoCommand::Paths => &["debug", "paths"],
            InfoCommand::Agents => &["debug", "agents"],
            InfoCommand::Changelog => &["changelog"],
        }
    }
}

/// Fails unless `kind` is one of the commands the CLI `name` has.
///
/// A CLI given a subcommand it lacks may take it as a prompt and start a turn, so each CLI names
/// the ones it has.
///
/// # Errors
///
/// Returns an error when `offered` does not contain `kind`.
pub fn check(name: &str, kind: InfoCommand, offered: &[InfoCommand]) -> Result<(), Error> {
    if offered.contains(&kind) {
        Ok(())
    } else {
        Err(format!("{name} has no `{}` command", kind.args().join(" ")).into())
    }
}

/// Longest a probe may take before it is killed. `doctor` is the slow one.
const TIMEOUT: Duration = Duration::from_secs(60);
/// Longest output kept from a command.
///
/// The model catalog is nearly 700 KB of JSON, so this is generous; it is a guard against a
/// runaway command, not a display limit.
const MAX_PIPE: usize = 8 * 1024 * 1024;
/// Longest output shown for a command whose output is for a reader, such as `doctor`.
const MAX_OUTPUT: usize = 60_000;

/// Runs the CLI at `binary` with `args` and returns its stdout, or stderr when stdout is empty.
///
/// Both pipes are read on threads of their own while the process runs so a large output cannot
/// stall the child.
///
/// # Errors
///
/// Returns an error when the program cannot be started, runs past the time limit, or exits with a
/// failure and prints nothing.
pub fn run(binary: &Path, args: &[&str]) -> Result<String, Error> {
    let mut cmd = patchwork_platform::host_command(binary);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = crate::command_size::spawn(&mut cmd, "an agent CLI")
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
                return Err(
                    format!("{} {} took too long", binary.display(), args.join(" ")).into(),
                );
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("Could not run {}: {e}", binary.display()).into()),
        }
    };
    let read = |pipe: Option<std::thread::JoinHandle<String>>| {
        let pipe = pipe?;
        let until = Instant::now() + Duration::from_secs(2);
        while !pipe.is_finished() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        pipe.is_finished().then(|| pipe.join().ok()).flatten()
    };
    let stdout = read(out).unwrap_or_default();
    let stderr = read(err).unwrap_or_default();
    let text = if stdout.trim().is_empty() {
        stderr
    } else {
        stdout
    };
    if !status.success() && text.trim().is_empty() {
        return Err(format!("{} {} failed", binary.display(), args.join(" ")).into());
    }
    Ok(text)
}

/// Reads a pipe to its end, keeping the first `MAX_PIPE` bytes and dropping the rest.
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

/// The version line the CLI at `binary` prints, e.g. `2.1.310 (Claude Code)`, or `unknown`.
#[must_use]
pub fn version(binary: &Path) -> String {
    run(binary, &["--version"]).map_or_else(
        |_| "unknown".into(),
        |text| text.lines().next().unwrap_or_default().trim().to_string(),
    )
}

/// Runs one of the read-only diagnostics commands on the CLI at `binary`.
///
/// # Errors
///
/// Returns an error when the command cannot be run, times out, or exits with a failure and prints
/// nothing.
pub fn command(binary: &Path, kind: InfoCommand) -> Result<String, Error> {
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

/// What a CLI's Details page offers, for a CLI Patchwork can read.
#[derive(Debug, Clone, Copy)]
pub struct CliPage {
    /// The binary's name, which titles a command's output.
    pub name: &'static str,
    /// Each command: its button, its output's title, and what it runs.
    pub commands: &'static [(&'static str, &'static str, InfoCommand)],
    pub rows: Option<CliRows>,
    /// The label of the chip that keeps only some rows.
    pub filter: Option<&'static str>,
    /// What the page says when no row is left.
    pub empty: &'static str,
}

/// What a CLI's rows are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliRows {
    /// Feature flags, each on or off.
    Flags,
    /// Settings from its config files.
    Settings,
}

/// The commands Claude Code and Codex share.
pub const SHARED_COMMANDS: &[(&str, &str, InfoCommand)] = &[
    ("Doctor", "doctor", InfoCommand::Doctor),
    ("MCP servers", "mcp list", InfoCommand::McpList),
    ("Plugins", "plugin list", InfoCommand::PluginList),
];

/// Each CLI's reading in one shape, so one view draws them all.
#[derive(Debug, Clone)]
pub struct CliReading {
    pub path: String,
    pub version: String,
    pub features: Vec<CliFeature>,
    pub models: Vec<CliModel>,
}

/// One flag or setting of a CLI.
#[derive(Debug, Clone)]
pub struct CliFeature {
    pub name: String,
    pub detail: String,
    pub enabled: Option<bool>,
    /// Whether the filter chip keeps it.
    pub kept: bool,
    /// The file that sets it.
    pub source: Option<String>,
}

/// One model in a CLI's catalog.
#[derive(Debug, Clone)]
pub struct CliModel {
    pub id: String,
    pub name: String,
    pub context_window: Option<u64>,
    pub efforts: Vec<String>,
    pub tag: Option<String>,
}

/// Models listed with no tag, as OpenCode and Antigravity list them.
#[must_use]
pub fn untagged_models(models: Vec<ModelOption>) -> Vec<CliModel> {
    models
        .into_iter()
        .map(|m| CliModel {
            id: m.id.unwrap_or_else(|| m.label.clone()),
            name: m.label,
            context_window: m.context_window,
            efforts: m.efforts.unwrap_or_default(),
            tag: None,
        })
        .collect()
}
