//! Building, installing and running the project on a device with the framework's own tool, in one
//! of Patchwork's terminals, and the panel's picture of the SDK and devices.
//!
//! The device is picked with `ANDROID_SERIAL`, which adb, Gradle's install tasks and the React
//! Native and Expo tools all read, so the command is the one a person would type.

// Rust guideline compliant 2026-10-09

use std::path::Path;

use super::gradle::application_id;
use super::types::{Framework, MobileProject, RunCommand, RunMode, RunTarget, Sdk, Snapshot};
use super::tool::{self, tool_command};
use super::{avd, devices};
use crate::terminal::Launch;
use crate::{Error, Server};

/// Where the last device picked for a project is kept in the app's state, by project id.
const DEVICE_KEY: &str = "android-device:";

/// The command that runs `project` on the device and as the variant of `target`, written for
/// `shell`.
///
/// Flutter, React Native and Expo run through their own tools. Expo is told the device by the
/// virtual device's name, which every version of its tool matches, and a phone by its serial. A
/// Gradle project installs the variant with the wrapper when it has one, then opens the app
/// when its id can be read from the module.
///
/// A run that is [`RunMode::Attached`] leaves Flutter and Expo running to reload the app. One
/// that runs [`RunMode::Once`] has each tool exit when the app is installed, without the server
/// that React Native and Expo load their JavaScript from.
#[must_use]
pub fn run_command(project: &MobileProject, target: &RunTarget, shell: &str) -> RunCommand {
    let variant = target.variant;
    let once = target.mode == RunMode::Once;
    let default = variant.flavors.is_empty() && variant.build_type == "debug";
    let mut words: Vec<String> = Vec::new();
    let mut push = |word: &str| words.push(word.to_string());
    match project.framework {
        Framework::Flutter => {
            push("flutter");
            push("run");
            if plain(target.serial) {
                push("-d");
                push(target.serial);
            }
            if let Some(flavor) = variant.flavor().filter(|flavor| plain(flavor)) {
                push("--flavor");
                push(&flavor);
            }
            if matches!(variant.build_type.as_str(), "profile" | "release") {
                push(&format!("--{}", variant.build_type));
            }
            if once {
                push("--no-resident");
            }
        }
        Framework::ReactNative => {
            push("npx");
            push("react-native");
            push("run-android");
            if !default && plain(&variant.name()) {
                push("--mode");
                push(&variant.name());
            }
            if once {
                push("--no-packager");
            }
        }
        Framework::Expo => {
            push("npx");
            push("expo");
            push("run:android");
            let device = target.avd.unwrap_or(target.serial);
            if plain(device) {
                push("--device");
                push(device);
            }
            if !default && plain(&variant.name()) {
                push("--variant");
                push(&variant.name());
            }
            if once {
                push("--no-bundler");
            }
        }
        Framework::KotlinMultiplatform | Framework::NativeAndroid => {
            return RunCommand {
                line: gradle(project, target, shell),
                cwd: project.root.clone(),
            };
        }
    }
    RunCommand {
        line: words.join(" "),
        cwd: project.root.clone(),
    }
}

/// Whether `word` holds only what a device name, serial or variant is made of, so it can go on
/// a command line as it is.
fn plain(word: &str) -> bool {
    !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':'))
}

fn gradle(project: &MobileProject, target: &RunTarget, shell: &str) -> String {
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
    let install = target.variant.install_task();
    let task = match &project.module {
        Some(module) => format!(":{module}:{install}"),
        None => install,
    };
    let install = format!("{wrapper} {task}");
    let Some(id) = application_id(project, target.variant) else {
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

    /// The SDK, the shell and the command a run of `project` for `target` uses.
    fn run_plan(
        &self,
        project: &MobileProject,
        target: &RunTarget,
    ) -> Result<(Sdk, String, RunCommand), Error> {
        let sdk: Sdk = self.android_sdk(&project.root)?;
        let shell =
            crate::system::command_shell().ok_or("No shell was found to run the build in.")?;
        let command = run_command(project, target, &shell);
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
        Ok((sdk, shell, command))
    }

    /// The terminal that runs `project` for `target`, and the command it runs.
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
        target: &RunTarget,
    ) -> Result<(Launch, RunCommand), Error> {
        let (sdk, shell, command) = self.run_plan(project, target)?;
        let mut launch =
            crate::terminal::launch(&self.store.settings(), Some(&project.root), None, None)?;
        launch.args = crate::system::shell_args(&shell, &command.line);
        launch.shell = shell;
        launch.cwd.clone_from(&command.cwd);
        let root = sdk.root.to_string_lossy().into_owned();
        launch
            .env
            .insert("ANDROID_SERIAL".into(), target.serial.to_string());
        launch.env.insert("ANDROID_HOME".into(), root.clone());
        launch.env.insert("ANDROID_SDK_ROOT".into(), root);
        // Windows names the variable `Path`.
        let key = launch
            .env
            .keys()
            .find(|key| key.eq_ignore_ascii_case("PATH"))
            .cloned()
            .unwrap_or_else(|| "PATH".into());
        let existing = launch.env.get(&key).cloned().unwrap_or_default();
        if let Some(path) = with_sdk_tools(&sdk, &existing) {
            launch.env.insert(key, path);
        }
        Ok((launch, command))
    }

    /// Runs `project` for `target` to its end and returns the end of what the build printed,
    /// for an agent that waits for it. `stopped` says the agent's turn was stopped.
    ///
    /// # Errors
    ///
    /// Returns an error when the run can't start, fails, takes longer than [`ONCE_LIMIT`], or
    /// is stopped.
    pub(super) fn android_run_once(
        &self,
        project: &MobileProject,
        target: &RunTarget,
        stopped: &dyn Fn() -> bool,
    ) -> Result<String, Error> {
        let (sdk, shell, command) = self.run_plan(project, target)?;
        let mut cmd = tool_command(&sdk, Path::new(&shell));
        cmd.args(crate::system::shell_args(&shell, &command.line))
            .current_dir(&command.cwd)
            .env("ANDROID_SERIAL", target.serial);
        let existing = crate::system::search_path()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some(path) = with_sdk_tools(&sdk, &existing) {
            cmd.env("PATH", path);
        }
        let out = tool::output_bytes(cmd, ONCE_LIMIT, stopped)?;
        let printed = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), out.stderr);
        if !out.ok {
            return Err(Error::Tool {
                message: format!("`{}` failed:\n{}", command.line, tool::tail(printed.trim())),
                detail: printed,
            });
        }
        Ok(tool::tail(printed.trim()))
    }
}

/// How long a run an agent waits for may take: a first Gradle build downloads its dependencies.
const ONCE_LIMIT: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// `path` with the folders of the SDK's adb and emulator put first.
fn with_sdk_tools(sdk: &Sdk, path: &str) -> Option<String> {
    let tools = [sdk.adb.as_deref(), sdk.emulator.as_deref()]
        .into_iter()
        .flatten()
        .filter_map(Path::parent)
        .map(Path::to_path_buf);
    let joined = std::env::join_paths(tools.chain(std::env::split_paths(path))).ok()?;
    Some(joined.to_string_lossy().into_owned())
}
