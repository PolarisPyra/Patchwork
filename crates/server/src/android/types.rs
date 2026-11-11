//! What the Android panel is sent: the mobile project, the SDK and its tools, the virtual and
//! connected devices, and the system images and hardware profiles a new device is made from.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;

use crate::Error;

/// The frameworks a mobile project is built with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framework {
    Flutter,
    KotlinMultiplatform,
    NativeAndroid,
    ReactNative,
    Expo,
}

impl Framework {
    /// The framework's name as its own documentation writes it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Flutter => "Flutter",
            Self::KotlinMultiplatform => "Kotlin Multiplatform",
            Self::NativeAndroid => "Android",
            Self::ReactNative => "React Native",
            Self::Expo => "Expo",
        }
    }
}

/// A mobile app found in a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobileProject {
    pub framework: Framework,
    /// The folder its build runs in: the project's, or one inside it in a monorepo.
    pub root: PathBuf,
    /// The Gradle module that is the app, such as `app`. None when the root project is the app,
    /// and for frameworks whose own tool finds it.
    pub module: Option<String>,
}

/// Where the SDK's folder was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdkSource {
    Settings,
    AndroidHome,
    AndroidSdkRoot,
    LocalProperties,
    PlatformDefault,
}

/// The Android SDK, and the tools in it that are installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sdk {
    pub root: PathBuf,
    pub source: SdkSource,
    pub emulator: Option<PathBuf>,
    pub adb: Option<PathBuf>,
    pub avdmanager: Option<PathBuf>,
    pub sdkmanager: Option<PathBuf>,
    /// The folder virtual devices are defined in.
    pub avd_home: PathBuf,
}

/// A virtual device, as its definition describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Avd {
    /// The name the emulator starts it by.
    pub name: String,
    pub display_name: String,
    /// Its API level as the SDK writes it, such as `34` or `37.0`.
    pub api: String,
    /// The Android version of that level, such as `14`, when it is a released one.
    pub android_version: Option<String>,
    pub abi: String,
    /// What its system image comes with: `AOSP`, `Google APIs` or `Play Store`.
    pub variant: String,
}

/// What adb says of a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceState {
    Online,
    /// Connected, and not answering yet: an emulator still starting.
    Offline,
    /// The device has not allowed this computer to debug it.
    Unauthorized,
    /// The USB device can't be opened by this user.
    NoPermissions,
    Other(String),
}

/// A running emulator or a connected device, as `adb devices` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub serial: String,
    pub state: DeviceState,
    /// The virtual device an emulator runs. None for a physical device.
    pub avd: Option<String>,
    pub emulator: bool,
    pub model: Option<String>,
    /// Patchwork started this emulator, so it may stop it.
    pub ours: bool,
}

/// A system image: installed, or offered for download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemImage {
    /// Its package as `sdkmanager` names it, which is what installs it.
    pub package: String,
    pub api: String,
    pub android_version: Option<String>,
    /// `default`, `google_apis`, `google_apis_playstore`, …
    pub tag: String,
    pub abi: String,
    pub installed: bool,
}

impl SystemImage {
    /// The package as `avdmanager` takes it, with `;` between its parts.
    #[must_use]
    pub fn avd_package(&self) -> String {
        format!(
            "system-images;android-{};{};{}",
            self.api, self.tag, self.abi
        )
    }

    /// Whether `other` is this image, however each tool wrote its package.
    #[must_use]
    pub fn same(&self, other: &Self) -> bool {
        (&self.api, &self.tag, &self.abi) == (&other.api, &other.tag, &other.abi)
    }

    /// What the image comes with: `AOSP`, `Google APIs` or `Play Store`.
    #[must_use]
    pub fn variant(&self) -> &'static str {
        variant(&self.tag)
    }
}

/// What a system image tag comes with, for a person to read.
pub(super) fn variant(tag: &str) -> &'static str {
    if tag.starts_with("google_apis_playstore") {
        "Play Store"
    } else if tag.starts_with("google_apis") {
        "Google APIs"
    } else {
        "AOSP"
    }
}

/// A device definition a virtual device takes its screen and hardware from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardwareProfile {
    pub id: String,
    pub name: String,
    pub oem: String,
}

/// How an emulator starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boot {
    /// From its saved snapshot when it has one.
    Quick,
    /// As a device that was switched off, without loading the snapshot.
    Cold,
    /// With its data erased, as a new device.
    Wipe,
}

/// An emulator that was started and has yet to finish booting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Booting {
    pub avd: String,
    pub serial: String,
}

/// The panel's whole picture at one moment.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// The SDK, or why there is none to use.
    pub sdk: Result<Sdk, Error>,
    pub avds: Vec<Avd>,
    /// The running emulators and connected devices, or why adb could not list them.
    pub devices: Result<Vec<Device>, Error>,
}

/// What a system image's download reports while it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallEvent {
    /// How far the step under way is, from 0 to 100, and what it is doing.
    Progress { percent: u8, step: String },
    /// The licence the download waits on, to be accepted or declined.
    License { text: String },
}

/// The command that builds, installs and runs a project on a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunCommand {
    /// The command line, as the person would type it in `cwd`.
    pub line: String,
    pub cwd: PathBuf,
}
