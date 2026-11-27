//! Android development from inside Patchwork.
//!
//! Whether a project is a mobile app, where the Android SDK is, the virtual devices and the ones
//! connected, and starting an emulator and running the app on it. The app reaches it through
//! [`Core::android`], which hands [`Android`] the app's settings and state as a [`Host`].
//!
//! ```text
//! sdk/
//!   locate    the SDK's folder and tools, found without configuration on each platform
//!   tool      running a tool as a terminal would, with a time limit
//!   detect    which framework builds the project, from its manifest and build files
//!   gradle    the build variants an app has and the id each installs under
//!   avd       the virtual devices defined, the hardware profiles, creating a device
//!   images    the system images installed and offered, downloading one
//! devices/
//!   connected the running emulators and connected devices adb lists
//!   adb       a picture of any device's screen, touches and keys through adb, emulator controls
//!   logcat    the app's own lines of a device's log, followed live or read once
//!   wifi      pairing with a phone over Wi-Fi and connecting to it
//!   grpc      the emulator's gRPC messages and calls the screen uses
//!   screen    an emulator's screen streamed into the panel, and input sent back
//!   mirror    a phone's screen shown the same way, one picture at a time
//! emulator    starting, stopping and waiting for an emulator; hardware acceleration
//! run         the framework's own command to run the app, in a terminal or to its end
//! host        what the tools read from the app, and the handle the app calls them through
//! agent       the tools a chat's agent uses devices with
//! ```
//!
//! Every function here that runs a tool blocks, so the app calls them off its UI thread.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;

use crate::terminal::Launch;
use crate::{Core, Error};

mod agent;
mod devices;
mod emulator;
mod host;
mod run;
mod sdk;

#[doc(inline)]
pub use agent::{TOOL_PREFIX, tool_names};
#[doc(inline)]
pub use devices::adb::{NetworkSpeed, StateAction};
#[doc(inline)]
pub use devices::connected::{Device, DeviceState, problem};
#[doc(inline)]
pub use devices::logcat::{LogLevel, LogLine, Logcat};
#[doc(inline)]
pub use devices::screen::{Screen, ScreenFrame, ScreenInput, ScreenSender};
#[doc(inline)]
pub use devices::wifi::WifiDevice;
#[doc(inline)]
pub use emulator::{Boot, Booting, Emulators, acceleration};
#[doc(inline)]
pub use host::{Android, Host};
#[doc(inline)]
pub use run::{Reload, RunCommand, RunMode, RunTarget, Snapshot, run_command, with_sdk_tools};
#[doc(inline)]
pub use sdk::avd::{Avd, HardwareProfile, create, profiles, suggest_name};
#[doc(inline)]
pub use sdk::detect::{Framework, MobileProject, detect, is_marker};
#[doc(inline)]
pub use sdk::gradle::{Variant, variants};
#[doc(inline)]
pub use sdk::images::{InstallEvent, SystemImage, available_images, install_image, installed_images};
#[doc(inline)]
pub use sdk::locate::{Sdk, SdkSource, abi_for, host_abi, locate, source_label};
#[doc(inline)]
pub use sdk::tool::missing_tools;

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

    fn chat_project(&self, chat_id: &str) -> Option<(String, PathBuf)> {
        let project_id = self.store.chat(chat_id)?.project_id?;
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
            let home = crate::system::files::paths::home_dir().unwrap_or_default();
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
        if let Some(path) = with_sdk_tools(&sdk, &existing) {
            launch.env.insert(key, path);
        }
        Ok((launch, command))
    }
}
