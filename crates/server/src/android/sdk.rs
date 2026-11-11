//! Finding the Android SDK and its tools without being told where they are, and running them.
//!
//! The SDK's folder is the first of these that exists: the one set in Patchwork's settings,
//! `ANDROID_HOME`, `ANDROID_SDK_ROOT`, `sdk.dir` in the project's `local.properties`, then where
//! Android Studio installs it on this platform. The variables are read as the user's shell sets
//! them, since an app started from the Finder or a desktop launcher has none of them.
//!
//! Discovery is written against a [`Host`], the platform, home folder and environment it runs on,
//! so each platform's conventions can be checked on any of them.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};
use std::process::Stdio;

use super::types::{Sdk, SdkSource};
use crate::Error;

/// The platforms the SDK lays itself out for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Os {
    Windows,
    Mac,
    Linux,
}

impl Os {
    pub(super) fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Mac
        } else {
            Self::Linux
        }
    }
}

/// The machine discovery runs on: its platform, the user's home folder, and their environment.
pub(super) struct Host<'a> {
    pub(super) os: Os,
    pub(super) home: Option<PathBuf>,
    pub(super) var: &'a dyn Fn(&str) -> Option<String>,
}

impl Host<'_> {
    fn var(&self, name: &str) -> Option<String> {
        (self.var)(name).filter(|value| !value.trim().is_empty())
    }
}

/// The SDK tools Patchwork runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tool {
    Emulator,
    Adb,
    AvdManager,
    SdkManager,
}

/// Where the SDK may be, in the order the places are tried, each with what named it.
pub(super) fn candidates(
    configured: Option<&str>,
    local_properties: Option<&str>,
    host: &Host,
) -> Vec<(SdkSource, PathBuf)> {
    let mut found = Vec::new();
    if let Some(path) = configured.map(str::trim).filter(|path| !path.is_empty()) {
        found.push((SdkSource::Settings, expand_home(path, host)));
    }
    if let Some(path) = host.var("ANDROID_HOME") {
        found.push((SdkSource::AndroidHome, PathBuf::from(path)));
    }
    if let Some(path) = host.var("ANDROID_SDK_ROOT") {
        found.push((SdkSource::AndroidSdkRoot, PathBuf::from(path)));
    }
    if let Some(path) = local_properties.and_then(sdk_dir) {
        found.push((SdkSource::LocalProperties, PathBuf::from(path)));
    }
    if let Some(path) = default_root(host) {
        found.push((SdkSource::PlatformDefault, path));
    }
    found
}

/// Where Android Studio installs the SDK on the host's platform.
pub(super) fn default_root(host: &Host) -> Option<PathBuf> {
    match host.os {
        Os::Windows => host
            .var("LOCALAPPDATA")
            .map(PathBuf::from)
            .or_else(|| Some(host.home.clone()?.join("AppData").join("Local")))
            .map(|local| local.join("Android").join("Sdk")),
        Os::Mac => Some(
            host.home
                .clone()?
                .join("Library")
                .join("Android")
                .join("sdk"),
        ),
        Os::Linux => Some(host.home.clone()?.join("Android").join("Sdk")),
    }
}

/// A leading `~` in a path the user typed, as their home folder.
fn expand_home(path: &str, host: &Host) -> PathBuf {
    let rest = path
        .strip_prefix("~/")
        .or_else(|| path.strip_prefix("~\\"))
        .or_else(|| (path == "~").then_some(""));
    match (rest, &host.home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

/// `sdk.dir` in a `local.properties` file. Android Studio writes it as a Java properties value,
/// in which `\\` is a backslash and `\:` a colon, as in `C\:\\Users\\me\\AppData\\Local\\Android\\Sdk`.
pub(super) fn sdk_dir(local_properties: &str) -> Option<String> {
    let value = local_properties.lines().find_map(|line| {
        let (key, value) = line.trim_start().split_once(['=', ':'])?;
        (key.trim() == "sdk.dir").then(|| value.trim())
    })?;
    let mut path = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => path.extend(chars.next()),
            c => path.push(c),
        }
    }
    (!path.is_empty()).then_some(path)
}

/// Where `tool` is in the SDK at `root`, for a host whose files `exists` answers for.
///
/// `versions` are the folders inside `cmdline-tools`: `latest` is used when it is there, else the
/// newest numbered one, else the `tools/bin` that SDKs older than the command-line tools have.
pub(super) fn tool_path(
    root: &Path,
    tool: Tool,
    os: Os,
    versions: &[String],
    exists: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let windows = os == Os::Windows;
    let folders: Vec<PathBuf> = match tool {
        Tool::Emulator => vec![root.join("emulator")],
        Tool::Adb => vec![root.join("platform-tools")],
        Tool::AvdManager | Tool::SdkManager => {
            let mut numbered: Vec<&String> = versions.iter().filter(|v| *v != "latest").collect();
            numbered.sort_by_key(|version| std::cmp::Reverse(version_key(version)));
            let tools = root.join("cmdline-tools");
            std::iter::once(tools.join("latest"))
                .chain(numbered.into_iter().map(|version| tools.join(version)))
                .chain(std::iter::once(root.join("tools")))
                .map(|folder| folder.join("bin"))
                .collect()
        }
    };
    // The programs are `.exe` files on Windows and the Java tools' launchers `.bat` files.
    let name = match (tool, windows) {
        (Tool::Emulator, true) => "emulator.exe",
        (Tool::Emulator, false) => "emulator",
        (Tool::Adb, true) => "adb.exe",
        (Tool::Adb, false) => "adb",
        (Tool::AvdManager, true) => "avdmanager.bat",
        (Tool::AvdManager, false) => "avdmanager",
        (Tool::SdkManager, true) => "sdkmanager.bat",
        (Tool::SdkManager, false) => "sdkmanager",
    };
    folders
        .into_iter()
        .map(|folder| folder.join(name))
        .find(|path| exists(path))
}

/// A version folder's numbers, so `11.0` sorts after `9.0`.
fn version_key(version: &str) -> Vec<u32> {
    version
        .split(['.', '-'])
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// The folder virtual devices are defined in: `ANDROID_AVD_HOME`, else `avd` in the folder
/// `ANDROID_USER_HOME` names, else `~/.android/avd`.
pub(super) fn avd_home(host: &Host) -> PathBuf {
    if let Some(path) = host.var("ANDROID_AVD_HOME") {
        return PathBuf::from(path);
    }
    host.var("ANDROID_USER_HOME")
        .map(PathBuf::from)
        .or_else(|| Some(host.home.clone()?.join(".android")))
        .unwrap_or_else(|| PathBuf::from(".android"))
        .join("avd")
}

/// The system image ABI that runs at full speed on a machine of architecture `arch`, as
/// `std::env::consts::ARCH` names it: `arm64-v8a` on Apple Silicon and ARM Linux and Windows,
/// `x86_64` elsewhere.
#[must_use]
pub fn abi_for(arch: &str) -> &'static str {
    if arch == "aarch64" {
        "arm64-v8a"
    } else {
        "x86_64"
    }
}

/// The system image ABI for this machine.
///
/// An Intel build of Patchwork on Apple Silicon runs translated and reports `x86_64`, so macOS is
/// asked whether that is so.
#[must_use]
pub fn host_abi() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        let translated = crate::system::host_command("sysctl")
            .args(["-n", "sysctl.proc_translated"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == "1");
        if translated {
            return abi_for("aarch64");
        }
    }
    abi_for(std::env::consts::ARCH)
}

/// Finds the SDK for the project at `project`, with the path set in settings tried first.
///
/// # Errors
///
/// Returns an error naming every place looked in when none of them is a folder.
pub fn locate(configured: Option<&str>, project: Option<&Path>) -> Result<Sdk, Error> {
    let var = |name: &str| crate::system::login_var(name);
    let host = Host {
        os: Os::current(),
        home: crate::system::fs::home_dir(),
        var: &var,
    };
    // Flutter and React Native keep the Gradle project, and its `local.properties`, in `android`.
    let local_properties = project.and_then(|root| {
        std::fs::read_to_string(root.join("local.properties"))
            .or_else(|_| std::fs::read_to_string(root.join("android").join("local.properties")))
            .ok()
    });
    let places = candidates(configured, local_properties.as_deref(), &host);
    let Some((source, root)) = places.iter().find(|(_, path)| path.is_dir()).cloned() else {
        let looked: Vec<String> = places
            .iter()
            .map(|(source, path)| format!("{}: {}", source_label(*source), path.display()))
            .collect();
        return Err(Error::Tool {
            message: "The Android SDK was not found. Install Android Studio, which sets it up on \
                      first launch, or set the SDK's folder here."
                .into(),
            detail: format!("Looked in:\n{}", looked.join("\n")),
        });
    };
    let versions: Vec<String> = std::fs::read_dir(root.join("cmdline-tools"))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let find = |tool| tool_path(&root, tool, host.os, &versions, &|path| path.is_file());
    Ok(Sdk {
        emulator: find(Tool::Emulator),
        // Linux distributions package adb on its own, outside any SDK.
        adb: find(Tool::Adb).or_else(|| crate::system::find_program("adb")),
        avdmanager: find(Tool::AvdManager),
        sdkmanager: find(Tool::SdkManager),
        avd_home: avd_home(&host),
        root,
        source,
    })
}

/// What named an SDK folder, for a person to read.
#[must_use]
pub fn source_label(source: SdkSource) -> &'static str {
    match source {
        SdkSource::Settings => "Patchwork's settings",
        SdkSource::AndroidHome => "ANDROID_HOME",
        SdkSource::AndroidSdkRoot => "ANDROID_SDK_ROOT",
        SdkSource::LocalProperties => "sdk.dir in local.properties",
        SdkSource::PlatformDefault => "Android Studio's default",
    }
}
