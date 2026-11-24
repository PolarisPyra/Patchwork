//! Keeping the agent CLIs current: each one's newest release, and updating it the way it was
//! installed.
//!
//! A CLI updates itself (`claude update`, `codex update`, `agy update`, `opencode upgrade`,
//! `qwen update`, `kimi update`) unless Homebrew or winget owns it, or it is too old to have an
//! update command or has none (Auggie); then the command to run by hand is given instead, so
//! Patchwork never fights a package manager.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

use super::{AgentKind, ModelOption, acp, agent_binary, claude, cli_version, probe_in};
use crate::{Core, Error};
use crate::system::Locked;
use crate::system::files::paths::home_dir;

/// Longest a release lookup may take, so a slow server never holds up the Providers pane.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a looked-up release is trusted. CLIs ship at most a few times a day.
const LOOKUP_TTL: Duration = Duration::from_secs(60 * 60);
/// Longest an update may run. Codex's and Claude's download well over 100 MB.
const UPDATE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// The first Codex with `codex update`.
const CODEX_UPDATE_SINCE: &str = "0.126.0";
/// The first OpenCode published as `@opencode/cli` rather than `opencode-ai`.
const OPENCODE_CLI_PACKAGE_SINCE: u32 = 2;
/// Models an older CLI cannot run, with the first CLI version that can. Kept by hand, because
/// a CLI lists models it has not shipped support for yet, and a saved or configured model may
/// be newer than the CLI.
const MODEL_REQUIREMENTS: [(AgentKind, &str, &str); 8] = [
    (AgentKind::Claude, "claude-fable-5-2", "2.1.321"),
    (AgentKind::Claude, "claude-haiku-5-5", "2.1.293"),
    (AgentKind::Claude, "claude-sonnet-5-5", "2.1.284"),
    (AgentKind::Claude, "claude-opus-5-5", "2.1.280"),
    (AgentKind::Codex, "gpt-6.2-sol", "0.169.1"),
    (AgentKind::Codex, "gpt-6.2-astra", "0.169.1"),
    (AgentKind::Codex, "gpt-6.1-terra", "0.165.0"),
    (AgentKind::Codex, "gpt-6-luna", "0.156.1"),
];

/// The newest version each release URL named, and when it was asked.
static LATEST: Mutex<Vec<(String, Instant, String)>> = Mutex::new(Vec::new());

/// Each CLI binary's version, by path and change time, so listing models asks it only once.
static VERSIONS: Mutex<Vec<(PathBuf, SystemTime, String)>> = Mutex::new(Vec::new());

/// How a CLI is brought up to date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateMethod {
    /// Patchwork runs the CLI's own update command.
    Run,
    /// The user runs this command, because a package manager owns the CLI or it cannot update
    /// itself.
    Manual(String),
}

/// A newer release of an installed CLI, and how to install it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliUpdate {
    pub installed: String,
    pub latest: String,
    pub method: UpdateMethod,
}

/// Looks up whether `kind`'s CLI has a newer release; `None` when it is current.
///
/// Releases are looked up at most once an hour per source.
///
/// # Errors
///
/// Returns an error when the CLI is not installed, its version cannot be read, or its newest
/// release cannot be looked up.
pub fn check_cli_update(kind: AgentKind) -> Result<Option<CliUpdate>, Error> {
    let bin = agent_binary(kind)?;
    let installed = cli_version(kind).ok_or("Could not read the CLI's version")?;
    let latest = latest_version(kind, &installed)?;
    if !is_older(&installed, &latest) {
        return Ok(None);
    }
    Ok(Some(CliUpdate {
        method: method(kind, &bin, &installed),
        installed,
        latest,
    }))
}

/// Updates `kind`'s CLI with its own update command, waiting up to 10 minutes.
///
/// # Errors
///
/// Returns an error when the CLI is not installed, must be updated by hand, or its update command
/// cannot start, runs too long, or fails.
pub fn update_cli(kind: AgentKind) -> Result<(), Error> {
    let bin = agent_binary(kind)?;
    let installed = cli_version(kind).ok_or("Could not read the CLI's version")?;
    if let UpdateMethod::Manual(command) = method(kind, &bin, &installed) {
        return Err(format!("Update it by running `{command}` in a terminal").into());
    }
    let args: &[&str] = match kind {
        AgentKind::OpenCode => &["upgrade"],
        AgentKind::Claude | AgentKind::Codex | AgentKind::Antigravity => &["update"],
        AgentKind::Qwen | AgentKind::Kimi | AgentKind::Auggie => acp::spec(kind)
            .and_then(|s| s.update)
            .map(|(args, _)| args)
            .ok_or("This CLI has no update command")?,
        AgentKind::Api => return Err("API providers have no CLI to update".into()),
    };
    let home = home_dir().ok_or("Could not find the home folder")?;
    let shown = format!("{} {}", file_name(&bin), args.join(" "));
    let run = probe_in(kind, args, &home, UPDATE_TIMEOUT).ok_or_else(|| {
        format!("`{shown}` could not start, or ran longer than 10 minutes and was stopped")
    })?;
    if !run.success {
        let said = if run.stderr.trim().is_empty() {
            &run.stdout
        } else {
            &run.stderr
        };
        return Err(format!("`{shown}` failed: {}", last_lines(said, 3)).into());
    }
    log::info!(agent:? = kind; "updated the {{agent}} CLI");
    Ok(())
}

/// Remembers, per CLI, the newest release the desktop already announced.
const ANNOUNCED_KEY: &str = "cli-update-announced";

impl Core {
    /// The connected CLIs with a newer release not yet announced, each marked announced.
    ///
    /// Empty when the notification is turned off in Settings → Notifications. A CLI whose release
    /// cannot be looked up is skipped until the next check.
    pub fn unannounced_cli_updates(&self) -> Vec<(AgentKind, CliUpdate)> {
        let settings = self.settings();
        if !settings.notifications.cli_updates {
            return Vec::new();
        }
        let mut found = Vec::new();
        for kind in settings.agents.into_iter().filter(|k| *k != AgentKind::Api) {
            let update = match check_cli_update(kind) {
                Ok(Some(update)) => update,
                Ok(None) => continue,
                Err(e) => {
                    log::warn!(agent:? = kind, error:% = e; "could not check {{agent}} for updates: {{error}}");
                    continue;
                }
            };
            let key = format!("{ANNOUNCED_KEY}-{kind:?}");
            if self.store.state(&key).as_deref() == Some(update.latest.as_str()) {
                continue;
            }
            if let Err(e) = self.store.set_state(&key, &update.latest) {
                log::warn!(error:% = e; "could not remember an announced CLI update: {{error}}");
            }
            found.push((kind, update));
        }
        found
    }
}

/// How `kind`'s CLI at `bin`, at version `installed`, is brought up to date.
fn method(kind: AgentKind, bin: &Path, installed: &str) -> UpdateMethod {
    // An npm or Homebrew install is a link to the real program, whose path tells who owns it.
    let real = std::fs::canonicalize(bin).unwrap_or_else(|_| bin.to_path_buf());
    let path = real.to_string_lossy().replace('\\', "/");
    if let Some(command) = homebrew_command(&path) {
        return UpdateMethod::Manual(command);
    }
    if kind == AgentKind::Claude && path.contains("Microsoft/WinGet/Packages/Anthropic.ClaudeCode_")
    {
        return UpdateMethod::Manual("winget upgrade Anthropic.ClaudeCode".into());
    }
    if kind == AgentKind::Codex && is_older(installed, CODEX_UPDATE_SINCE) {
        let command = if path.contains("/node_modules/@openai/codex/") {
            "npm install -g @openai/codex@latest"
        } else if cfg!(windows) {
            "powershell -c \"irm https://chatgpt.com/codex/install.ps1 | iex\""
        } else {
            "curl -fsSL https://chatgpt.com/codex/install.sh | sh"
        };
        return UpdateMethod::Manual(command.into());
    }
    if let Some(spec) = acp::spec(kind) {
        let runs = spec
            .update
            .is_some_and(|(_, since)| !is_older(installed, since));
        if !runs {
            return UpdateMethod::Manual(format!("npm install -g {}@latest", spec.npm_package));
        }
    }
    UpdateMethod::Run
}

/// `brew upgrade` for a program under a Homebrew keg, such as `/opt/homebrew/Caskroom/codex/…`.
fn homebrew_command(path: &str) -> Option<String> {
    [("/Caskroom/", " --cask"), ("/Cellar/", "")]
        .into_iter()
        .find_map(|(folder, flag)| {
            let rest = &path[path.find(folder)? + folder.len()..];
            let keg = rest.split('/').next().filter(|k| !k.is_empty())?;
            Some(format!("brew upgrade{flag} {keg}"))
        })
}

/// The newest release of `kind`'s CLI. OpenCode's source depends on the `installed` major.
fn latest_version(kind: AgentKind, installed: &str) -> Result<String, Error> {
    let (url, read): (String, fn(&str) -> Option<String>) = match kind {
        AgentKind::Claude => (
            format!(
                "https://downloads.claude.ai/claude-code-releases/{}",
                claude_channel()
            ),
            |body| Some(body.trim().to_string()),
        ),
        AgentKind::Codex => (
            "https://api.github.com/repos/openai/codex/releases/latest".into(),
            |body| {
                let tag = json_field(body, "tag_name")?;
                Some(tag.strip_prefix("rust-v").unwrap_or(&tag).to_string())
            },
        ),
        AgentKind::OpenCode => {
            let major = version_parts(installed).and_then(|p| p.first().copied());
            let package = if major.is_some_and(|m| m >= OPENCODE_CLI_PACKAGE_SINCE) {
                "@opencode%2fcli"
            } else {
                "opencode-ai"
            };
            (
                format!("https://registry.npmjs.org/{package}/latest"),
                |body| json_field(body, "version"),
            )
        }
        AgentKind::Antigravity => (
            format!(
                "https://antigravity-cli-auto-updater-974169037036.us-central1.run.app/manifests/{}.json",
                agy_platform().ok_or("Antigravity publishes no CLI for this system")?
            ),
            |body| json_field(body, "version"),
        ),
        AgentKind::Qwen | AgentKind::Kimi | AgentKind::Auggie => (
            format!(
                "https://registry.npmjs.org/{}/latest",
                acp::spec(kind)
                    .ok_or("Unknown agent")?
                    .npm_package
                    .replace('/', "%2f")
            ),
            |body| json_field(body, "version"),
        ),
        AgentKind::Api => return Err("API providers have no CLI".into()),
    };
    if let Some((_, _, version)) = LATEST
        .locked()
        .iter()
        .find(|(u, at, _)| *u == url && at.elapsed() < LOOKUP_TTL)
    {
        return Ok(version.clone());
    }
    let failed = |e: &dyn std::fmt::Display| format!("Could not look up the newest release: {e}");
    let http: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(LOOKUP_TIMEOUT))
        .build()
        .into();
    let body = http
        .get(&url)
        .call()
        .and_then(|mut r| r.body_mut().read_to_string())
        .map_err(|e| failed(&e))?;
    let version = read(&body)
        .filter(|v| version_parts(v).is_some())
        .ok_or_else(|| failed(&"the answer named no version"))?;
    let mut cache = LATEST.locked();
    cache.retain(|(u, _, _)| *u != url);
    cache.push((url, Instant::now(), version.clone()));
    Ok(version)
}

/// The release channel Claude Code updates from: `stable` or `latest`. A managed setting wins
/// over the user's own.
fn claude_channel() -> &'static str {
    let managed = if cfg!(target_os = "macos") {
        Some(PathBuf::from(
            "/Library/Application Support/ClaudeCode/managed-settings.json",
        ))
    } else if cfg!(windows) {
        std::env::var_os("ProgramFiles")
            .map(|p| PathBuf::from(p).join(r"ClaudeCode\managed-settings.json"))
    } else {
        Some(PathBuf::from("/etc/claude-code/managed-settings.json"))
    };
    let user = claude::home().map(|h| h.join("settings.json"));
    let channel = [managed, user].into_iter().flatten().find_map(|file| {
        let text = std::fs::read_to_string(file).ok()?;
        let settings: Value = serde_json::from_str(&text).ok()?;
        settings["autoUpdatesChannel"].as_str().map(str::to_owned)
    });
    match channel.as_deref() {
        Some("stable") => "stable",
        _ => "latest",
    }
}

/// The name agy's update server gives this system, such as `linux_amd64`.
fn agy_platform() -> Option<String> {
    let os = match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "darwin",
        "windows" => "windows",
        _ => return None,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => return None,
    };
    let musl = if cfg!(target_env = "musl") {
        "_musl"
    } else {
        ""
    };
    Some(format!("{os}_{arch}{musl}"))
}

/// The CLI version `model` needs, if the table names one. A variant such as `[1m]` counts as
/// its model.
fn model_requirement(kind: AgentKind, model: &str) -> Option<&'static str> {
    MODEL_REQUIREMENTS
        .iter()
        .find(|(k, id, _)| *k == kind && model.starts_with(id))
        .map(|(_, _, min)| *min)
}

/// Marks each of `kind`'s models the installed CLI is too old to run. The CLI is only asked
/// its version when a listed model is in the table.
pub(crate) fn mark_models_needing_update(kind: AgentKind, models: &mut [ModelOption]) {
    let needs = |m: &ModelOption| {
        m.id.as_deref()
            .or(m.default_model.as_deref())
            .and_then(|id| model_requirement(kind, id))
    };
    if !models.iter().any(|m| needs(m).is_some()) {
        return;
    }
    let Some(installed) = cached_version(kind) else {
        return;
    };
    for model in models {
        model.needs_cli = needs(model)
            .filter(|min| is_older(&installed, min))
            .map(str::to_owned);
    }
}

/// `kind`'s CLI version, asked again only once its binary changes, as an update changes it.
fn cached_version(kind: AgentKind) -> Option<String> {
    let bin = agent_binary(kind).ok()?;
    let changed = std::fs::metadata(&bin).and_then(|m| m.modified()).ok()?;
    let known = VERSIONS
        .locked()
        .iter()
        .find(|(path, at, _)| *path == bin && *at == changed)
        .map(|(_, _, version)| version.clone());
    if known.is_some() {
        return known;
    }
    let version = cli_version(kind)?;
    let mut versions = VERSIONS.locked();
    versions.retain(|(path, _, _)| *path != bin);
    versions.push((bin, changed, version.clone()));
    Some(version)
}

fn json_field(body: &str, field: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    value[field].as_str().map(str::to_owned)
}

/// `1.5.16` as `[1, 5, 16]`, ignoring a `v` before it and anything after `-` or `+`.
fn version_parts(version: &str) -> Option<Vec<u32>> {
    let number = version.trim().trim_start_matches('v');
    let number = number.split(['-', '+', ' ']).next()?;
    number.split('.').map(|part| part.parse().ok()).collect()
}

/// Whether `version` is older than `than`. Versions that cannot be read are never older.
pub(super) fn is_older(version: &str, than: &str) -> bool {
    match (version_parts(version), version_parts(than)) {
        (Some(have), Some(newest)) => have < newest,
        _ => false,
    }
}

fn file_name(bin: &Path) -> String {
    bin.file_stem().map_or_else(
        || bin.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// The last `count` lines of `text` that say something, joined by spaces.
fn last_lines(text: &str, count: usize) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    lines[lines.len().saturating_sub(count)..].join(" ")
}
