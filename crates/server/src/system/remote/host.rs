//! What kind of machine an SSH host is: its system, its processor and its home folder.
//!
//! A Linux or macOS host answers `uname`; a Windows host, whose OpenSSH server runs commands in
//! `cmd` or PowerShell, answers with its environment variables instead. The answer is kept for
//! the rest of the session.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Mutex, PoisonError};

use super::connection::ssh;

/// A host's operating system, as far as Patchwork's helper cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Mac,
    Windows,
}

/// What Patchwork learned about an SSH host when it first reached it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Host {
    pub os: Os,
    /// `x86_64` or `aarch64`.
    pub arch: &'static str,
    /// The home folder, with forward slashes on Windows: `C:/Users/me`.
    pub home: String,
}

/// What `host` is, asked once per session.
///
/// # Errors
///
/// Returns an error when the host cannot be reached, or runs a system or processor Patchwork has
/// no helper for.
pub fn probe(host: &str) -> Result<Host, Error> {
    static HOSTS: Mutex<Option<HashMap<String, Host>>> = Mutex::new(None);
    if let Some(known) = HOSTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(HashMap::new)
        .get(host)
    {
        return Ok(known.clone());
    }
    let found = ask(host)?;
    HOSTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(HashMap::new)
        .insert(host.to_string(), found.clone());
    Ok(found)
}

/// The home folder on `host`.
///
/// # Errors
///
/// Returns an error when the host cannot be reached.
pub fn home(host: &str) -> Result<String, Error> {
    probe(host).map(|h| h.home)
}

fn ask(host: &str) -> Result<Host, Error> {
    // Each shell reads this line: a POSIX shell or fish answers, and cmd or PowerShell report
    // that `uname` is unknown and print nothing useful.
    let said = say(host, "uname -sm; echo \"$HOME\"")?;
    if let Some(found) = unix(&said) {
        return Ok(found);
    }
    let line = "echo %OS% %PROCESSOR_ARCHITECTURE% %USERPROFILE%";
    let mut said = say(host, line)?;
    // PowerShell prints the line as it is; cmd expands it.
    if said.trim_start().starts_with("%OS%") {
        said = say(host, &format!("cmd /d /c {line}"))?;
    }
    windows(&said).ok_or_else(|| {
        let what = said.lines().next().unwrap_or_default().trim();
        if what.is_empty() {
            format!("{host} did not say what system it runs").into()
        } else {
            format!("{host} runs {what}, which Patchwork has no helper for").into()
        }
    })
}

/// What `line`, run in `host`'s own shell, prints.
fn say(host: &str, line: &str) -> Result<String, Error> {
    let out = ssh(host)?
        .args(["-T", "--", host, line])
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("Could not run ssh: {e}"))?;
    // 255 is ssh's own failure; anything else is the shell's answer, read whatever it was.
    if out.status.code() == Some(255) {
        let said = String::from_utf8_lossy(&out.stderr);
        let reason = said
            .lines()
            .map(str::trim)
            .rfind(|l| !l.is_empty() && !l.starts_with("**"))
            .unwrap_or("ssh failed");
        return Err(format!("Could not connect to {host}: {reason}").into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A Linux or macOS host from `uname -sm` and `$HOME`.
fn unix(said: &str) -> Option<Host> {
    let mut lines = said.lines();
    let mut words = lines.next()?.split_whitespace();
    let (system, machine) = (words.next()?, words.next()?);
    let os = match system {
        "Linux" => Os::Linux,
        "Darwin" => Os::Mac,
        _ => return None,
    };
    let home = lines.next()?.trim().to_string();
    home.starts_with('/').then_some(Host {
        os,
        arch: arch(machine)?,
        home,
    })
}

/// A Windows host from `%OS% %PROCESSOR_ARCHITECTURE% %USERPROFILE%`.
fn windows(said: &str) -> Option<Host> {
    let line = said.lines().next()?.trim().trim_end_matches('"').trim();
    let mut words = line.splitn(3, ' ');
    if words.next()? != "Windows_NT" {
        return None;
    }
    // Windows on ARM runs the x86_64 helper through its emulation.
    words.next().and_then(arch)?;
    let home = words.next()?.trim().replace('\\', "/");
    super::location::has_drive(&home).then_some(Host {
        os: Os::Windows,
        arch: "x86_64",
        home,
    })
}

fn arch(machine: &str) -> Option<&'static str> {
    match machine.to_ascii_lowercase().as_str() {
        "x86_64" | "amd64" => Some("x86_64"),
        "aarch64" | "arm64" => Some("aarch64"),
        _ => None,
    }
}
