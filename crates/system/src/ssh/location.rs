//! Where a remote project is: an SSH host alias and a folder on it, written as one string.
//!
//! A project on another machine keeps its folder as `ssh://<alias>/<absolute path>`, so every
//! function that takes a project root as a string carries the host along with it. Code that
//! reaches the folder asks [`split`] whether the root is remote, and runs over SSH when it is.
//!
//! A folder on a Linux or macOS host is `ssh://box/home/me/app`; one on a Windows host keeps its
//! drive and forward slashes, `ssh://pc/C:/Users/me/app`, which Windows reads as well.

// Rust guideline compliant 2026-10-09

/// The scheme that marks a root as a folder on an SSH host.
const SCHEME: &str = "ssh://";

/// A folder on an SSH host: the `Host` alias from `~/.ssh/config` and the absolute path there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location<'a> {
    pub host: &'a str,
    pub path: &'a str,
}

/// The host and folder of `root` when it names a folder on an SSH host, else None.
#[must_use]
pub fn split(root: &str) -> Option<Location<'_>> {
    let rest = root.strip_prefix(SCHEME)?;
    let slash = rest.find('/')?;
    let (host, path) = rest.split_at(slash);
    // A Windows path keeps its drive first, without the slash that separates it from the host.
    let path = if has_drive(&path[1..]) {
        &path[1..]
    } else {
        path
    };
    (!host.is_empty()).then_some(Location { host, path })
}

/// Whether `path` starts with a Windows drive, such as `C:`.
#[must_use]
pub fn has_drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && bytes.get(2).is_none_or(|&b| b == b'/' || b == b'\\')
}

/// Whether `root` names a folder on an SSH host.
#[must_use]
pub fn is_remote(root: &str) -> bool {
    split(root).is_some()
}

/// The root for the absolute folder `path` on `host`.
#[must_use]
pub fn root(host: &str, path: &str) -> String {
    if path.starts_with('/') {
        format!("{SCHEME}{host}{path}")
    } else {
        format!("{SCHEME}{host}/{}", path.replace('\\', "/"))
    }
}

/// `root`'s folder as the machine it is on sees it: the path after the host for a remote root,
/// the root itself for a local one.
///
/// The helper Patchwork runs on an SSH host is handed roots as Patchwork keeps them, and reads
/// them with this.
#[must_use]
pub fn here(root: &str) -> &str {
    split(root).map_or(root, |l| l.path)
}

/// `rel` inside the folder `root`, local or remote, as a string of the same kind.
#[must_use]
pub fn join(root: &str, rel: &str) -> String {
    if rel.is_empty() {
        return root.to_string();
    }
    if is_remote(root) {
        format!(
            "{}/{}",
            root.trim_end_matches('/'),
            rel.trim_start_matches('/')
        )
    } else {
        std::path::Path::new(root)
            .join(rel)
            .to_string_lossy()
            .into_owned()
    }
}

/// How `root` reads to the user: `alias:/path` for a remote folder, the path for a local one.
#[must_use]
pub fn display(root: &str) -> String {
    split(root).map_or_else(|| root.to_string(), |l| format!("{}:{}", l.host, l.path))
}

/// `text` quoted for a POSIX shell, and for fish, which reads `'\''` the same way.
#[must_use]
pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// `path`, which a command run in `root` printed, as a root of the same kind: on the same host
/// when `root` is remote.
#[must_use]
pub fn same_host(root: &str, path: &str) -> String {
    split(root).map_or_else(|| path.to_string(), |l| self::root(l.host, path))
}
