//! Copying Patchwork's helper and Patchwork's own files to an SSH host.
//!
//! An agent on a host runs with the same launch files it would get here (settings, skills,
//! hooks, the relay file they read), so the files a command names under Patchwork's own
//! folders are copied to a mirror on the host, and the command names the copies. Patchwork's
//! binary, which hooks and proxies run, becomes the helper built for the host.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::sync::{Mutex, OnceLock, PoisonError};

use sha2::{Digest, Sha256};

use super::command::{posix, send};
use super::host::{Host, Os, probe};
use super::location::quote;

/// The helper's name, beside Patchwork's binary, with the host's system and processor after it.
const HELPER: &str = "patchwork-remote";

/// The most bytes of files copied for one command, so a folder named by mistake cannot fill
/// the host.
const MOST: u64 = 256 * 1024 * 1024;

/// The helper on each host it was copied to.
static HELPERS: Mutex<Option<HashMap<String, Installed>>> = Mutex::new(None);

/// A hash of what each file copied to a host held, by host and local path, so unchanged files
/// are not sent again.
static SENT: Mutex<Option<HashMap<(String, String), u64>>> = Mutex::new(None);

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
    let cached = crate::system::dirs()
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
    let bytes = crate::updates::release_helper(&name)?;
    let dir = crate::system::dirs()
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
    Sha256::digest(bytes)
        .iter()
        .take(8)
        .fold(String::with_capacity(16), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        })
}

/// The helper on `host`, copied there first if it isn't yet.
///
/// # Errors
///
/// Returns an error when the host runs a system Patchwork has no helper for, or the helper
/// cannot be copied.
pub(super) fn helper(host: &str) -> Result<Installed, Error> {
    if let Some(known) = HELPERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(HashMap::new)
        .get(host)
    {
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
    HELPERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(HashMap::new)
        .insert(host.to_string(), installed.clone());
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
    if let Some(helpers) = HELPERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        helpers.remove(host);
    }
    if let Some(sent) = SENT.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
        sent.retain(|(sent_to, _), _| sent_to != host);
    }
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
        String::from_utf8_lossy(&out.stderr).trim()
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
    let dir = crate::system::dirs().cache.join("remote-helpers");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    let local = dir.join(format!("upload-{}", crate::system::new_id()));
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
        let reason = said
            .lines()
            .map(str::trim)
            .rfind(|l| !l.is_empty() && !l.starts_with("**"))
            .unwrap_or("sftp failed");
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

/// Patchwork's folders whose files a command may name, and whether a folder named under each
/// is copied whole. The data folder holds the database, so only its single files go.
fn roots() -> &'static [(String, bool)] {
    static ROOTS: OnceLock<Vec<(String, bool)>> = OnceLock::new();
    ROOTS.get_or_init(|| {
        let dirs = crate::system::dirs();
        [
            (&dirs.config, true),
            (&dirs.cache, true),
            (&dirs.state, true),
            (&dirs.data, false),
        ]
        .into_iter()
        .filter(|(p, _)| p.is_absolute())
        .map(|(p, whole)| (p.to_string_lossy().into_owned(), whole))
        .collect()
    })
}

/// Whether `c` ends a path written inside an argument, a JSON value or a TOML string.
fn ends_path(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '"' | '\'' | ',' | ';' | ')' | ']' | '}' | '>' | '`' | '\0'
        )
}

/// A local path as it is named under the mirror folder: `/a/b` stays, and `C:\a\b` becomes
/// `/C/a/b`.
fn mirror_name(path: &str) -> String {
    let path = path.replace('\\', "/");
    if super::location::has_drive(&path) {
        format!("/{}{}", &path[..1], &path[2..])
    } else {
        path
    }
}

/// The files a command names under Patchwork's folders, gathered for one copy to the host.
pub(super) struct Mirror {
    host: String,
    installed: Installed,
    /// The folder the files go in on the host; a local path `/a/b` becomes `<folder>/a/b`.
    folder: String,
    exe: Option<String>,
    /// What to send: name under the mirror, contents as the host gets them, and its [`Kind`].
    files: Vec<(String, Vec<u8>, Kind)>,
    seen: HashSet<String>,
    size: u64,
}

impl Mirror {
    /// A mirror of files for `host`, with the helper there already.
    pub(super) fn new(host: &str) -> Result<Self, Error> {
        Ok(Self {
            host: host.to_string(),
            installed: helper(host)?,
            folder: format!("{}/mirror", base(host)?),
            exe: std::env::current_exe()
                .ok()
                .map(|p| p.to_string_lossy().into_owned()),
            files: Vec::new(),
            seen: HashSet::new(),
            size: 0,
        })
    }

    /// The helper on the host.
    pub(super) fn helper(&self) -> &Installed {
        &self.installed
    }

    /// `text` with Patchwork's binary and files named as on the host, and the files it names
    /// gathered to send.
    pub(super) fn rewrite(&mut self, text: &str) -> Result<String, Error> {
        let mut text = text.to_string();
        if let Some(exe) = &self.exe {
            text = text.replace(exe.as_str(), &self.installed.path);
            // A Windows path written in JSON doubles its backslashes.
            if exe.contains('\\') {
                text = text.replace(&exe.replace('\\', r"\\"), &self.installed.path);
            }
        }
        for (root, whole) in roots() {
            text = self.rewrite_root(&text, root, *whole, false)?;
            if root.contains('\\') {
                text = self.rewrite_root(&text, &root.replace('\\', r"\\"), *whole, true)?;
            }
        }
        Ok(text)
    }

    /// `text` with the paths under the local folder `root` named as their mirrors; `escaped`
    /// when `root` is written with doubled backslashes.
    fn rewrite_root(
        &mut self,
        text: &str,
        root: &str,
        whole: bool,
        escaped: bool,
    ) -> Result<String, Error> {
        if !text.contains(root) {
            return Ok(text.to_string());
        }
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(at) = rest.find(root) {
            let tail = &rest[at..];
            // Looked for after the folder itself, whose own name may hold a space.
            let len = tail[root.len()..]
                .find(ends_path)
                .map_or(tail.len(), |end| root.len() + end);
            let path = &tail[..len];
            // A longer name that only starts like the folder, such as `patchwork-old`, is
            // something else.
            let after = &path[root.len()..];
            let inside = after.is_empty() || after.starts_with(['/', '\\']);
            out.push_str(&rest[..at]);
            if inside {
                let local = if escaped {
                    path.replace(r"\\", "\\")
                } else {
                    path.to_string()
                };
                self.gather(Path::new(&local), whole)?;
                out.push_str(&self.folder);
                out.push_str(&mirror_name(&local));
            } else {
                out.push_str(path);
            }
            rest = &tail[len..];
        }
        out.push_str(rest);
        Ok(out)
    }

    /// Adds `path` to what is sent: a file, or a folder's files when `whole`.
    fn gather(&mut self, path: &Path, whole: bool) -> Result<(), Error> {
        let key = path.to_string_lossy().into_owned();
        if !self.seen.insert(key.clone()) {
            return Ok(());
        }
        let Ok(meta) = std::fs::metadata(path) else {
            // Named but not there yet, such as a file the command writes; its folder is made.
            if let Some(parent) = path.parent() {
                let parent = mirror_name(&parent.to_string_lossy());
                self.files.push((parent, Vec::new(), Kind::Folder));
            }
            return Ok(());
        };
        if meta.is_dir() {
            if !whole {
                // A folder of the data folder, such as where conversations are kept, is made
                // empty: what is in it stays on this machine.
                self.files
                    .push((mirror_name(&key), Vec::new(), Kind::Folder));
                return Ok(());
            }
            let entries = std::fs::read_dir(path).map_err(|e| format!("Cannot read {key}: {e}"))?;
            for entry in entries.flatten() {
                self.gather(&entry.path(), true)?;
            }
            return Ok(());
        }
        self.size += meta.len();
        if self.size > MOST {
            return Err(format!(
                "{key} and the files before it are too large to copy to {}",
                self.host
            )
            .into());
        }
        let bytes = std::fs::read(path).map_err(|e| format!("Cannot read {key}: {e}"))?;
        // A launch file may itself name Patchwork's binary or other files, such as a settings
        // file whose hooks run Patchwork.
        let bytes = match String::from_utf8(bytes) {
            Ok(text) => self.rewrite(&text)?.into_bytes(),
            Err(e) => e.into_bytes(),
        };
        let kind = if runs(&meta) {
            Kind::Program
        } else {
            Kind::File
        };
        self.files.push((mirror_name(&key), bytes, kind));
        Ok(())
    }

    /// Sends the gathered files that changed since they were last sent to the host.
    pub(super) fn send(self) -> Result<(), Error> {
        let mut stream = Vec::new();
        let mut sent = Vec::new();
        {
            let guard = SENT.lock().unwrap_or_else(PoisonError::into_inner);
            for (path, bytes, kind) in &self.files {
                let mut hasher = DefaultHasher::new();
                (bytes, *kind as u8).hash(&mut hasher);
                let hash = hasher.finish();
                let key = (self.host.clone(), path.clone());
                if guard.as_ref().and_then(|m| m.get(&key)) == Some(&hash) {
                    continue;
                }
                frame(&mut stream, path, bytes, *kind);
                sent.push((key, hash));
            }
        }
        if sent.is_empty() {
            return Ok(());
        }
        let line = self.installed.line(&[super::serve::UNPACK_ARG]);
        let out = send(&self.host, &line, Some(&stream))?;
        if !out.status.success() {
            return Err(format!(
                "Could not copy Patchwork's files to {}: {}",
                self.host,
                String::from_utf8_lossy(&out.stderr).trim()
            )
            .into());
        }
        SENT.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert_with(HashMap::new)
            .extend(sent);
        Ok(())
    }
}
/// `text` with the paths it names under Patchwork's folders as they are on `host`.
///
/// Patchwork's binary and those files are copied there first. This is for what a command reads
/// on stdin, which [`super::supervised`] does not see.
///
/// # Errors
///
/// Returns an error when the host cannot be reached or the files cannot be copied to it.
pub fn mirrored(host: &str, text: &str) -> Result<String, Error> {
    let mut mirror = Mirror::new(host)?;
    let text = mirror.rewrite(text)?;
    mirror.send()?;
    Ok(text)
}

/// Whether the file `meta` describes is a program.
fn runs(meta: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        false
    }
}

/// Appends one entry to the stream the helper unpacks: the path's length and the path, its
/// [`Kind`], the contents' length and the contents.
fn frame(stream: &mut Vec<u8>, path: &str, bytes: &[u8], kind: Kind) {
    stream.extend_from_slice(&(path.len() as u64).to_be_bytes());
    stream.extend_from_slice(path.as_bytes());
    stream.push(kind as u8);
    stream.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    stream.extend_from_slice(bytes);
}

/// What an entry in the stream the helper unpacks is, as the byte after its path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Kind {
    File = 0,
    Program = 1,
    Folder = 2,
}
