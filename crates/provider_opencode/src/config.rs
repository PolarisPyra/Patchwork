//! OpenCode binary lookup, 1.x/2.x database compatibility check, auth, and model listing.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::sync::Mutex;

use patchwork_provider_core::{
    AgentKind, AgentStatus, ModelOption, PROBE_TIMEOUT, command_at, find_binary, located_or,
    probe_at, probe_command,
};
use patchwork_platform::Locked;
use patchwork_platform::files::paths::home_dir;
#[cfg(feature = "sqlite")]
use rusqlite::{Connection, OpenFlags};

/// `opencode` on PATH or in the usual install folders, else where its installer puts it.
#[must_use]
pub fn opencode_bin() -> Option<PathBuf> {
    located_or(AgentKind::OpenCode, || {
        find_binary("opencode").or_else(|| {
            home_dir()
                .map(|h| {
                    h.join(".opencode/bin")
                        .join(format!("opencode{}", std::env::consts::EXE_SUFFIX))
                })
                .filter(|p| p.is_file())
        })
    })
}

/// The database OpenCode 1.x and 2.x share, in OpenCode's data folder.
pub const SHARED_DATABASE: &str = "opencode.db";

/// The database Patchwork gives OpenCode 1.x when it cannot open the shared one.
pub const V1_DATABASE: &str = "opencode-v1.db";

/// OpenCode's data folder, where both databases live.
#[must_use]
pub fn data_dir() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".local/share/opencode"))
}

/// Whether the OpenCode in use is a 1.x that cannot open the shared database.
///
/// 1.x opens only a database with a `session` table. Once OpenCode 2 has migrated the shared
/// database to `session_v2`, giving 1.x its own database lets it run without being updated.
#[cfg(feature = "sqlite")]
#[must_use]
pub fn needs_own_database() -> bool {
    let Some(shared) = data_dir().map(|dir| dir.join(SHARED_DATABASE)) else {
        return false;
    };
    if !shared.is_file() || major_version() != 1 {
        return false;
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let tables: Vec<String> = Connection::open_with_flags(shared, flags)
        .and_then(|conn| {
            conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table'")?
                .query_map([], |row| row.get(0))?
                .collect()
        })
        .unwrap_or_default();
    !tables.is_empty() && !tables.iter().any(|name| name == "session")
}

/// Whether the OpenCode in use is a 1.x that cannot open the shared database: never, in a build
/// that cannot read the database, such as the helper on an SSH host, which starts no OpenCode.
#[cfg(not(feature = "sqlite"))]
#[must_use]
pub fn needs_own_database() -> bool {
    false
}

/// Checks whether the OpenCode CLI is installed and usable.
pub fn auth(status: &mut AgentStatus) {
    status.signed_in = opencode_bin().is_some();
    status.detail = if status.signed_in {
        "Using the OpenCode CLI — run `opencode auth login` to add providers".into()
    } else {
        "OpenCode CLI is not installed".into()
    };
}

/// The models OpenCode can use with the providers it is signed in to, as `provider/model`.
#[must_use]
pub fn models() -> Vec<ModelOption> {
    let Some(listed) = opencode_bin()
        .and_then(|bin| {
            let mut cmd = command_at(&bin);
            if needs_own_database() {
                cmd.env("OPENCODE_DB", V1_DATABASE);
            }
            probe_command(cmd, &["models"], PROBE_TIMEOUT, |_| false)
        })
        .filter(|p| p.success)
    else {
        return Vec::new();
    };
    listed
        .stdout
        .lines()
        .map(str::trim)
        .filter(|line| line.contains('/') && !line.contains(char::is_whitespace))
        .map(|id| ModelOption::listed(id, id))
        .collect()
}

/// The CLI's major version, re-read only when the binary changes.
#[must_use]
pub fn major_version() -> u32 {
    static MAJOR: Mutex<Option<(BinaryKey, u32)>> = Mutex::new(None);
    let bin = opencode_bin();
    let key = bin.clone().map(|bin| {
        let modified = std::fs::metadata(&bin).and_then(|m| m.modified()).ok();
        (bin, modified)
    });
    let mut cached = MAJOR.locked();
    if let Some((_, major)) = cached.as_ref().filter(|(k, _)| *k == key) {
        return *major;
    }
    let major = bin
        .and_then(|bin| probe_at(&bin, &["--version"]))
        .and_then(|p| {
            p.stdout
                .split_whitespace()
                .find_map(|w| w.trim_start_matches('v').split('.').next()?.parse().ok())
        })
        .unwrap_or(1);
    *cached = Some((key, major));
    major
}

type BinaryKey = Option<(PathBuf, Option<std::time::SystemTime>)>;
