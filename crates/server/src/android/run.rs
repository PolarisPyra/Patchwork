//! Building, installing and running the project on a device with the framework's own tool, in one
//! of Patchwork's terminals, and the panel's picture of the SDK and devices.
//!
//! The device is picked with `ANDROID_SERIAL`, which adb, Gradle's install tasks and the React
//! Native and Expo tools all read, so the command is the one a person would type.

// Rust guideline compliant 2026-10-09

use std::path::Path;

use super::types::{Framework, MobileProject, RunCommand, Sdk, Snapshot};
use super::{avd, devices};
use crate::terminal::Launch;
use crate::{Error, Server};

/// Where the last device picked for a project is kept in the app's state, by project id.
const DEVICE_KEY: &str = "android-device:";

/// The command that runs `project` on the device `serial`, written for `shell`. `avd` is the
/// virtual device an emulator runs.
///
/// Flutter, React Native and Expo run through their own tools. Expo is told the device by the
/// virtual device's name, which every version of its tool matches, and a phone by its serial. A
/// Gradle project installs its debug build with the wrapper when it has one, then opens the app
/// when its id can be read from the module.
#[must_use]
pub fn run_command(
    project: &MobileProject,
    serial: &str,
    avd: Option<&str>,
    shell: &str,
) -> RunCommand {
    let line = match project.framework {
        Framework::Flutter => format!("flutter run -d {serial}"),
        Framework::ReactNative => "npx react-native run-android".to_string(),
        Framework::Expo => {
            let device = avd.unwrap_or(serial);
            // Only what a device name or serial is made of goes on a command line.
            let plain = !device.is_empty()
                && device
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':'));
            if plain {
                format!("npx expo run:android --device {device}")
            } else {
                "npx expo run:android".to_string()
            }
        }
        Framework::KotlinMultiplatform | Framework::NativeAndroid => gradle(project, shell),
    };
    RunCommand {
        line,
        cwd: project.root.clone(),
    }
}

fn gradle(project: &MobileProject, shell: &str) -> String {
    let shell = crate::system::shell_name(shell);
    let powershell = matches!(shell.as_str(), "pwsh" | "powershell");
    let root = &project.root;
    let wrapper = if shell == "cmd" && root.join("gradlew.bat").is_file() {
        "gradlew.bat"
    } else if powershell && root.join("gradlew.bat").is_file() {
        r".\gradlew.bat"
    } else if !powershell && shell != "cmd" && root.join("gradlew").is_file() {
        "./gradlew"
    } else {
        "gradle"
    };
    let task = match &project.module {
        Some(module) => format!(":{module}:installDebug"),
        None => "installDebug".to_string(),
    };
    let install = format!("{wrapper} {task}");
    let Some(id) = application_id(project) else {
        return install;
    };
    let open = format!("adb shell monkey -p {id} -c android.intent.category.LAUNCHER 1");
    // Windows PowerShell 5 has no `&&`.
    if powershell {
        format!("{install}; if ($?) {{ {open} }}")
    } else {
        format!("{install} && {open}")
    }
}

/// The id the debug build is installed under, read from the app's module.
///
/// It is the `applicationId` of the module's build file, else its `namespace`, else the
/// `package` of its manifest, which is where builds older than the namespace kept it; then the
/// `applicationIdSuffix` of the `debug` build type when the build file sets one. An id computed
/// in the build file rather than written in it is not found, and the app is then only installed.
fn application_id(project: &MobileProject) -> Option<String> {
    let mut folder = project.root.clone();
    for part in project.module.iter().flat_map(|module| module.split(':')) {
        folder.push(part);
    }
    let build = std::fs::read_to_string(folder.join("build.gradle.kts"))
        .or_else(|_| std::fs::read_to_string(folder.join("build.gradle")))
        .ok()?;
    let id = quoted_after(&build, "applicationId")
        .or_else(|| quoted_after(&build, "namespace"))
        .or_else(|| {
            // A Kotlin Multiplatform module keeps its Android sources in `androidMain`.
            ["src/main", "src/androidMain"].iter().find_map(|sources| {
                let manifest =
                    std::fs::read_to_string(folder.join(sources).join("AndroidManifest.xml"))
                        .ok()?;
                quoted_after(&manifest, "package=")
            })
        })?;
    // The suffix is the debug build type's when it comes before the end of a block named
    // `debug`; a signing config of that name has none, so every such block is looked in.
    let suffix = build
        .match_indices("debug")
        .find_map(|(at, _)| {
            let block = &build[at..];
            let end = block.find('}').unwrap_or(block.len());
            quoted_after(&block[..end], "applicationIdSuffix")
        })
        .unwrap_or_default();
    let id = format!("{id}{suffix}");
    // Only what a package name is made of goes on a command line.
    (id.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_')))
    .then_some(id)
}

/// The quoted value on the first line of `text` that starts with `key`, as in
/// `applicationId = "com.example.shop"`. None when the value is not a literal.
fn quoted_after(text: &str, key: &str) -> Option<String> {
    text.lines().map(str::trim).find_map(|line| {
        let rest = match line.strip_prefix(key) {
            Some(rest) => rest,
            // A manifest's attribute may share its line with others.
            None if key.ends_with('=') => &line[line.find(key)? + key.len()..],
            None => return None,
        };
        // `applicationIdSuffix` starts with `applicationId` and is another setting.
        if rest.starts_with(|c: char| c.is_ascii_alphanumeric()) {
            return None;
        }
        let value = rest.split(['"', '\'']).nth(1)?;
        (!value.is_empty()).then(|| value.to_string())
    })
}

impl Server {
    /// The SDK, the virtual devices, and the running emulators and connected devices, as they
    /// are now. It runs adb, so call it off the UI thread.
    pub fn android_snapshot(&self, project: &Path) -> Snapshot {
        let sdk = self.android_sdk(project);
        let Ok(found) = &sdk else {
            return Snapshot {
                sdk,
                avds: Vec::new(),
                devices: Ok(Vec::new()),
            };
        };
        self.emulators.prune(&self.store);
        Snapshot {
            avds: avd::list(&found.avd_home),
            devices: devices::list(found, &self.emulators),
            sdk,
        }
    }

    /// Keeps the SDK folder the user typed, or forgets it when `path` is empty.
    ///
    /// # Errors
    ///
    /// Returns an error when the folder does not exist, or the settings cannot be saved.
    pub fn set_android_sdk(&self, path: &str) -> Result<(), Error> {
        let path = path.trim();
        let kept = (!path.is_empty()).then(|| path.to_string());
        if let Some(path) = &kept {
            let home = crate::system::fs::home_dir().unwrap_or_default();
            let folder = match path.strip_prefix("~/") {
                Some(rest) => home.join(rest),
                None => path.into(),
            };
            if !folder.is_dir() {
                return Err(format!("{path} is not a folder. Enter the folder that holds platform-tools and emulator.").into());
            }
        }
        self.store
            .update_settings(|settings| settings.android_sdk = kept)
    }

    /// The device last picked for a project: a virtual device's name, or a serial.
    pub fn android_device(&self, project_id: &str) -> Option<String> {
        self.store.state(&format!("{DEVICE_KEY}{project_id}"))
    }

    /// Remembers the device picked for a project.
    ///
    /// # Errors
    ///
    /// Returns an error when the app's state cannot be saved.
    pub fn set_android_device(&self, project_id: &str, device: &str) -> Result<(), Error> {
        self.store
            .set_state(&format!("{DEVICE_KEY}{project_id}"), device)
    }

    /// The terminal that runs `project` on the device `serial`, and the command it runs. `avd` is
    /// the virtual device an emulator runs.
    ///
    /// The command runs in the shell that runs agents' commands, in the project's folder, with
    /// the user's own environment, the device picked, and the SDK's tools on the PATH.
    ///
    /// # Errors
    ///
    /// Returns an error when no SDK or no shell is found, or the framework's tool is not installed.
    pub fn android_run(
        &self,
        project: &MobileProject,
        serial: &str,
        avd: Option<&str>,
    ) -> Result<(Launch, RunCommand), Error> {
        let sdk: Sdk = self.android_sdk(&project.root)?;
        let shell =
            crate::system::command_shell().ok_or("No shell was found to run the build in.")?;
        let command = run_command(project, serial, avd, &shell);
        // The command's first word is the framework's tool; a wrapper in the project is a path.
        let program = command.line.split_whitespace().next().unwrap_or_default();
        let in_project = program.contains(['/', '\\']) || project.root.join(program).is_file();
        if !in_project && crate::system::find_program(program).is_none() {
            return Err(format!(
                "{program} was not found on the PATH, so {} can't be built. Install it, restart \
                 Patchwork so it reads the new PATH, then run again.",
                project.framework.label()
            )
            .into());
        }
        let mut launch =
            crate::terminal::launch(&self.store.settings(), Some(&project.root), None)?;
        launch.args = crate::system::shell_args(&shell, &command.line);
        launch.shell = shell;
        launch.cwd.clone_from(&command.cwd);
        let root = sdk.root.to_string_lossy().into_owned();
        launch
            .env
            .insert("ANDROID_SERIAL".into(), serial.to_string());
        launch.env.insert("ANDROID_HOME".into(), root.clone());
        launch.env.insert("ANDROID_SDK_ROOT".into(), root);
        // Windows names the variable `Path`.
        let key = launch
            .env
            .keys()
            .find(|key| key.eq_ignore_ascii_case("PATH"))
            .cloned()
            .unwrap_or_else(|| "PATH".into());
        let tools = [sdk.adb.as_deref(), sdk.emulator.as_deref()]
            .into_iter()
            .flatten()
            .filter_map(Path::parent)
            .map(Path::to_path_buf);
        let existing = launch.env.get(&key).cloned().unwrap_or_default();
        if let Ok(joined) = std::env::join_paths(tools.chain(std::env::split_paths(&existing))) {
            launch
                .env
                .insert(key, joined.to_string_lossy().into_owned());
        }
        Ok((launch, command))
    }
}
