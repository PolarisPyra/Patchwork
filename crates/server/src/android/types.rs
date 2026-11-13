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

    /// How the framework's running app takes changed code. None when every change is a rebuild.
    #[must_use]
    pub fn reload(self) -> Option<Reload> {
        match self {
            Self::Flutter | Self::Expo => Some(Reload::Typed("r")),
            Self::ReactNative => Some(Reload::DeviceKeys),
            Self::KotlinMultiplatform | Self::NativeAndroid => None,
        }
    }

    /// What is typed into the tool's terminal to start the app again with its state cleared.
    #[must_use]
    pub fn restart(self) -> Option<&'static str> {
        (self == Self::Flutter).then_some("R")
    }

    /// Whether saving the project-relative `path` should reload the running app. React Native
    /// and Expo watch the files themselves; Flutter waits to be told.
    #[must_use]
    pub fn reloads_on_save(self, path: &str) -> bool {
        self == Self::Flutter
            && std::path::Path::new(path)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("dart"))
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

/// One picture of an emulator's screen, scaled to the size asked for, in the BGRA order the app
/// draws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenFrame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// What the screen view sends the emulator. Points are fractions of the picture shown, 0 at its
/// left or top and 1 at its right or bottom.
#[derive(Debug, Clone, PartialEq)]
pub enum ScreenInput {
    /// The pointer pressed, moved while pressed, or released.
    Touch { x: f32, y: f32, down: bool },
    /// The wheel turned by `dy` lines over the point `x`, `y`, positive towards the top of the
    /// page.
    Wheel { x: f32, y: f32, dy: f32 },
    /// A key by its web name, such as `"GoBack"`, `"Enter"` or `"Power"`, pressed and released.
    Key(String),
    /// Text typed.
    Text(String),
    /// The room the picture is drawn in, in pixels, which frames are scaled to fit.
    Fit { width: u32, height: u32 },
}

/// One way the app is built: a product flavor from each dimension the project has, and a build
/// type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    /// Empty for a project without flavors.
    pub flavors: Vec<String>,
    pub build_type: String,
}

impl Default for Variant {
    /// The debug build of a project without flavors.
    fn default() -> Self {
        Self {
            flavors: Vec::new(),
            build_type: "debug".to_string(),
        }
    }
}

impl Variant {
    /// The flavors as Gradle joins them, such as `freeStaging`. None without flavors.
    #[must_use]
    pub fn flavor(&self) -> Option<String> {
        let (first, rest) = self.flavors.split_first()?;
        Some(rest.iter().fold(first.clone(), |joined, flavor| {
            joined + &capitalized(flavor)
        }))
    }

    /// The variant's Gradle name, such as `freeDebug`.
    #[must_use]
    pub fn name(&self) -> String {
        match self.flavor() {
            Some(flavor) => flavor + &capitalized(&self.build_type),
            None => self.build_type.clone(),
        }
    }

    /// The Gradle task that installs the variant, such as `installFreeDebug`.
    pub(super) fn install_task(&self) -> String {
        format!("install{}", capitalized(&self.name()))
    }
}

/// `word` with its first letter in upper case, as Gradle joins names.
fn capitalized(word: &str) -> String {
    let mut letters = word.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

/// What running the app is for, which decides whether the framework's tool stays running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// In a terminal the panel keeps, so the tool stays to reload the code as it changes.
    Attached,
    /// To its end, for an agent that waits for the result: the tool builds, installs, opens the
    /// app and exits.
    Once,
}

/// The device and build a run is for.
#[derive(Debug, Clone, Copy)]
pub struct RunTarget<'a> {
    pub serial: &'a str,
    /// The virtual device an emulator runs.
    pub avd: Option<&'a str>,
    pub variant: &'a Variant,
    pub mode: RunMode,
}

/// How an app that is running takes code that changed since it was built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reload {
    /// The framework's tool reloads when this is typed into the terminal it runs in.
    Typed(&'static str),
    /// The app reloads when the device is sent the R key twice.
    DeviceKeys,
}

/// What is done with a saved state of an emulator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateAction {
    Save,
    Load,
    Delete,
}

/// The mobile network an emulator's connection is slowed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkSpeed {
    Full,
    Lte,
    Hsdpa,
    Umts,
    Edge,
    Gprs,
}

impl NetworkSpeed {
    /// Every speed, the fastest first.
    pub const ALL: [Self; 6] = [
        Self::Full,
        Self::Lte,
        Self::Hsdpa,
        Self::Umts,
        Self::Edge,
        Self::Gprs,
    ];

    /// The speed's name as a phone's status bar shows it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "Full speed",
            Self::Lte => "LTE",
            Self::Hsdpa => "3.5G (HSDPA)",
            Self::Umts => "3G (UMTS)",
            Self::Edge => "2G (EDGE)",
            Self::Gprs => "2G (GPRS)",
        }
    }

    /// The speed's name in the emulator's console.
    pub(super) fn console_name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Lte => "lte",
            Self::Hsdpa => "hsdpa",
            Self::Umts => "umts",
            Self::Edge => "edge",
            Self::Gprs => "gprs",
        }
    }
}

/// A device adb found on the local network, waiting to be paired or connected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiDevice {
    /// The name the device announces itself under.
    pub name: String,
    /// Where it listens, as `192.168.1.20:37251`.
    pub address: String,
    /// Whether it is showing a pairing code, rather than waiting for a paired computer.
    pub pairing: bool,
}

/// How serious a line of the device's log is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Verbose,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

/// One line of the device's log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    /// None for a line that is not an app's, such as one Patchwork adds when the app starts.
    pub level: Option<LogLevel>,
    pub text: String,
}
