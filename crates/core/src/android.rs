//! Android development, from the `patchwork-android` crate: the app's settings and state are its
//! [`Host`], and [`Core::android`] is the handle the app calls it through.
//!
//! Setting the SDK folder and opening a run in a terminal stay here, since they write the
//! settings and build a terminal.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;

use crate::terminal::Launch;
use crate::{Core, Error};

#[doc(inline)]
pub use patchwork_android::{
    Android, Avd, Boot, Booting, Device, DeviceState, Emulators, Framework, HardwareProfile, Host,
    InstallEvent, LogLevel, LogLine, Logcat, MobileProject, NetworkSpeed, Reload, RunCommand,
    RunMode, RunTarget, Screen, ScreenFrame, ScreenInput, ScreenSender, Sdk, SdkSource, Snapshot,
    StateAction, SystemImage, TOOL_PREFIX, Variant, WifiDevice, abi_for, acceleration,
    available_images, create, detect, host_abi, install_image, installed_images, is_marker, locate,
    missing_tools, problem, profiles, run_command, source_label, suggest_name, tool_names,
    variants,
};

impl Host for Core {
    fn sdk_setting(&self) -> Option<String> {
        self.store.settings().android_sdk
    }

    fn agents_allowed(&self) -> bool {
        self.store.settings().android_use
    }

    fn state(&self, key: &str) -> Option<String> {
        self.store.state(key)
    }

    fn set_state(&self, key: &str, value: &str) -> Result<(), Error> {
        self.store.set_state(key, value)
    }

    fn thread_project(&self, thread_id: &str) -> Option<(String, PathBuf)> {
        let project_id = self.store.thread(thread_id)?.project_id?;
        let root = self.project_root(&project_id).ok()?;
        Some((project_id, PathBuf::from(root)))
    }
}

impl Core {
    /// The Android tools, acting for this app.
    #[must_use]
    pub fn android(&self) -> Android<'_> {
        Android::new(self, &self.emulators)
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
        self.update_settings(|settings| settings.android_sdk = kept)
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
        let (sdk, shell, command) = self.android().run_plan(project, target)?;
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
        if let Some(path) = patchwork_android::with_sdk_tools(&sdk, &existing) {
            launch.env.insert(key, path);
        }
        Ok((launch, command))
    }
}
