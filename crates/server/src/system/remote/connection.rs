//! One SSH connection per host, shared by every command Patchwork runs there.
//!
//! The first command for a host starts a master connection (`ControlMaster`) that later commands
//! reuse, so each one costs a round trip instead of a login. The master also carries a tunnel
//! from the host back to Patchwork's relay, at the same port number, so the hooks and MCP proxies
//! an agent starts there reach this copy of Patchwork as they would on this machine.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// The relay's port on this machine, once it listens; 0 before.
static RELAY_PORT: AtomicU16 = AtomicU16::new(0);

/// The hosts with a master Patchwork checked lately, and whether it carries the relay tunnel.
static MASTERS: Mutex<Option<HashMap<String, Master>>> = Mutex::new(None);

/// Serializes starting masters, so two commands for a new host don't each log in.
static STARTING: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy)]
struct Master {
    checked: Instant,
    tunnel: bool,
}

/// How long a master is trusted to be up before it is checked again.
const RECHECK: Duration = Duration::from_secs(20);

/// How long a master with nothing running stays up after its last command.
const PERSIST: &str = "ControlPersist=15m";

/// Tells new connections which port the relay listens on, so they tunnel it back.
pub fn set_relay_port(port: u16) {
    RELAY_PORT.store(port, Ordering::Relaxed);
}

/// The folder the masters' sockets are in. The runtime folder keeps the path under the length a
/// socket's name may have.
fn socket_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map_or_else(|| crate::system::dirs().state.clone(), PathBuf::from)
        .join("patchwork-ssh")
}

/// The options every ssh Patchwork starts uses: keys only, and the shared connection.
fn options(cmd: &mut Command) {
    cmd.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10"]);
    if cfg!(unix) {
        let path = socket_dir().join("%C");
        cmd.arg("-o")
            .arg(format!("ControlPath={}", path.to_string_lossy()))
            .args(["-o", "ControlMaster=no"]);
    }
}

/// An `ssh` command for `host` that runs over its shared connection, with the connection up.
///
/// Arguments added after it are ssh's own until `--`; the caller adds `--`, the host and the
/// command to run there.
///
/// # Errors
///
/// Returns an error when the connection to `host` cannot be made.
pub fn ssh(host: &str) -> Result<Command, Error> {
    ensure(host)?;
    let mut cmd = crate::system::host_command("ssh");
    options(&mut cmd);
    // Without a shared connection the tunnel goes with each command instead.
    if !cfg!(unix) {
        tunnel_args(&mut cmd);
    }
    Ok(cmd)
}

/// An `sftp` command for `host` over its shared connection, with the connection up. The caller
/// adds sftp's own arguments and the host.
///
/// # Errors
///
/// Returns an error when the connection to `host` cannot be made.
pub(super) fn sftp(host: &str) -> Result<Command, Error> {
    ensure(host)?;
    let mut cmd = crate::system::host_command("sftp");
    options(&mut cmd);
    Ok(cmd)
}

/// Adds the relay tunnel to an ssh command, when the relay listens.
fn tunnel_args(cmd: &mut Command) -> bool {
    let port = RELAY_PORT.load(Ordering::Relaxed);
    if port == 0 {
        return false;
    }
    cmd.arg("-R").arg(format!("{port}:127.0.0.1:{port}"));
    true
}

/// Makes sure `host` has a master connection that carries the relay tunnel.
fn ensure(host: &str) -> Result<(), Error> {
    if !cfg!(unix) {
        return Ok(());
    }
    let relay = RELAY_PORT.load(Ordering::Relaxed) != 0;
    let known = masters(|m| m.get(host).copied());
    if let Some(master) = known
        && master.checked.elapsed() < RECHECK
        && (master.tunnel || !relay)
    {
        return Ok(());
    }
    let _starting = STARTING.lock().unwrap_or_else(PoisonError::into_inner);
    let mut tunnel = known.is_some_and(|m| m.tunnel);
    if control(host, "check") {
        if relay && !tunnel {
            tunnel = forward(host);
        }
    } else {
        tunnel = start(host)?;
    }
    masters(|m| {
        m.insert(
            host.to_string(),
            Master {
                checked: Instant::now(),
                tunnel,
            },
        )
    });
    Ok(())
}

fn masters<T>(f: impl FnOnce(&mut HashMap<String, Master>) -> T) -> T {
    let mut guard = MASTERS.lock().unwrap_or_else(PoisonError::into_inner);
    f(guard.get_or_insert_with(HashMap::new))
}

/// Sends a control request (`check`, `exit`) to `host`'s master; whether it answered.
fn control(host: &str, request: &str) -> bool {
    let mut cmd = crate::system::host_command("ssh");
    options(&mut cmd);
    cmd.args(["-O", request, "--", host])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Adds the relay tunnel to a master started before the relay listened.
fn forward(host: &str) -> bool {
    let mut cmd = crate::system::host_command("ssh");
    options(&mut cmd);
    cmd.args(["-O", "forward"]);
    if !tunnel_args(&mut cmd) {
        return false;
    }
    cmd.args(["--", host])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Starts `host`'s master in the background; whether it carries the relay tunnel.
fn start(host: &str) -> Result<bool, Error> {
    let dir = socket_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    restrict(&dir);
    // The master stays in the background holding whatever it was given for output, so what ssh
    // says goes to a file rather than a pipe that would never close.
    let log = dir.join(format!("{}.log", sanitize(host)));
    let file =
        std::fs::File::create(&log).map_err(|e| format!("Cannot write {}: {e}", log.display()))?;
    let mut cmd = crate::system::host_command("ssh");
    cmd.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10"])
        .arg("-o")
        .arg(format!("ControlPath={}", dir.join("%C").to_string_lossy()))
        .args(["-o", "ControlMaster=yes", "-o", PERSIST])
        .args([
            "-o",
            "ServerAliveInterval=30",
            "-o",
            "ExitOnForwardFailure=no",
        ]);
    let tunnel = tunnel_args(&mut cmd);
    cmd.args(["-M", "-N", "-f", "--", host])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(file);
    let status = cmd
        .status()
        .map_err(|e| format!("Could not run ssh: {e}"))?;
    if status.success() {
        return Ok(tunnel);
    }
    let said = std::fs::read_to_string(&log).unwrap_or_default();
    // ssh's reason is its last line; the ones before are warnings and banners.
    let reason = said
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .map_or_else(|| format!("ssh exited with {status}"), str::to_string);
    Err(format!("Could not connect to {host}: {reason}").into())
}

/// Ends every master Patchwork started, so none outlives the app by its persist time.
pub fn close_all() {
    let hosts: Vec<String> = masters(|m| m.drain().map(|(h, _)| h).collect());
    for host in hosts {
        control(&host, "exit");
    }
}

/// `host` as a file name.
fn sanitize(host: &str) -> String {
    host.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Keeps the sockets' folder to its owner, since a socket there logs in as the user.
fn restrict(dir: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    let _ = dir;
}
