//! Installing an agent CLI the way its makers publish it.
//!
//! On macOS and Linux each CLI comes from its official shell script, and on Windows from its
//! official PowerShell script. OpenCode publishes no PowerShell script, so on Windows its release
//! archive is downloaded and unpacked where its shell script would have put it. Each CLI lands in
//! the folder its installer picks by default, which Patchwork already searches.

// Rust guideline compliant 2026-10-09

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::provider::AgentKind;
use crate::{Error, Server};

/// How long reaching a download server may take. The download itself is not timed, because the
/// OpenCode archive is about 80 MB.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// OpenCode's newest version, which its npm package publishes before the release archives exist.
const OPENCODE_VERSION_URL: &str = "https://registry.npmjs.org/opencode-ai/latest";

/// Where OpenCode's release archives are, by version tag.
const OPENCODE_RELEASES_URL: &str = "https://github.com/anomalyco/opencode/releases/download";

/// How a CLI is installed.
enum Installer {
    /// An official install script, saved under `name` and run by `shell`.
    Script {
        url: &'static str,
        shell: &'static str,
        name: &'static str,
    },
    /// OpenCode's release archive, for Windows, where it has no install script.
    OpenCodeArchive,
}

impl Server {
    /// Downloads `kind`'s CLI with its official installer and runs it without questions.
    ///
    /// A script is saved before it runs, so the file that runs is the one that was fetched. Codex
    /// and OpenCode are kept from editing the shell profile; Antigravity's script always adds its
    /// folder to it. On Windows the Codex and Antigravity installers add their folder to the
    /// user's PATH, as they do when run by hand.
    ///
    /// # Errors
    ///
    /// Returns an error when the agent has no CLI, the home or app data folder is unavailable, the
    /// installer cannot be downloaded or run, or it finishes without leaving the CLI in place.
    pub fn install_cli(&self, kind: AgentKind) -> Result<(), Error> {
        let home = crate::system::fs::home_dir().ok_or("Could not find the home folder")?;
        let (installer, bin) = plan(kind, &home)?;
        std::fs::create_dir_all(&self.installers_dir)
            .map_err(|e| format!("Could not make {}: {e}", self.installers_dir.display()))?;
        let http: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .build()
            .into();
        match installer {
            Installer::Script { url, shell, name } => {
                run_script(&http, &self.installers_dir, kind, url, shell, name, &home)?;
            }
            Installer::OpenCodeArchive => {
                install_opencode_archive(&http, &self.installers_dir, &bin)?;
            }
        }
        if !bin.is_file() {
            return Err(format!(
                "The installer finished, but the CLI is not at {}",
                bin.display()
            )
            .into());
        }
        log::info!(agent:? = kind, path:% = bin.display(); "installed {{agent}} at {{path}}");
        Ok(())
    }
}

/// How `kind` is installed on this system, and where its CLI is once it is.
fn plan(kind: AgentKind, home: &Path) -> Result<(Installer, PathBuf), Error> {
    let windows = cfg!(windows);
    let local_app_data = || {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or("Could not find the local app data folder")
    };
    let script = |url, shell, name| Installer::Script { url, shell, name };
    Ok(match kind {
        AgentKind::Claude if windows => (
            script("https://claude.ai/install.ps1", "powershell", "claude"),
            home.join(r".local\bin\claude.exe"),
        ),
        AgentKind::Claude => (
            script("https://claude.ai/install.sh", "bash", "claude"),
            home.join(".local/bin/claude"),
        ),
        AgentKind::Codex if windows => (
            script(
                "https://chatgpt.com/codex/install.ps1",
                "powershell",
                "codex",
            ),
            local_app_data()?.join(r"Programs\OpenAI\Codex\bin\codex.exe"),
        ),
        AgentKind::Codex => (
            script("https://chatgpt.com/codex/install.sh", "sh", "codex"),
            home.join(".local/bin/codex"),
        ),
        AgentKind::Antigravity if windows => (
            script(
                "https://antigravity.google/cli/install.ps1",
                "powershell",
                "antigravity",
            ),
            local_app_data()?.join(r"agy\bin\agy.exe"),
        ),
        AgentKind::Antigravity => (
            script(
                "https://antigravity.google/cli/install.sh",
                "bash",
                "antigravity",
            ),
            home.join(".local/bin/agy"),
        ),
        AgentKind::OpenCode if windows => (
            Installer::OpenCodeArchive,
            home.join(r".opencode\bin\opencode.exe"),
        ),
        AgentKind::OpenCode => (
            script("https://opencode.ai/install", "bash", "opencode"),
            home.join(".opencode/bin/opencode"),
        ),
        AgentKind::Api => return Err("API providers have no CLI to install".into()),
    })
}

/// Saves the install script at `url` in `dir`, runs it with `shell`, then deletes it.
fn run_script(
    http: &ureq::Agent,
    dir: &Path,
    kind: AgentKind,
    url: &str,
    shell: &str,
    name: &str,
    home: &Path,
) -> Result<(), Error> {
    let windows = cfg!(windows);
    let script = dir.join(format!(
        "install-{name}.{}",
        if windows { "ps1" } else { "sh" }
    ));
    download(http, url, &script)?;
    let mut cmd = crate::system::host_command(shell);
    if windows {
        cmd.args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ]);
    }
    cmd.arg(&script);
    match kind {
        AgentKind::Codex => {
            // On macOS and Linux the script adds its folder to the shell profile unless it is on
            // PATH already.
            if !windows {
                let mut paths = vec![home.join(".local/bin")];
                if let Some(existing) = std::env::var_os("PATH") {
                    paths.extend(std::env::split_paths(&existing));
                }
                if let Ok(joined) = std::env::join_paths(paths) {
                    cmd.env("PATH", joined);
                }
            }
            cmd.env("CODEX_NON_INTERACTIVE", "1");
        }
        AgentKind::OpenCode => {
            cmd.arg("--no-modify-path");
        }
        AgentKind::Claude | AgentKind::Antigravity | AgentKind::Api => {}
    }
    let run = cmd.output();
    let _ = std::fs::remove_file(&script);
    let run = run.map_err(|e| format!("Could not run {shell}: {e}"))?;
    if run.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&run.stderr);
    let stdout = String::from_utf8_lossy(&run.stdout);
    let said = if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    };
    Err(format!("The installer stopped: {}", last_lines(&said, 3)).into())
}

/// Downloads OpenCode's newest Windows release and unpacks `opencode.exe` to `bin`.
fn install_opencode_archive(http: &ureq::Agent, dir: &Path, bin: &Path) -> Result<(), Error> {
    let latest: serde_json::Value = http
        .get(OPENCODE_VERSION_URL)
        .call()
        .and_then(|mut r| r.body_mut().read_json())
        .map_err(|e| format!("Could not look up OpenCode's newest version: {e}"))?;
    // npm's `latest` is OpenCode 1, the line `https://opencode.ai/install` installs. OpenCode 2 has
    // its own installer and is not on npm.
    let version = latest["version"]
        .as_str()
        .filter(|v| v.starts_with("1."))
        .ok_or("Could not look up OpenCode's newest version")?;
    let target = if has_avx2() {
        "windows-x64"
    } else {
        "windows-x64-baseline"
    };
    let archive = dir.join(format!("opencode-{target}.zip"));
    let url = format!("{OPENCODE_RELEASES_URL}/v{version}/opencode-{target}.zip");
    let unpacked =
        download(http, &url, &archive).and_then(|()| unpack(&archive, "opencode.exe", bin));
    let _ = std::fs::remove_file(&archive);
    unpacked
}

/// Whether this processor runs OpenCode's main build, which needs AVX2. Other processors get its
/// baseline build.
fn has_avx2() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::arch::is_x86_feature_detected!("avx2")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// Writes what `url` serves to `to`.
fn download(http: &ureq::Agent, url: &str, to: &Path) -> Result<(), Error> {
    let response = http
        .get(url)
        .call()
        .map_err(|e| format!("Could not download {url}: {e}"))?;
    let mut file =
        File::create(to).map_err(|e| format!("Could not write {}: {e}", to.display()))?;
    std::io::copy(&mut response.into_body().into_reader(), &mut file)
        .map_err(|e| format!("Could not download {url}: {e}"))?;
    Ok(())
}

/// Unpacks the file `name` from the zip `archive` to `to`, making its folder first.
///
/// It is written beside `to` and renamed into place, so a stopped unpack leaves no broken CLI.
fn unpack(archive: &Path, name: &str, to: &Path) -> Result<(), Error> {
    let broken = |e: &dyn std::fmt::Display| format!("Could not unpack {}: {e}", archive.display());
    let file = File::open(archive).map_err(|e| broken(&e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| broken(&e))?;
    let mut entry = zip.by_name(name).map_err(|e| broken(&e))?;
    if let Some(folder) = to.parent() {
        std::fs::create_dir_all(folder)
            .map_err(|e| format!("Could not make {}: {e}", folder.display()))?;
    }
    let part = to.with_extension("part");
    let written = File::create(&part)
        .and_then(|mut out| std::io::copy(&mut entry, &mut out))
        .and_then(|_| std::fs::rename(&part, to));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&part);
        return Err(format!("Could not write {}: {e}", to.display()).into());
    }
    Ok(())
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
