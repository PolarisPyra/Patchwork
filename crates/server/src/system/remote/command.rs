//! Running a program in a folder on an SSH host.
//!
//! Callers build a [`Command`] as they would for a local folder, with the project root as its
//! working directory, and pass it through [`prepare`] or [`supervised`] before they set its input
//! and output. For a local root these hand the command back unchanged; for a remote one they
//! return an `ssh` command that runs the same program, arguments and environment on the host.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::ffi::OsStr;
use std::io::Write;
use std::process::{Command, Output, Stdio};

use serde::{Deserialize, Serialize};

use super::connection::ssh;
use super::host::{Os, probe};
use super::location::{Location, quote, split};

/// The environment variables a remote command is given from its local one. The rest, such as
/// PATH and HOME, describe this machine; the host's own login sets them there.
const PREFIXES: [&str; 10] = [
    "PATCHWORK_",
    "CLAUDE_",
    "CODEX_",
    "OPENCODE_",
    "ANTHROPIC_",
    "OPENAI_",
    "GIT_",
    "GEMINI_",
    "MCP_",
    "GH_",
];
const NAMES: [&str; 7] = [
    "CI",
    "NO_COLOR",
    "TERM",
    "FORCE_COLOR",
    "CLICOLOR",
    "COLORTERM",
    "DISABLE_AUTOUPDATER",
];

/// Whether the local value of `key` goes along to a remote command.
fn forwarded(key: &str) -> bool {
    NAMES.contains(&key) || PREFIXES.iter().any(|p| key.starts_with(p))
}

/// What a supervised command runs on the host, as Patchwork's helper there reads it.
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Spec {
    pub cwd: String,
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// `line` run by `sh`, whatever the user's login shell on the host is, so fish and others read
/// it the same.
pub(super) fn posix(line: &str) -> String {
    format!("exec sh -c {}", quote(line))
}

/// The host and folder `cmd` runs in, when its working directory is a remote root.
fn location(cmd: &Command) -> Option<(String, String)> {
    let dir = cmd.get_current_dir()?.to_str()?;
    split(dir).map(|Location { host, path }| (host.to_string(), path.to_string()))
}

fn text(s: &OsStr) -> String {
    s.to_string_lossy().into_owned()
}

/// The program's name to run on the host: a local absolute path means nothing there, so the host
/// finds the program by its name instead, without the `.exe` a Windows path ends in.
fn program_name(cmd: &Command) -> String {
    let program = std::path::Path::new(cmd.get_program());
    if program.is_absolute() {
        let exe = program
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"));
        if exe {
            program.file_stem()
        } else {
            program.file_name()
        }
        .map_or_else(|| text(cmd.get_program()), text)
    } else {
        text(cmd.get_program())
    }
}

/// `cmd`'s environment that goes along to the host.
fn env_of(cmd: &Command) -> Vec<(String, String)> {
    cmd.get_envs()
        .filter_map(|(k, v)| Some((text(k), text(v?))))
        .filter(|(k, _)| forwarded(k))
        .collect()
}

/// An `ssh` command that opens the user's login shell on `host` in the folder `path`, on a
/// terminal of its own, for an in-app terminal.
///
/// # Errors
///
/// Returns an error when the connection to the host cannot be made.
pub fn interactive(host: &str, path: &str) -> Result<Command, Error> {
    let line = if probe(host)?.os == Os::Windows {
        // The helper starts PowerShell there, or cmd, in the folder.
        super::upload::helper(host)?.line(&[super::serve::SHELL_ARG, &hex(path.as_bytes())])
    } else {
        posix(&format!(
            "cd {} 2>/dev/null; exec \"${{SHELL:-sh}}\" -l",
            quote(path)
        ))
    };
    let mut remote = ssh(host)?;
    remote.args(["-tt", "--", host, &line]);
    Ok(remote)
}

/// A command that runs the shell line `line` in the folder `root`, local or remote, before
/// [`supervised`] moves it to its host. None when no local shell is found.
#[must_use]
pub fn shell_in(root: &str, line: &str) -> Option<Command> {
    let mut cmd = if let Some(at) = split(root) {
        // The helper on the host gives `sh` the login environment the user's shell would.
        let windows = probe(at.host).is_ok_and(|h| h.os == Os::Windows);
        let mut cmd = Command::new(if windows { "cmd" } else { "sh" });
        if windows {
            cmd.args(["/d", "/s", "/c", line]);
        } else {
            cmd.args(["-c", line]);
        }
        cmd
    } else {
        crate::system::shell_command(line)?
    };
    cmd.current_dir(root);
    Some(cmd)
}

/// `bytes` as hex, for a command line that every shell passes through unchanged.
pub(super) fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = std::fmt::Write::write_fmt(&mut s, format_args!("{b:02x}"));
            s
        })
}

/// The longest hex spec put on a Windows host's command line, well under the 8191 characters
/// `cmd` takes.
const MOST_INLINE: usize = 6000;

/// `cmd` as an `ssh` command that runs it on its host, or `cmd` itself for a local folder.
///
/// For short commands such as `git`: the program is found on the PATH a non-interactive login
/// to the host gives. Set the command's input and output after this, since they are not carried
/// over.
///
/// # Errors
///
/// Returns an error when the connection to the host cannot be made.
pub fn prepare(cmd: Command) -> Result<Command, Error> {
    let Some((host, path)) = location(&cmd) else {
        return Ok(cmd);
    };
    let line = if probe(&host)?.os == Os::Windows {
        // No shell line means the same in cmd and PowerShell, so the helper runs the command.
        let spec = Spec {
            cwd: path,
            program: program_name(&cmd),
            args: cmd.get_args().map(text).collect(),
            env: env_of(&cmd),
        };
        let body = serde_json::to_vec(&spec).map_err(|e| e.to_string())?;
        let inline = hex(&body);
        let installed = super::upload::helper(&host)?;
        if inline.len() <= MOST_INLINE {
            installed.line(&[super::serve::RUN_ARG, &inline])
        } else {
            let id = stage(&host, spec)?;
            installed.line(&[super::serve::EXEC_ARG, &id])
        }
    } else {
        let mut line = format!("cd {} && exec", quote(&path));
        let env = env_of(&cmd);
        if !env.is_empty() {
            line.push_str(" env");
            for (k, v) in &env {
                line.push(' ');
                line.push_str(&quote(&format!("{k}={v}")));
            }
        }
        line.push(' ');
        line.push_str(&quote(&program_name(&cmd)));
        for arg in cmd.get_args() {
            line.push(' ');
            line.push_str(&quote(&text(arg)));
        }
        posix(&line)
    };
    let mut remote = ssh(&host)?;
    remote.args(["-T", "--", &host, &line]);
    Ok(remote)
}

/// Keeps `spec` in a private file on `host` and returns its name, so what it carries never
/// shows on a command line anyone there could read.
fn stage(host: &str, spec: Spec) -> Result<String, Error> {
    super::op::call(host, &super::op::Request::Stage { spec })
}

/// `cmd` as an `ssh` command that runs it on its host under Patchwork's helper, or `cmd` itself
/// for a local folder.
///
/// For agents and other long runs: the helper gives the program the host's login environment,
/// finds it where the user's shell would, and stops it when the connection ends, so stopping the
/// returned command stops the program on the host. Patchwork's own files the command names, and
/// Patchwork's binary, are copied to the host and named by their copies there.
///
/// # Errors
///
/// Returns an error when the connection to the host cannot be made, or the helper or the files
/// cannot be copied to it.
pub fn supervised(cmd: Command) -> Result<Command, Error> {
    let Some((host, path)) = location(&cmd) else {
        return Ok(cmd);
    };
    let mut mirror = super::upload::Mirror::new(&host)?;
    let installed = mirror.helper().clone();
    let exe = std::env::current_exe().ok();
    let program = if exe.as_deref() == Some(std::path::Path::new(cmd.get_program())) {
        installed.path.clone()
    } else {
        program_name(&cmd)
    };
    let args = cmd
        .get_args()
        .map(|a| mirror.rewrite(&text(a)))
        .collect::<Result<Vec<_>, _>>()?;
    let env = env_of(&cmd)
        .into_iter()
        .map(|(k, v)| Ok((k, mirror.rewrite(&v)?)))
        .collect::<Result<Vec<_>, Error>>()?;
    mirror.send()?;
    let id = stage(
        &host,
        Spec {
            cwd: path,
            program,
            args,
            env,
        },
    )?;
    let mut remote = ssh(&host)?;
    remote.args([
        "-T",
        "--",
        &host,
        &installed.line(&[super::serve::EXEC_ARG, &id]),
    ]);
    Ok(remote)
}

/// Runs the command line `line` in `host`'s own shell, with `input` on its stdin, and waits for
/// it.
///
/// # Errors
///
/// Returns an error when the connection to `host` cannot be made or ssh cannot be started.
pub(super) fn send(host: &str, line: &str, input: Option<&[u8]>) -> Result<Output, Error> {
    let mut cmd = ssh(host)?;
    cmd.args(["-T", "--", host, line])
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("Could not run ssh: {e}"))?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        let bytes = input.to_vec();
        // Written from its own thread, so a command that answers before it has read all of its
        // input cannot leave both sides waiting on a full pipe.
        std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        });
    }
    Ok(child.wait_with_output()?)
}
