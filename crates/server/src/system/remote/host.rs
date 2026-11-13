//! What kind of machine an SSH host is: its system, its processor and its home folder.
//!
//! A Linux or macOS host answers `uname`; a Windows host, whose OpenSSH server runs commands in
//! `cmd` or PowerShell, answers with its environment variables instead.
//!
//! The answers are kept in a file between runs, so a host Patchwork has met costs no questions
//! the next time. [`forget`] drops a host whose answers turned out stale.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};

use super::connection::ssh;
use crate::system::device::Kind;

/// What Patchwork learned about each host, read from [`file`] on first use.
static KNOWN: Mutex<Option<HashMap<String, Known>>> = Mutex::new(None);

/// A host's operating system, as far as Patchwork's helper cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

/// A host as [`file`] keeps it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Known {
    os: Os,
    arch: String,
    home: String,
    /// Whether it is a desktop or a laptop, once it said.
    #[serde(default)]
    kind: Option<Kind>,
    /// The file name of the helper last seen installed there.
    #[serde(default)]
    helper: Option<String>,
}

/// The file the hosts are remembered in between runs. Deleting it only makes Patchwork ask
/// each host again.
fn file() -> PathBuf {
    crate::system::dirs().cache.join("remote-hosts.json")
}

/// Runs `f` on the remembered hosts, and writes them to [`file`] when it returns true.
fn known<T>(f: impl FnOnce(&mut HashMap<String, Known>) -> (T, bool)) -> T {
    let mut guard = KNOWN.lock().unwrap_or_else(PoisonError::into_inner);
    let hosts = guard.get_or_insert_with(|| {
        std::fs::read(file())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    });
    let (out, changed) = f(hosts);
    if changed {
        let path = file();
        let part = path.with_extension("json.part");
        let written = serde_json::to_vec_pretty(hosts)
            .map_err(std::io::Error::other)
            .and_then(|body| {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                std::fs::write(&part, body)?;
                std::fs::rename(&part, &path)
            });
        if let Err(e) = written {
            log::warn!(error:% = e; "could not remember the SSH hosts: {{error}}");
        }
    }
    out
}

/// What `host` is, asked the first time Patchwork reaches it and remembered after.
///
/// # Errors
///
/// Returns an error when the host cannot be reached, or runs a system or processor Patchwork has
/// no helper for.
pub fn probe(host: &str) -> Result<Host, Error> {
    let remembered = known(|hosts| {
        let found = hosts.get(host).and_then(|k| {
            Some(Host {
                os: k.os,
                arch: arch(&k.arch)?,
                home: k.home.clone(),
            })
        });
        (found, false)
    });
    if let Some(found) = remembered {
        return Ok(found);
    }
    let found = ask(host)?;
    known(|hosts| {
        hosts.insert(
            host.to_string(),
            Known {
                os: found.os,
                arch: found.arch.to_string(),
                home: found.home.clone(),
                kind: None,
                helper: None,
            },
        );
        ((), true)
    });
    Ok(found)
}

/// Forgets what `host` said, so it is asked again: for a host that changed since.
pub(super) fn forget(host: &str) {
    known(|hosts| ((), hosts.remove(host).is_some()));
}

/// The file name of the helper last seen installed on `host`.
pub(super) fn helper_name(host: &str) -> Option<String> {
    known(|hosts| (hosts.get(host).and_then(|k| k.helper.clone()), false))
}

/// Remembers that the helper named `name` is installed on `host`.
pub(super) fn set_helper(host: &str, name: &str) {
    known(|hosts| {
        let Some(k) = hosts.get_mut(host) else {
            return ((), false);
        };
        let changed = k.helper.as_deref() != Some(name);
        k.helper = Some(name.to_string());
        ((), changed)
    });
}

/// The home folder on `host`.
///
/// # Errors
///
/// Returns an error when the host cannot be reached.
pub fn home(host: &str) -> Result<String, Error> {
    probe(host).map(|h| h.home)
}

/// Whether `host` is a desktop or a laptop, read from what its system reports about its case and
/// battery the first time, and remembered after.
///
/// # Errors
///
/// Returns an error when the host cannot be reached.
pub fn kind(host: &str) -> Result<Kind, Error> {
    if let Some(kind) = known(|hosts| (hosts.get(host).and_then(|k| k.kind), false)) {
        return Ok(kind);
    }
    let kind = ask_kind(host)?;
    known(|hosts| {
        let Some(k) = hosts.get_mut(host) else {
            return ((), false);
        };
        k.kind = Some(kind);
        ((), true)
    });
    Ok(kind)
}

fn ask_kind(host: &str) -> Result<Kind, Error> {
    use crate::system::device;
    Ok(match probe(host)?.os {
        Os::Linux => device::from_linux(&say(
            host,
            &super::command::posix(
                "cat /sys/class/dmi/id/chassis_type 2>/dev/null || echo; ls /sys/class/power_supply 2>/dev/null",
            ),
        )?),
        Os::Mac => device::from_pmset(&say(host, "pmset -g batt")?),
        // Read the same from PowerShell or cmd, whichever the OpenSSH server starts.
        Os::Windows => device::from_chassis(&say(
            host,
            &format!(
                "powershell -NoProfile -NonInteractive -Command \"{}\"",
                device::WINDOWS_QUERY
            ),
        )?),
    })
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
