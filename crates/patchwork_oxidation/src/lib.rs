//! Oxidation: a reusable Rust hot-reload library and CLI runner.
//!
//! Watches workspace paths configured in `oxidation.toml` (or programmatically via [`Config`]),
//! debounces saves, runs `cargo build`, and swaps the running application process cleanly.
//! With `run.handoff` enabled, Oxidation communicates over the [`patchwork_oxidation_client`] protocol to
//! preserve application state and Unix file descriptors across restarts, hot-reload assets in
//! place, and forward compiler diagnostics to the running app.

// Rust guideline compliant 2026-10-09

pub mod app;
pub mod build;
pub mod config;
pub mod keys;
pub mod session;
pub mod signals;
pub mod term;
pub mod watch;

/// Client SDK, wire protocol, and Unix descriptor handover shared with managed apps.
pub use patchwork_oxidation_client::{self as client, fds, protocol};

pub use config::{Build as BuildConfig, Config, Run as RunConfig, TargetBuild, Watch as WatchConfig};
pub use session::{NullReporter, Reporter, Session};
pub use term::{Mark, Stream, Term};

use std::path::PathBuf;
use std::sync::mpsc;

/// Events delivered from the file watcher, keyboard listener, cargo build, and running app.
#[derive(Debug)]
pub enum Event {
    /// A watched file was created, modified, moved, or removed, and the action it triggers.
    Changed(PathBuf, watch::Kind),
    /// A single-key command was typed in the terminal.
    Key(keys::Key),
    /// Cargo emitted progress or an artifact for the build with this id.
    Build(u64, build::Note),
    /// The managed app wrote a line to stdout or stderr.
    Output(term::Stream, Vec<u8>),
}

/// Runs a complete hot-reload session over `root` using `config`, reporting status to `reporter`.
///
/// Installs signal handlers, starts the file watcher and terminal key listener, and runs until
/// the user presses `q`, sends SIGINT/SIGTERM/SIGHUP, or the event channel closes.
///
/// # Errors
///
/// Returns an error if the file watcher cannot be started for `config.watch`.
pub fn run_session<R: Reporter>(
    root: PathBuf,
    config: Config,
    app_args: Vec<String>,
    reporter: &mut R,
) -> anyhow::Result<()> {
    signals::catch();
    let (send, events) = mpsc::channel();
    let _watcher = watch::start(&root, &config.watch, send.clone())?;
    let keys = keys::listen(send.clone());
    Session::new(reporter, config, root, app_args, send, keys.active()).run(&events);
    Ok(())
}
