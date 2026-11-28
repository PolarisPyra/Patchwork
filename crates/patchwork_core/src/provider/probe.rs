//! Asking an agent CLI about itself: running it briefly with a time limit, and reading its
//! version, sign-in, and models.

// Rust guideline compliant 2026-10-09

use std::path::Path;
use std::time::Duration;

use super::{AgentKind, AgentStatus, ModelOption, ProviderKind, agent_command, located};

#[doc(inline)]
pub use patchwork_provider_core::Probe;
pub(super) use patchwork_provider_core::{PROBE_TIMEOUT, stop};
use patchwork_provider_core::{probe_command, version_in};

/// Runs the agent CLI with `args`. Returns None if it cannot start or exceeds
/// `PROBE_TIMEOUT` (the process group is killed in that case).
#[must_use]
pub fn probe(kind: AgentKind, args: &[&str]) -> Option<Probe> {
    probe_until(kind, args, |_| false)
}

/// Runs a CLI probe and stops it early once `done` accepts the output.
///
/// Like `probe`, but also ends the process once `done` accepts the stdout so far, and returns
/// that output as a success. Some CLIs print their answer and then keep running, so waiting
/// for them to exit would only ever reach the timeout.
pub fn probe_until(kind: AgentKind, args: &[&str], done: impl Fn(&str) -> bool) -> Option<Probe> {
    let cmd = agent_command(kind).ok()?;
    probe_command(cmd, args, PROBE_TIMEOUT, done)
}

/// Like `probe`, but runs in `dir` and allows `timeout`.
#[must_use]
pub fn probe_in(kind: AgentKind, args: &[&str], dir: &Path, timeout: Duration) -> Option<Probe> {
    let mut cmd = agent_command(kind).ok()?;
    cmd.current_dir(dir);
    probe_command(cmd, args, timeout, |_| false)
}

/// Checks whether `kind`'s CLI is installed, reads its version, and fills in sign-in status.
#[must_use]
pub fn detect_agent(kind: AgentKind) -> AgentStatus {
    let mut status = AgentStatus {
        kind,
        installed: false,
        version: None,
        signed_in: false,
        detail: "Not installed".into(),
    };
    // API providers run in Patchwork itself, so there is no CLI to probe.
    if kind.identity().cli.is_none() {
        kind.provider().sign_in(&mut status);
        return status;
    }
    let Some(version) = probe(kind, &["--version"]).filter(|p| p.success) else {
        if let Some(path) = located(kind) {
            status.detail = if path.is_file() {
                format!("Could not run {}", path.display())
            } else {
                format!("No file at {}", path.display())
            };
        }
        return status;
    };
    status.installed = true;
    status.version = version_in(&version.stdout);
    kind.provider().sign_in(&mut status);
    status
}

/// The CLI's version, as `--version` prints it.
#[must_use]
pub fn cli_version(kind: AgentKind) -> Option<String> {
    probe(kind, &["--version"])
        .filter(|p| p.success)
        .and_then(|p| version_in(&p.stdout))
}

/// Models offered for `kind`, starting with the CLI default.
#[must_use]
pub fn list_models(kind: AgentKind) -> Vec<ModelOption> {
    let (default_model, models) = kind.provider().models();
    let default_model = default_model.filter(|m| !m.trim().is_empty());
    // The default holds what the model it names holds, when that model is listed.
    let context_window = default_model.as_deref().and_then(|id| {
        models
            .iter()
            .find(|m| m.id.as_deref() == Some(id))?
            .context_window
    });
    let mut out = vec![ModelOption {
        id: None,
        label: "Default".into(),
        default_model,
        is_legacy: false,
        efforts: None,
        context_window,
        needs_cli: None,
    }];
    out.extend(models);
    super::cli_update::mark_models_needing_update(kind, &mut out);
    out
}
