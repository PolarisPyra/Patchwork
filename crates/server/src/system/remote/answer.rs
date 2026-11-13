//! The helper's side of [`super::op`]: answering a request about this machine's folders.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::collections::BTreeMap;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::Serialize;

use super::op::{Reply, Request, Survey};

/// Answers the request on stdin with a reply on stdout, and returns the exit code.
#[must_use]
pub fn answer() -> i32 {
    let mut input = Vec::new();
    let reply = match std::io::stdin().read_to_end(&mut input) {
        Ok(_) => reply_to(&input),
        Err(e) => Reply::Failed(format!("Unreadable request: {e}")),
    };
    let Ok(body) = serde_json::to_vec(&reply) else {
        return 1;
    };
    let mut out = std::io::stdout().lock();
    if out.write_all(&body).and_then(|()| out.flush()).is_err() {
        return 1;
    }
    0
}

/// Answers each line of stdin, a request, with a line on stdout, until stdin closes, and returns
/// the exit code. Patchwork keeps one of these running per host ([`super::session`]).
#[must_use]
pub fn answer_all() -> i32 {
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().split(b'\n') {
        let Ok(line) = line else {
            return 1;
        };
        let Ok(mut body) = serde_json::to_vec(&reply_to(&line)) else {
            return 1;
        };
        body.push(b'\n');
        if out.write_all(&body).and_then(|()| out.flush()).is_err() {
            return 1;
        }
    }
    0
}

/// The reply to the request `input` holds as JSON.
fn reply_to(input: &[u8]) -> Reply {
    match serde_json::from_slice::<Request>(input) {
        Ok(request) => match handle(request) {
            Ok(value) => Reply::Done(value),
            Err(Error::OutsideProject) => Reply::Outside,
            Err(e) => Reply::Failed(e.to_string()),
        },
        Err(e) => Reply::Failed(format!("Unreadable request: {e}")),
    }
}

fn handle(request: Request) -> Result<serde_json::Value, Error> {
    match request {
        Request::Ping => json(crate::VERSION),
        Request::CanonicalDir { path } => json(dir(&home_path(&path))?),
        Request::MakeDirs { path } => {
            let path = home_path(&path);
            std::fs::create_dir_all(&path)
                .map_err(|e| format!("Cannot create {}: {e}", path.display()))?;
            json(dir(&path)?)
        }
        Request::CreateDir { parent, name } => json(create_dir(&parent, &name)?),
        Request::Resolve { dir, rel } => json(portable(&inside(&dir, &rel)?)),
        Request::Read { dir, rel, max } => json(read(&dir, &rel, max)?),
        Request::Remove { dir, rel } => {
            // Gone already counts as done: a request whose reply was lost is sent again.
            let joined = Path::new(&dir).join(rel.trim_start_matches('/'));
            if std::fs::symlink_metadata(&joined)
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
            {
                return json(());
            }
            let path = inside(&dir, &rel)?;
            match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    Err(format!("Cannot delete {rel}: {e}").into())
                }
                _ => json(()),
            }
        }
        Request::Sizes { dir, rels } => json(sizes(&dir, &rels)),
        Request::LineCounts { dir, rels } => json(line_counts(&dir, &rels)),
        Request::Browse { path } => json(browse(path.as_deref())?),
        Request::Stage { spec } => json(stage(&spec)?),
        Request::Survey {
            dir,
            names,
            programs,
            read,
            max,
        } => json(survey(&dir, &names, &programs, &read, max)),
        Request::Poll { dir } => json(poll(&dir)),
        Request::Snapshot { dir } => json(snapshot(&dir)?),
        Request::Prune => json(prune()?),
    }
}

fn json(value: impl Serialize) -> Result<serde_json::Value, Error> {
    serde_json::to_value(value).map_err(|e| e.to_string().into())
}

/// `path` as Patchwork writes paths on every host: forward slashes, and on Windows without the
/// `\\?\` prefix a resolved path carries.
#[must_use]
pub(super) fn portable(path: &Path) -> String {
    let text = path.to_string_lossy();
    if cfg!(windows) {
        let text = crate::system::fs::without_verbatim(path).unwrap_or_else(|| text.to_string());
        text.replace('\\', "/")
    } else {
        text.into_owned()
    }
}

/// The folder the helper keeps its files in: the one above the `bin` folder it runs from.
pub(super) fn base() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.parent()?.to_path_buf())
}

/// `path`, taken from the home folder when it is relative or starts with `~`.
fn home_path(path: &str) -> PathBuf {
    let home = crate::system::fs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    match path.trim() {
        "" | "~" => home,
        p => crate::system::fs::resolve(&home, p),
    }
}

/// The folder `path` with its links resolved, as a portable path.
fn dir(path: &Path) -> Result<String, Error> {
    let real = path
        .canonicalize()
        .map_err(|e| format!("{} is not a folder: {e}", path.display()))?;
    if !real.is_dir() {
        return Err(format!("{} is not a folder", path.display()).into());
    }
    Ok(portable(&real))
}

/// `rel` under `dir` with its links resolved; [`Error::OutsideProject`] when it leaves `dir`.
fn inside(dir: &str, rel: &str) -> Result<PathBuf, Error> {
    let root = Path::new(dir)
        .canonicalize()
        .map_err(|e| format!("{dir}: {e}"))?;
    let path = root
        .join(rel.trim_start_matches('/'))
        .canonicalize()
        .map_err(|e| format!("{rel}: {e}"))?;
    if path == root || !path.starts_with(&root) {
        return Err(Error::OutsideProject);
    }
    Ok(path)
}

fn create_dir(parent: &str, name: &str) -> Result<String, Error> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return Err(format!("{name} is not a folder name").into());
    }
    let parent = Path::new(parent)
        .canonicalize()
        .map_err(|e| format!("{parent} is not a folder: {e}"))?;
    let path = parent.join(name);
    if path.exists() {
        let empty = std::fs::read_dir(&path).is_ok_and(|mut d| d.next().is_none());
        if !empty {
            return Err(format!("{} already exists and is not empty", portable(&path)).into());
        }
    }
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("Cannot create {}: {e}", portable(&path)))?;
    dir(&path)
}

fn read(dir: &str, rel: &str, max: u64) -> Result<String, Error> {
    let path = inside(dir, rel)?;
    if !path.is_file() {
        return Err(format!("{rel} is not a file").into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .and_then(|f| f.take(max + 1).read_to_end(&mut bytes))
        .map_err(|e| format!("{rel}: {e}"))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

fn sizes(dir: &str, rels: &[String]) -> Vec<Option<u64>> {
    rels.iter()
        .map(|rel| {
            std::fs::symlink_metadata(Path::new(dir).join(rel)).ok()?;
            Some(
                inside(dir, rel)
                    .ok()
                    .and_then(|p| std::fs::metadata(p).ok())
                    .filter(std::fs::Metadata::is_file)
                    .map_or(0, |m| m.len()),
            )
        })
        .collect()
}

fn line_counts(dir: &str, rels: &[String]) -> Vec<u32> {
    rels.iter()
        .map(|rel| {
            let Ok(bytes) = inside(dir, rel).and_then(|p| Ok(std::fs::read(p)?)) else {
                return 0;
            };
            if bytes.contains(&0) {
                return 0;
            }
            #[expect(
                clippy::naive_bytecount,
                reason = "counted once per changed file, not worth a dependency"
            )]
            let lines = bytes.iter().filter(|&&b| b == b'\n').count();
            let last = usize::from(bytes.last().is_some_and(|&b| b != b'\n'));
            u32::try_from(lines + last).unwrap_or(u32::MAX)
        })
        .collect()
}

fn browse(path: Option<&str>) -> Result<crate::system::browse::DirListing, Error> {
    let mut listing = crate::system::browse::browse(path)?;
    let fix = |p: &mut String| *p = portable(Path::new(p.as_str()));
    fix(&mut listing.path);
    if let Some(parent) = &mut listing.parent {
        fix(parent);
    }
    for entry in &mut listing.entries {
        fix(&mut entry.path);
    }
    Ok(listing)
}

/// Writes `spec` where only this user can read it, under the helper's own folder, and returns
/// the name `--remote-exec` takes.
fn stage(spec: &super::command::Spec) -> Result<String, Error> {
    let dir = base()
        .ok_or("The helper cannot find its folder")?
        .join("run");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    let id = crate::system::new_id();
    let path = dir.join(format!("{id}.json"));
    let body = serde_json::to_vec(spec).map_err(|e| e.to_string())?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    };
    options
        .open(&path)
        .and_then(|mut f| f.write_all(&body))
        .map_err(|e| format!("Cannot write {}: {e}", path.display()))?;
    Ok(id)
}

fn survey(dir: &str, names: &[String], programs: &[String], read: &[String], max: u64) -> Survey {
    let dir = Path::new(dir);
    let mut found = Survey::default();
    for name in names {
        let path = dir.join(name);
        if path.is_file() {
            found.files.push(name.clone());
        } else if path.is_dir() {
            found.dirs.push(name.clone());
        }
    }
    found.programs = programs
        .iter()
        .filter(|p| crate::system::find_program(p).is_some())
        .cloned()
        .collect();
    found.texts = read
        .iter()
        .map(|name| {
            let mut bytes = Vec::new();
            std::fs::File::open(dir.join(name))
                .and_then(|f| f.take(max).read_to_end(&mut bytes))
                .ok()?;
            Some(String::from_utf8_lossy(&bytes).into_owned())
        })
        .collect();
    found
}

/// `git` with `args` in `dir`, found where the user's shell would find it.
fn git(dir: &str, args: &[&str], index: Option<&Path>) -> Result<String, Error> {
    let program = crate::system::find_program("git").ok_or("git is not installed")?;
    let mut cmd = crate::system::host_command(program);
    cmd.args(args).current_dir(dir);
    if let Some(index) = index {
        cmd.env("GIT_INDEX_FILE", index)
            .env("GIT_LITERAL_PATHSPECS", "1");
    }
    let out = cmd
        .output()
        .map_err(|e| format!("Could not run git: {e}"))?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        return Err(said.trim().to_string().into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn poll(dir: &str) -> BTreeMap<String, String> {
    let listed = git(
        dir,
        &["ls-files", "-z", "-m", "-o", "--exclude-standard"],
        None,
    )
    .unwrap_or_default();
    let mut state = BTreeMap::new();
    for rel in listed
        .split('\0')
        .filter(|r| !r.is_empty())
        .chain([".git/HEAD", ".git/index"])
    {
        let Ok(meta) = std::fs::metadata(Path::new(dir).join(rel)) else {
            continue;
        };
        let time = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        state.insert(rel.to_string(), format!("{time}.{}", meta.len()));
    }
    if let Ok(head) = git(dir, &["rev-parse", "HEAD"], None) {
        state.insert(".git/HEAD@".to_string(), head.trim().to_string());
    }
    state
}

fn snapshot(dir: &str) -> Result<String, Error> {
    let at = |rel: &str| -> Result<PathBuf, Error> {
        let path = git(dir, &["rev-parse", "--git-path", rel], None)?;
        Ok(Path::new(dir).join(path.trim()))
    };
    let index = at("index")?;
    let temp = at(&format!("patchwork-index-{}", crate::system::new_id()))?;
    if index.exists() {
        std::fs::copy(&index, &temp).map_err(|e| e.to_string())?;
    }
    let tree =
        git(dir, &["add", "-A"], Some(&temp)).and_then(|_| git(dir, &["write-tree"], Some(&temp)));
    let _ = std::fs::remove_file(&temp);
    Ok(tree?.trim().to_string())
}

/// How long a helper copy no Patchwork has started stays before [`prune`] deletes it.
const UNUSED: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 60 * 60);

/// How long a staged run spec stays before [`prune`] deletes it. `--remote-exec` reads and
/// removes one at once, so an older one is from a start that never happened, and may hold
/// forwarded credentials.
const STAGED: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// Marks this helper as in use, then deletes the other helper copies beside it that no Patchwork
/// has started for [`UNUSED`], with their marks.
///
/// Each Patchwork version installs a helper named for its contents, so copies from older
/// versions pile up otherwise. A copy another running Patchwork still uses was marked when that
/// one started, and stays.
fn prune() -> Result<(), Error> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let mark = |exe: &Path| {
        let mut name = exe.as_os_str().to_owned();
        name.push(".used");
        PathBuf::from(name)
    };
    std::fs::write(mark(&me), b"").map_err(|e| format!("Cannot mark the helper used: {e}"))?;
    let Some(bin) = me.parent() else {
        return Ok(());
    };
    let entries = std::fs::read_dir(bin).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path == me
            || !name.starts_with("patchwork-remote-")
            || path.extension().is_some_and(|x| x == "used")
        {
            continue;
        }
        let used = std::fs::metadata(mark(&path))
            .or_else(|_| entry.metadata())
            .and_then(|m| m.modified());
        let stale = used
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > UNUSED);
        if stale && std::fs::remove_file(&path).is_ok() {
            let _ = std::fs::remove_file(mark(&path));
        }
    }
    let staged = base()
        .map(|b| b.join("run"))
        .and_then(|d| std::fs::read_dir(d).ok());
    for entry in staged.into_iter().flatten().flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > STAGED);
        if old && entry.path().extension().is_some_and(|x| x == "json") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    Ok(())
}
