//! The SSH hosts in the user's `~/.ssh/config`, read only: Patchwork lists them and checks that one
//! answers, and never writes the config itself.

use std::io::Read;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::Serialize;
use ts_rs::TS;

use super::fs::home_dir;

/// A `Host` alias from `~/.ssh/config`, with what its block sets.
#[derive(Serialize, TS)]
#[ts(export, export_to = "projects.ts")]
pub struct SshHost {
    pub name: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
}

/// Whether a host let Patchwork in with a key, without asking anything.
#[derive(Serialize, TS)]
#[ts(export, export_to = "projects.ts")]
#[serde(rename_all = "camelCase")]
pub struct SshTest {
    pub ok: bool,
    /// How long the login took, when it worked.
    pub latency_ms: Option<u32>,
    /// What ssh said, when it did not.
    pub error: Option<String>,
}

/// How long a test may take before it is stopped, whatever stage it is in.
const TEST_LIMIT: Duration = Duration::from_secs(10);

pub fn config_path() -> Option<PathBuf> {
    home_dir().map(|home| home.join(".ssh").join("config"))
}

/// The `Host` aliases in `~/.ssh/config`, each with its `HostName`, `User` and `Port`. Wildcard
/// patterns (`*`, `?`) and negations are skipped, since they are not hosts to pick. `Include`d
/// files are not followed.
pub fn hosts() -> Vec<SshHost> {
    let Some(text) = config_path().and_then(|p| std::fs::read_to_string(p).ok()) else {
        return Vec::new();
    };
    let mut hosts: Vec<SshHost> = Vec::new();
    // Indices into `hosts` for the aliases of the Host block being read, so its options fill them.
    let mut current: Vec<usize> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Options are `Key value` or `Key=value`.
        let Some((key, value)) = line.split_once(|c: char| c.is_whitespace() || c == '=') else {
            continue;
        };
        let value = value
            .trim_start_matches(|c: char| c.is_whitespace() || c == '=')
            .trim();
        match key.to_ascii_lowercase().as_str() {
            "host" => {
                current.clear();
                for alias in value.split_whitespace() {
                    if alias.contains(['*', '?']) || alias.starts_with('!') {
                        continue;
                    }
                    current.push(hosts.len());
                    hosts.push(SshHost {
                        name: alias.to_string(),
                        hostname: None,
                        user: None,
                        port: None,
                    });
                }
            }
            // A `Match` block's options are conditional, so they are not shown as a host's.
            "match" => current.clear(),
            // ssh takes the first value it reads for an option, so a later one does not replace it.
            "hostname" => {
                for &i in &current {
                    hosts[i].hostname.get_or_insert_with(|| value.to_string());
                }
            }
            "user" => {
                for &i in &current {
                    hosts[i].user.get_or_insert_with(|| value.to_string());
                }
            }
            "port" => {
                for &i in &current {
                    if hosts[i].port.is_none() {
                        hosts[i].port = value.parse().ok();
                    }
                }
            }
            _ => {}
        }
    }
    hosts
}

/// Logs in to `name` and runs `true`, with keys only (`BatchMode`), so a host that would ask for a
/// password or to trust a new key fails instead of waiting. Only an alias from the config is
/// accepted, so the name can never be read as an ssh option.
pub fn test(name: &str) -> Result<SshTest, String> {
    if !hosts().iter().any(|h| h.name == name) {
        return Err(format!("{name} is not a host in ~/.ssh/config"));
    }
    let started = Instant::now();
    let mut child = super::host_command("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            "--",
            name,
            "true",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not run ssh: {e}"))?;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break Some(status);
        }
        if started.elapsed() > TEST_LIMIT {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let elapsed = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
    let Some(status) = status else {
        return Ok(SshTest {
            ok: false,
            latency_ms: None,
            error: Some(format!("No answer within {} seconds", TEST_LIMIT.as_secs())),
        });
    };
    if status.success() {
        return Ok(SshTest {
            ok: true,
            latency_ms: Some(elapsed),
            error: None,
        });
    }
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    // ssh's reason is its last line; the ones before are warnings and banners.
    let reason = stderr
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("ssh exited with {status}"));
    Ok(SshTest {
        ok: false,
        latency_ms: None,
        error: Some(reason),
    })
}
