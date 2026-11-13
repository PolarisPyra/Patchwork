//! Android development from inside Patchwork: whether a project is a mobile app, where the
//! Android SDK is, the virtual devices and the ones connected, and starting an emulator and
//! running the app on it.
//!
//! ```text
//! detect    which framework builds the project, from its manifest and build files
//! sdk       the SDK's folder and tools, found without configuration on each platform
//! tool      running a tool as a terminal would, with a time limit
//! avd       the virtual devices defined, the hardware profiles, creating a device
//! images    the system images installed and offered, downloading one
//! devices   the running emulators and connected devices adb lists
//! emulator  starting, stopping and waiting for an emulator; hardware acceleration
//! gradle    the build variants an app has and the id each installs under
//! run       the framework's own command to run the app, in a terminal or to its end
//! device    a picture of any device's screen, touches and keys through adb, emulator controls
//! logcat    the app's own lines of a device's log, followed live or read once
//! wifi      pairing with a phone over Wi-Fi and connecting to it
//! grpc      the emulator's gRPC messages and calls the screen uses
//! screen    an emulator's screen streamed into the panel, and input sent back
//! mirror    a phone's screen shown the same way, one picture at a time
//! agent     the tools a chat's agent uses devices with
//! ```
//!
//! Every function here that runs a tool blocks, so the app calls them off its UI thread.

// Rust guideline compliant 2026-10-09

mod agent;
mod avd;
mod detect;
mod device;
mod devices;
mod emulator;
mod gradle;
mod grpc;
mod images;
mod logcat;
mod mirror;
mod run;
mod screen;
mod sdk;
#[cfg(test)]
mod tests;
mod tool;
mod types;
mod wifi;

#[doc(inline)]
pub use agent::TOOL_PREFIX;
pub(crate) use agent::tool_names;
#[doc(inline)]
pub use avd::{create, profiles, suggest_name};
#[doc(inline)]
pub use detect::{detect, is_marker};
#[doc(inline)]
pub use devices::problem;
#[doc(inline)]
pub use emulator::{Emulators, acceleration};
#[doc(inline)]
pub use gradle::variants;
#[doc(inline)]
pub use images::{available_images, install_image, installed_images};
#[doc(inline)]
pub use logcat::Logcat;
#[doc(inline)]
pub use run::run_command;
#[doc(inline)]
pub use screen::{Screen, ScreenSender};
#[doc(inline)]
pub use sdk::{abi_for, host_abi, locate, source_label};
#[doc(inline)]
pub use tool::missing_tools;
#[doc(inline)]
pub use types::{
    Avd, Boot, Booting, Device, DeviceState, Framework, HardwareProfile, InstallEvent, LogLevel,
    LogLine, MobileProject, NetworkSpeed, Reload, RunCommand, RunMode, RunTarget, ScreenFrame,
    ScreenInput, Sdk, SdkSource, Snapshot, StateAction, SystemImage, Variant, WifiDevice,
};
