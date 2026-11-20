//! Copying Patchwork's helper to an SSH host.
//!
//! Patchwork's binary, which hooks and proxies run, becomes the helper built for the host: this
//! copy's own when it has one, else the release's.

// Rust guideline compliant 2026-10-09

use crate::Locked;
use crate::Error;
use std::collections::HashMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Output, Stdio};
use std::sync::{LazyLock, Mutex, OnceLock};

use sha2::{Digest, Sha256};

use super::command::{posix, send};
use super::host::{Host, Os, probe};
use super::location::quote;

/// The helper's name, beside Patchwork's binary, with the host's system and processor after it.
const HELPER: &str = "patchwork-remote";

/// The helper on each host it was copied to.
static HELPERS: LazyLock<Mutex<HashMap<String, Installed>>> = LazyLock::new(Mutex::default);

/// Where a helper this copy of Patchwork does not ship comes from, by its file name.
type HelperSource = fn(&str) -> Result<Vec<u8>, Error>;

/// The source the app set, which downloads the helper from this version's release.
static HELPER_SOURCE: OnceLock<HelperSource> = OnceLock::new();

/// Tells uploads where to get a helper built for another system or processor.
///
/// Only the first call counts.
pub fn set_helper_source(source: HelperSource) {
    let _ = HELPER_SOURCE.set(source);
}

/// The helper on a host.
#[derive(Debug, Clone)]
pub(super) struct Installed {
    /// Its full path there, with forward slashes.
    pub path: String,
    /// Its file name, in the `bin` folder under [`base`].
    name: String,
    os: Os,
    /// Whether it is there on the word of an earlier run, not seen by this one.
    pub remembered: bool,
}

impl Installed {
    /// The command line that runs the helper with `args`, which are plain words, in the host's
    /// own shell.
    pub(super) fn line(&self, args: &[&str]) -> String {
        if self.os == Os::Windows {
            // cmd and PowerShell both run a program named relative to the folder they start in,
            // the home folder, and neither needs it quoted.
            format!(
                r".\.cache\patchwork-remote\bin\{} {}",
                self.name,
                args.join(" ")
            )
        } else {
            let mut line = format!("exec {}", quote(&self.path));
            for arg in args {
                line.push(' ');
                line.push_str(&quote(arg));
            }
            posix(&line)
        }
    }
}

/// The folder Patchwork keeps its files in on `host`.
///
/// # Errors
///
/// Returns an error when the host cannot be reached.
pub(super) fn base(host: &str) -> Result<String, Error> {
    Ok(format!("{}/.cache/patchwork-remote", probe(host)?.home))
}

/// The helper's file name for `host`'s system and processor.
fn helper_name(host: &Host) -> String {
    match host.os {
        Os::Linux => format!("{HELPER}-linux-{}", host.arch),
        Os::Mac => format!("{HELPER}-macos-{}", host.arch),
        Os::Windows => format!("{HELPER}-windows-{}.exe", host.arch),
    }
}

/// The helper built for `host`: beside Patchwork's binary, where cargo puts a build for that
/// target in a development build, or downloaded from this version's release before.
fn local_helper(host: &Host) -> Option<PathBuf> {
    let name = helper_name(host);
    let cached = crate::dirs()
        .cache
        .join("remote-helpers")
        .join(crate::VERSION)
        .join(&name);
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let (triple, file) = match host.os {
        Os::Linux => (
            format!("{}-unknown-linux-musl", host.arch),
            HELPER.to_string(),
        ),
        Os::Mac => (format!("{}-apple-darwin", host.arch), HELPER.to_string()),
        Os::Windows => (
            format!("{}-pc-windows-gnu", host.arch),
            format!("{HELPER}.exe"),
        ),
    };
    let built = dir.parent().map(|target| target.join(&triple));
    // A development build for the machine Patchwork runs on puts the helper beside it.
    let native = (host.os == this_os() && host.arch == std::env::consts::ARCH)
        .then(|| dir.join(&file))
        .filter(|_| host.os != Os::Linux);
    [Some(dir.join(&name)), Some(cached)]
        .into_iter()
        .chain(["release", "debug"].map(|p| built.as_ref().map(|t| t.join(p).join(&file))))
        .chain([native])
        .flatten()
        .find(|p| p.is_file())
}

fn this_os() -> Os {
    if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::Mac
    } else {
        Os::Linux
    }
}

/// The helper for `host`'s bytes: this copy's own, or the release's, downloaded and kept.
fn helper_bytes(host: &Host) -> Result<Vec<u8>, Error> {
    if let Some(local) = local_helper(host) {
        return std::fs::read(&local)
            .map_err(|e| format!("Cannot read {}: {e}", local.display()).into());
    }
    let name = helper_name(host);
    log::info!(name:% = name; "downloading the SSH helper {{name}}");
    let source = HELPER_SOURCE
        .get()
        .ok_or_else(|| format!("Patchwork has no helper for {name}"))?;
    let bytes = source(&name)?;
    let dir = crate::dirs()
        .cache
        .join("remote-helpers")
        .join(crate::VERSION);
    if let Err(e) =
        std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(dir.join(&name), &bytes))
    {
        log::warn!(error:% = e; "could not keep the SSH helper: {{error}}");
    }
    Ok(bytes)
}

/// The first 16 hex digits of the SHA-256 of `bytes`.
fn short_hash(bytes: &[u8]) -> String {
    let mut hash = format!("{:x}", Sha256::digest(bytes));
    hash.truncate(16);
    hash
}

/// The helper on `host`, copied there first if it isn't yet.
///
/// # Errors
///
/// Returns an error when the host runs a system Patchwork has no helper for, or the helper
/// cannot be copied.
pub(super) fn helper(host: &str) -> Result<Installed, Error> {
    if let Some(known) = HELPERS.locked().get(host) {
        return Ok(known.clone());
    }
    let about = probe(host)?;
    let bytes = helper_bytes(&about)?;
    // Named by its contents, so a new build of the helper never runs an old copy.
    let mut name = format!("{HELPER}-{}", short_hash(&bytes));
    if about.os == Os::Windows {
        name.push_str(".exe");
    }
    // An earlier run saw this very helper there. A call it fails to answer has it copied anew.
    let remembered = super::host::helper_name(host).as_deref() == Some(name.as_str());
    let installed = Installed {
        path: format!("{}/bin/{name}", base(host)?),
        name,
        os: about.os,
        remembered,
    };
    if !remembered {
        if about.os == Os::Windows {
            copy_windows(host, &installed, &bytes)?;
        } else {
            copy_unix(host, &installed, &bytes)?;
        }
        super::host::set_helper(host, &installed.name);
    }
    HELPERS.locked().insert(host.to_string(), installed.clone());
    // Off the caller's way: the helper marks itself used and deletes old versions' copies.
    let host = host.to_string();
    std::thread::spawn(move || {
        if let Err(e) = super::op::call::<()>(&host, &super::op::Request::Prune) {
            log::warn!(host:% = host, error:% = e; "could not clear old helpers on {{host}}: {{error}}");
        }
    });
    Ok(installed)
}

/// Forgets `host`'s helper, the files sent to it, and what the host said about itself, so all
/// are looked at again: for a host that was reinstalled, or whose helper was deleted.
pub fn forget(host: &str) {
    HELPERS.locked().remove(host);
    super::mirror::forget(host);
    super::session::close(host);
    super::host::forget(host);
}

/// Copies the helper to a Linux or macOS host, unless it is there already. The helper's bytes
/// go only to a host that lacks it.
fn copy_unix(host: &str, installed: &Installed, bytes: &[u8]) -> Result<(), Error> {
    let q = quote(&installed.path);
    if send(host, &posix(&format!("test -x {q}")), None)?
        .status
        .success()
    {
        return Ok(());
    }
    let line = format!(
        "umask 077 && mkdir -p \"$(dirname {q})\" && cat > {q}.part && chmod 700 {q}.part && mv -f {q}.part {q}"
    );
    let out = send(host, &posix(&line), Some(bytes))?;
    if out.status.success() {
        return Ok(());
    }
    Err(format!(
        "Could not copy Patchwork's helper to {host}: {}",
        super::connection::failure_reason(&String::from_utf8_lossy(&out.stderr))
            .unwrap_or("it gave no reason")
    )
    .into())
}

/// Copies the helper to a Windows host over SFTP, unless it answers there already.
fn copy_windows(host: &str, installed: &Installed, bytes: &[u8]) -> Result<(), Error> {
    let ping = br#"{"op":"ping"}"#;
    let out = send(host, &installed.line(&[super::op::OP_ARG]), Some(ping))?;
    if serde_json::from_slice::<super::op::Reply>(&out.stdout).is_ok() {
        return Ok(());
    }
    // SFTP reads files from this machine, so the bytes go through a file of Patchwork's own.
    let dir = crate::dirs().cache.join("remote-helpers");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    let local = dir.join(format!("upload-{}", crate::new_id()));
    std::fs::write(&local, bytes).map_err(|e| format!("Cannot write {}: {e}", local.display()))?;
    let local_name = local.to_string_lossy().replace('\\', "/");
    let bin = ".cache/patchwork-remote/bin";
    let target = format!("{bin}/{}", installed.name);
    // A `-` before a command lets the batch go on when it fails, as making a folder that is
    // there already does.
    let batch = format!(
        "-mkdir .cache\n-mkdir .cache/patchwork-remote\n-mkdir {bin}\n\
         put \"{local_name}\" \"{target}.part\"\n-rm \"{target}\"\nrename \"{target}.part\" \"{target}\"\n"
    );
    let result = sftp(host, &batch);
    let _ = std::fs::remove_file(&local);
    let out = result?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        let reason = super::connection::failure_reason(&said).unwrap_or("sftp failed");
        return Err(format!("Could not copy Patchwork's helper to {host}: {reason}").into());
    }
    Ok(())
}

/// Runs the SFTP commands in `batch` on `host`.
fn sftp(host: &str, batch: &str) -> Result<Output, Error> {
    let mut cmd = super::connection::sftp(host)?;
    cmd.args(["-b", "-", "--", host])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Could not run sftp: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(batch.as_bytes())
            .map_err(|e| format!("Could not run sftp: {e}"))?;
    }
    Ok(child.wait_with_output()?)
}
