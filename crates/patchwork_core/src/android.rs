//! Android development from inside Patchwork, as the core hands it its settings and state.
//!
//! The tools themselves are in `patchwork_sidecars::android`; this is the [`Host`] the core is
//! for them, and the core's calls that reach them.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;

pub use patchwork_sidecars::android::*;

use crate::terminal::Launch;
use crate::{Core, Error};

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
        let mut launch = crate::terminal::launch(
            &self.store.settings().terminal,
            Some(&project.root),
            None,
            None,
        )?;
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
