//! One Patchwork per data folder: how a second launch of the same app finds the one already
//! running and asks it to come forward, instead of opening beside it.
//!
//! The first Patchwork to open a data folder locks it (see `persistence`), and writes beside
//! the lock where it listens and which app it is. A launch that finds the folder locked by the
//! same app asks that one to show its window and exits. A different build, such as a
//! development build beside the installed app, is not the same app and opens as before.

// Rust guideline compliant 2026-10-09

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{Value, json};

/// Locked by the first Patchwork to open the data folder, for as long as it runs.
pub(crate) const LOCK_FILE: &str = "patchwork.lock";
/// Where the Patchwork holding the lock listens, and which app it is. It outlives that
/// Patchwork, so it is only believed while the lock is held.
const INSTANCE_FILE: &str = "patchwork.instance";

/// How long the running Patchwork has to answer. It answers at once unless it is hung, and a
/// launch that waited on a hung one would open nothing.
const ANSWER_WITHIN: Duration = Duration::from_secs(2);

/// The data folder this Patchwork holds the lock of, once it does.
static OWNED: OnceLock<PathBuf> = OnceLock::new();
/// Where this Patchwork's relay listens, as `{"port", "token"}`, once it does.
static LISTENING: OnceLock<Value> = OnceLock::new();

/// Which app this is: the `AppImage` it runs from, else its program file. Two launches of one
/// installed app share it; a development build does not share it with the installed app.
fn identity() -> String {
    std::env::var("APPIMAGE")
        .ok()
        .filter(|path| !path.is_empty())
        .or_else(|| Some(std::env::current_exe().ok()?.to_string_lossy().into_owned()))
        .unwrap_or_default()
}

/// Notes that this Patchwork holds the lock on `data_dir`.
pub(crate) fn owns(data_dir: &Path) {
    let _ = OWNED.set(data_dir.to_path_buf());
    announce();
}

/// Notes where this Patchwork's relay listens.
pub(crate) fn listens(endpoint: &Value) {
    let _ = LISTENING.set(endpoint.clone());
    announce();
}

/// Writes where this Patchwork can be reached, once it both holds the lock and listens. The two
/// happen on different threads, in either order.
fn announce() {
    let (Some(dir), Some(endpoint)) = (OWNED.get(), LISTENING.get()) else {
        return;
    };
    let note = json!({
        "port": endpoint["port"],
        "token": endpoint["token"],
        "app": identity(),
    });
    if let Err(e) = super::fs::write_private(&dir.join(INSTANCE_FILE), &note.to_string()) {
        log::warn!(error:% = e; "could not note where this Patchwork listens: {{error}}");
    }
}

/// Asks the Patchwork already running on `data_dir` to show its window, when it is this same
/// app. Whether it answered, in which case this launch has nothing left to do.
///
/// False when no Patchwork holds the folder, when the one that does is a different build, or
/// when it does not answer in time.
#[must_use]
pub fn raise_running(data_dir: &Path) -> bool {
    let Ok(lock) = std::fs::File::open(data_dir.join(LOCK_FILE)) else {
        return false;
    };
    // Taking the lock means nobody holds it. It is let go at once, for the store to take.
    if lock.try_lock().is_ok() {
        return false;
    }
    let note = std::fs::read_to_string(data_dir.join(INSTANCE_FILE))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let Some(note) = note.filter(|note| note["app"].as_str() == Some(identity().as_str())) else {
        return false;
    };
    let Some(port) = note["port"].as_u64().and_then(|p| u16::try_from(p).ok()) else {
        return false;
    };
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, ANSWER_WITHIN) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(ANSWER_WITHIN));
    let hello = json!({"token": note["token"], "raise": true});
    if writeln!(stream, "{hello}").is_err() {
        return false;
    }
    let mut answer = String::new();
    BufReader::new(stream).read_line(&mut answer).is_ok() && answer.trim() == "raised"
}
