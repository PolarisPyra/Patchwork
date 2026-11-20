//! One SSH connection per host, shared by every command Patchwork runs there.
//!
//! The first command for a host starts a master connection (`ControlMaster`) that later commands
//! reuse, so each one costs a round trip instead of a login. The master also carries a tunnel
//! from the host back to Patchwork's relay, at the same port number, so the hooks and MCP proxies
//! an agent starts there reach this copy of Patchwork as they would on this machine.
//!
//! Patchwork runs the system's `ssh` and `sftp`, so keys, the agent, `known_hosts` and the
//! ciphers are OpenSSH's, set by the user's own configuration. Patchwork adds only what
//! [`options`] lists and never an option that loosens a check: no `StrictHostKeyChecking`, no
//! `UserKnownHostsFile`, no agent forwarding. `BatchMode` makes a host whose key is unknown or
//! changed fail instead of asking, since nobody is there to answer.
//!
//! A master's socket logs in as the user, so the sockets' folder is the user's alone, and no
//! connection is made when it cannot be kept so.

// Rust guideline compliant 2026-10-09

use crate::Locked;
use crate::Error;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

/// The relay's port on this machine, once it listens; 0 before.
static RELAY_PORT: AtomicU16 = AtomicU16::new(0);

/// The hosts with a master Patchwork checked lately, and whether it carries the relay tunnel.
static MASTERS: LazyLock<Mutex<HashMap<String, Master>>> = LazyLock::new(Mutex::default);

/// One lock per host, held while its master starts, so two commands for a new host don't each
/// log in and a host that is slow to answer holds up only its own commands.
static STARTING: LazyLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = LazyLock::new(Mutex::default);

/// The hosts that could not be reached lately, with when and why. Until [`RECHECK`] passes,
/// commands for one fail with that reason at once instead of each waiting out a login.
static UNREACHABLE: LazyLock<Mutex<HashMap<String, (Instant, String)>>> =
    LazyLock::new(Mutex::default);

#[derive(Clone, Copy)]
struct Master {
    checked: Instant,
    tunnel: bool,
    /// Whether this master was asked for the tunnel since it was last checked, so one whose
    /// host refuses it is not asked again by every command.
    asked: bool,
    /// The master's process, as it named itself when last checked.
    pid: Option<u32>,
    /// Whether this copy of Patchwork started it, rather than found it running.
    started_here: bool,
}

/// How long a master is trusted to be up before it is checked again.
const RECHECK: Duration = Duration::from_secs(20);

/// How long a master with nothing running stays up after its last command.
const PERSIST: &str = "ControlPersist=15m";

/// Keys only, so ssh fails instead of asking for a password or about a host key.
const BATCH: &str = "BatchMode=yes";

/// How long a login may take, in seconds.
const CONNECT_TIMEOUT: &str = "ConnectTimeout=10";

/// How often a master asks a quiet host whether it is still there, in seconds. ssh ends the
/// connection after three unanswered, which ends the commands waiting on it.
const KEEP_ALIVE: &str = "ServerAliveInterval=10";

/// Tells new connections which port the relay listens on, so they tunnel it back.
pub fn set_relay_port(port: u16) {
    RELAY_PORT.store(port, Ordering::Relaxed);
}

/// The folder the masters' sockets are in.
///
/// A socket's path may be about 100 characters, and ssh's own part of it takes 57, so the
/// folder is the runtime folder, or one in `~/.ssh` on a machine without it, such as a Mac. The
/// state folder is too deep for either. It is named for this build, so a debug build and the
/// installed app each keep their own masters and neither ends the other's.
fn socket_dir() -> PathBuf {
    let name = crate::dirs::NAME;
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime).join(format!("{name}-ssh"));
    }
    crate::fs::home_dir().map_or_else(
        || crate::dirs().state.join("patchwork-ssh"),
        |home| home.join(".ssh").join(name),
    )
}

/// Makes the sockets' folder, the user's alone, and returns it.
fn private_socket_dir() -> Result<PathBuf, Error> {
    let dir = socket_dir();
    // The folder goes into `ControlPath` in quotes, which a quote in it would end.
    if dir.to_string_lossy().contains('"') {
        return Err(format!("Cannot keep SSH connections in {}", dir.display()).into());
    }
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder
        .create(&dir)
        .map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    // A folder that was there already keeps its own mode. Only its owner may change it, so
    // this also turns down a folder someone else made.
    #[cfg(unix)]
    std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))
        .map_err(|e| format!("Cannot keep {} private: {e}", dir.display()))?;
    Ok(dir)
}

/// The options every ssh Patchwork starts uses: keys only, and the shared connection, which
/// `master` says this command is to start rather than join.
fn options(cmd: &mut Command, master: bool) {
    cmd.args(["-o", BATCH, "-o", CONNECT_TIMEOUT]);
    if cfg!(unix) {
        // In quotes, since ssh splits an option's value at a space, and with each `%` doubled,
        // since ssh reads one as the start of a token such as `%C`.
        let dir = socket_dir().to_string_lossy().replace('%', "%%");
        cmd.arg("-o")
            .arg(format!("ControlPath=\"{dir}/%C\""))
            .arg("-o")
            .arg(if master {
                "ControlMaster=yes"
            } else {
                "ControlMaster=no"
            });
    }
}

/// Turns down a host name ssh could read as something other than a host.
///
/// Every command names the host after `--`, so ssh never takes it for an option; this keeps a
/// name that is plainly not one from reaching ssh at all.
fn check_host(host: &str) -> Result<(), Error> {
    let plain = !host.is_empty()
        && !host.starts_with('-')
        && !host.chars().any(|c| c.is_whitespace() || c.is_control());
    if plain {
        Ok(())
    } else {
        Err(format!("{host:?} is not an SSH host's name").into())
    }
}

/// Why ssh or sftp failed, from what it wrote to stderr: its last line, since the ones before
/// are banners and the warnings OpenSSH marks with `**`.
pub(super) fn failure_reason(stderr: &str) -> Option<&str> {
    stderr
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty() && !line.starts_with("**"))
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
    let mut cmd = crate::host_command("ssh");
    options(&mut cmd, false);
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
    let mut cmd = crate::host_command("sftp");
    options(&mut cmd, false);
    Ok(cmd)
}

/// Checks that `host`'s end of the relay tunnel is Patchwork's own.
///
/// The relay's port number is the same on the host. When another program there holds it, the
/// hooks and proxies of a turn would hand that program the relay's token and take its answers.
/// On Windows the tunnel goes with each command, and this cannot tell whether the host gave it.
///
/// # Errors
///
/// Returns an error when the host cannot be reached, or its side of the port is taken.
pub fn relay_reaches(host: &str) -> Result<(), Error> {
    ensure(host)?;
    let port = RELAY_PORT.load(Ordering::Relaxed);
    if !cfg!(unix) || port == 0 || masters(|m| m.get(host).is_some_and(|m| m.tunnel)) {
        return Ok(());
    }
    Err(format!(
        "{host} did not give Patchwork port {port}, so its permission hook cannot reach this \
         machine from there. The port is in use on {host}, or {host} does not allow port \
         forwarding. Restarting Patchwork picks another port."
    )
    .into())
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
    check_host(host)?;
    if !cfg!(unix) {
        return Ok(());
    }
    let relay = RELAY_PORT.load(Ordering::Relaxed) != 0;
    let known = masters(|m| m.get(host).copied());
    if let Some(master) = known
        && master.checked.elapsed() < RECHECK
        && (master.tunnel || master.asked || !relay)
    {
        return Ok(());
    }
    recent_failure(host)?;
    let lock = Arc::clone(STARTING.locked().entry(host.to_string()).or_default());
    let _starting = lock.locked();
    // Another command may have started this host's master while this one waited.
    if let Some(master) = masters(|m| m.get(host).copied())
        && master.checked.elapsed() < RECHECK
        && (master.tunnel || master.asked || !relay)
    {
        return Ok(());
    }
    // Another command may have tried this host while this one waited.
    recent_failure(host)?;
    // What this copy knows holds only for the master it last saw: one that ended and was
    // started anew by another copy of Patchwork is that copy's, with that copy's tunnel.
    let (running, pid) = check(host);
    let same = masters(|m| m.get(host).copied()).filter(|m| running && m.pid == pid);
    let mut tunnel = same.is_some_and(|m| m.tunnel);
    let mut started_here = same.is_some_and(|m| m.started_here);
    let mut pid = pid;
    if running {
        if relay && !tunnel {
            tunnel = forward(host);
        }
    } else {
        start(host).inspect_err(|e| {
            UNREACHABLE
                .locked()
                .insert(host.to_string(), (Instant::now(), e.to_string()));
        })?;
        started_here = true;
        pid = check(host).1;
        tunnel = relay && forward(host);
    }
    UNREACHABLE.locked().remove(host);
    masters(|m| {
        m.insert(
            host.to_string(),
            Master {
                checked: Instant::now(),
                tunnel,
                asked: relay,
                pid,
                started_here,
            },
        )
    });
    Ok(())
}

/// The reason `host` could not be reached, when that was less than [`RECHECK`] ago.
fn recent_failure(host: &str) -> Result<(), Error> {
    match UNREACHABLE.locked().get(host) {
        Some((when, reason)) if when.elapsed() < RECHECK => Err(reason.clone().into()),
        _ => Ok(()),
    }
}

fn masters<T>(f: impl FnOnce(&mut HashMap<String, Master>) -> T) -> T {
    f(&mut MASTERS.locked())
}

/// Whether `host`'s master answers, and the process it says it is (`Master running (pid=N)`).
fn check(host: &str) -> (bool, Option<u32>) {
    let mut cmd = crate::host_command("ssh");
    options(&mut cmd, false);
    let Ok(out) = cmd
        .args(["-O", "check", "--", host])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
    else {
        return (false, None);
    };
    let said = String::from_utf8_lossy(&out.stderr);
    let pid = said.split_once("pid=").and_then(|(_, rest)| {
        let digits = rest.split(|c: char| !c.is_ascii_digit()).next()?;
        digits.parse().ok()
    });
    (out.status.success(), pid)
}

/// Asks `host`'s master to end.
fn exit(host: &str) {
    let mut cmd = crate::host_command("ssh");
    options(&mut cmd, false);
    let ended = cmd
        .args(["-O", "exit", "--", host])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if let Err(e) = ended {
        log::warn!(host:% = host, error:% = e; "could not end the connection to {{host}}: {{error}}");
    }
}

/// Adds the relay tunnel to `host`'s master; whether the host gave it the port.
///
/// The master is asked for the tunnel on its own, never at its start, because only this request
/// says whether the host's side of the port was free.
fn forward(host: &str) -> bool {
    let mut cmd = crate::host_command("ssh");
    options(&mut cmd, false);
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

/// Starts `host`'s master in the background, without the relay tunnel.
fn start(host: &str) -> Result<(), Error> {
    let dir = private_socket_dir()?;
    // The master stays in the background holding whatever it was given for output, so what ssh
    // says goes to a file rather than a pipe that would never close.
    let log = dir.join(format!("{}.log", sanitize(host)));
    let file =
        std::fs::File::create(&log).map_err(|e| format!("Cannot write {}: {e}", log.display()))?;
    let mut cmd = crate::host_command("ssh");
    options(&mut cmd, true);
    cmd.args(["-o", PERSIST, "-o", KEEP_ALIVE]);
    // No `-M` here: with `ControlMaster=yes` it asks ssh to confirm each command that joins the
    // connection, and with nobody to ask, the master turns them all away.
    cmd.args(["-N", "-f", "--", host])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(file);
    let status = cmd
        .status()
        .map_err(|e| format!("Could not run ssh: {e}"))?;
    if status.success() {
        return Ok(());
    }
    let said = std::fs::read_to_string(&log).unwrap_or_default();
    let reason =
        failure_reason(&said).map_or_else(|| format!("ssh exited with {status}"), str::to_string);
    Err(format!("Could not connect to {host}: {reason}").into())
}

/// Ends every master this copy of Patchwork started, so none outlives the app by its persist
/// time. A master it found running belongs to another copy, whose commands it still carries.
pub fn close_all() {
    let started: Vec<String> = masters(|m| {
        m.drain()
            .filter(|(_, master)| master.started_here)
            .map(|(host, _)| host)
            .collect()
    });
    for host in started {
        exit(&host);
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
