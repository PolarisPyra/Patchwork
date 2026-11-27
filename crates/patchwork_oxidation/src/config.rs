//! `oxidation.toml`: what to watch, what to build, and how to run it.
//!
//! Every key is optional. Without a config file, the current folder is the root, every `.rs` and
//! `.toml` file under it is watched, and `cargo build` builds the default workspace members.
//!
//! Platform-specific build settings (such as fast linkers like `wild` or `mold`) can be declared
//! under `[build.targets.<os>]` and are applied only when the configured tools exist on `PATH`.

// Rust guideline compliant 2026-10-09

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;

/// Default configuration filename searched upward from the working directory.
pub const FILE: &str = "oxidation.toml";

/// Complete Oxidation configuration.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// How to build the app.
    pub build: Build,
    /// How to run it.
    pub run: Run,
    /// Which saves trigger a reload, restart, or rebuild.
    pub watch: Watch,
}

impl Config {
    /// Creates a default configuration ready for builder-style customization.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the cargo package to build (`cargo build -p <package>`).
    #[must_use]
    pub fn with_package(mut self, package: impl Into<String>) -> Self {
        self.build.package = Some(package.into());
        self
    }

    /// Sets the binary target to run (`cargo build --bin <bin>`).
    #[must_use]
    pub fn with_bin(mut self, bin: impl Into<String>) -> Self {
        self.build.bin = Some(bin.into());
        self
    }

    /// Enables or disables the `oxidation-client` handoff protocol.
    #[must_use]
    pub fn with_handoff(mut self, handoff: bool) -> Self {
        self.run.handoff = handoff;
        self
    }

    /// Sets a dedicated Cargo target directory relative to the workspace root.
    #[must_use]
    pub fn with_target_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.build.target_dir = Some(dir.into());
        self
    }
}

/// The `[build]` table.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Build {
    /// The package to build, as `cargo build -p` takes it.
    pub package: Option<String>,
    /// The binary to run, when the package has more than one.
    pub bin: Option<String>,
    /// Extra arguments for `cargo build`, such as `--features`.
    pub args: Vec<String>,
    /// A target folder of its own, relative to the root, so editor checks never hold its lock.
    pub target_dir: Option<PathBuf>,
    /// The rustup toolchain to build with, such as `nightly`, instead of the default toolchain.
    pub toolchain: Option<String>,
    /// Optional OS-specific overrides keyed by `std::env::consts::OS` (`linux`, `macos`, `windows`).
    pub targets: BTreeMap<String, TargetBuild>,
}

/// Optional per-OS build customization under `[build.targets.<os>]`.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TargetBuild {
    /// Optional rustup toolchain override for this OS.
    pub toolchain: Option<String>,
    /// Additional arguments appended to `cargo build` on this OS.
    pub args: Vec<String>,
    /// Optional fast linker binary (e.g. `wild`, `mold`, `lld`), enabled only when present on `PATH`.
    pub linker: Option<String>,
    /// Optional C compiler driver (e.g. `clang`) used with `linker`, enabled only when present on `PATH`.
    pub linker_driver: Option<String>,
}

impl Build {
    /// Effective toolchain for the current host OS.
    #[must_use]
    pub fn effective_toolchain(&self) -> Option<&str> {
        self.targets
            .get(std::env::consts::OS)
            .and_then(|target| target.toolchain.as_deref())
            .or(self.toolchain.as_deref())
    }

    /// Effective `cargo build` arguments for the current host OS, injecting configured fast
    /// linkers only when their binaries are actually installed on `PATH`.
    #[must_use]
    pub fn effective_args(&self) -> Vec<String> {
        let mut args = self.args.clone();
        let Some(target) = self.targets.get(std::env::consts::OS) else {
            return args;
        };
        args.extend(target.args.iter().cloned());
        if let Some(linker) = &target.linker {
            let driver = target.linker_driver.as_deref();
            let linker_ok = command_on_path(linker);
            let driver_ok = driver.is_none_or(command_on_path);
            if linker_ok
                && driver_ok
                && let Some(triple) = host_target_triple()
            {
                if let Some(driver) = driver {
                    args.push("--config".to_owned());
                    args.push(format!("target.{triple}.linker=\"{driver}\""));
                }
                args.push("--config".to_owned());
                args.push(format!(
                    "target.{triple}.rustflags=[\"-Clink-arg=--ld-path={linker}\"]"
                ));
            }
        }
        args
    }
}

/// The `[run]` table.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Run {
    /// Arguments for the app, before any given after `--`.
    pub args: Vec<String>,
    /// Variables added to the app's environment.
    pub env: BTreeMap<String, String>,
    /// How long the app has to quit before it is killed, in milliseconds.
    pub stop_within_ms: u64,
    /// Whether the app speaks the `oxidation-client` handoff protocol on standard input.
    pub handoff: bool,
}

impl Default for Run {
    fn default() -> Self {
        Self {
            args: Vec::new(),
            env: BTreeMap::new(),
            stop_within_ms: 2000,
            handoff: false,
        }
    }
}

/// The `[watch]` table.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Watch {
    /// Files and folders to watch, relative to the root.
    pub paths: Vec<PathBuf>,
    /// Extensions of files whose saves trigger a rebuild.
    pub extensions: Vec<String>,
    /// Globs, relative to the root, of paths ignored by the watcher.
    pub ignore: Vec<String>,
    /// Globs, relative to the root, of files that restart the app without rebuilding.
    pub restart: Vec<String>,
    /// Globs, relative to the root, of files sent directly to the running app via `reload`.
    pub send: Vec<String>,
    /// How long saves must settle before a build starts, in milliseconds.
    pub settle_ms: u64,
}

impl Default for Watch {
    fn default() -> Self {
        Self {
            paths: vec![PathBuf::from(".")],
            extensions: vec!["rs".into(), "toml".into()],
            ignore: vec![
                "target/**".into(),
                "**/.git/**".into(),
                "**/node_modules/**".into(),
            ],
            restart: Vec::new(),
            send: Vec::new(),
            settle_ms: 40,
        }
    }
}

/// Reads the nearest `oxidation.toml` at or above `here`, returning the config and its root folder.
///
/// # Errors
///
/// Returns an error if the config file cannot be read or contains invalid TOML.
pub fn load(here: &Path) -> anyhow::Result<(Config, PathBuf)> {
    let Some(root) = here.ancestors().find(|dir| dir.join(FILE).is_file()) else {
        return Ok((Config::default(), here.to_path_buf()));
    };
    load_file(&root.join(FILE))
}

/// Reads an explicit `oxidation.toml` file, returning the parsed config and its parent folder.
///
/// # Errors
///
/// Returns an error if the file cannot be read or contains invalid TOML.
pub fn load_file(path: &Path) -> anyhow::Result<(Config, PathBuf)> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("could not read {}", path.display()))?;
    let config =
        toml::from_str(&text).with_context(|| format!("{} is not valid", path.display()))?;
    let root = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    Ok((config, root))
}

fn command_on_path(binary: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| {
        let candidate = dir.join(binary);
        candidate.is_file() || (cfg!(windows) && dir.join(format!("{binary}.exe")).is_file())
    })
}

fn host_target_triple() -> Option<&'static str> {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("x86_64", "linux") => Some("x86_64-unknown-linux-gnu"),
        ("aarch64", "linux") => Some("aarch64-unknown-linux-gnu"),
        ("x86_64", "macos") => Some("x86_64-apple-darwin"),
        ("aarch64", "macos") => Some("aarch64-apple-darwin"),
        ("x86_64", "windows") => Some("x86_64-pc-windows-msvc"),
        ("aarch64", "windows") => Some("aarch64-pc-windows-msvc"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_platform_target_overrides() {
        let toml_text = r#"
[build]
package = "demo"
args = ["--features", "dynamic"]

[build.targets.linux]
linker = "nonexistent-fast-linker-xyz"
linker_driver = "clang"
args = ["--verbose"]

[run]
handoff = true
"#;
        let config: Config = toml::from_str(toml_text).expect("valid config");
        assert_eq!(config.build.package.as_deref(), Some("demo"));
        assert!(config.run.handoff);
        let args = config.build.effective_args();
        assert!(args.iter().any(|a| a == "dynamic"));
        // Missing linker is gracefully skipped rather than producing broken cargo flags.
        assert!(
            !args
                .iter()
                .any(|a| a.contains("nonexistent-fast-linker-xyz"))
        );
    }

    #[test]
    fn builder_helpers_populate_config() {
        let config = Config::new()
            .with_package("patchwork")
            .with_bin("patchwork")
            .with_handoff(true)
            .with_target_dir("target/oxidation");
        assert_eq!(config.build.package.as_deref(), Some("patchwork"));
        assert_eq!(config.build.bin.as_deref(), Some("patchwork"));
        assert!(config.run.handoff);
        assert_eq!(
            config.build.target_dir.as_deref(),
            Some(Path::new("target/oxidation"))
        );
    }
}
