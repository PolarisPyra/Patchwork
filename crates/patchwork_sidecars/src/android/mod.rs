//! Android development from inside Patchwork.
//!
//! Whether a project is a mobile app, where the Android SDK is, the virtual devices and the ones
//! connected, and starting an emulator and running the app on it. The app reaches it through
//! the core's `android()`, which hands [`Android`] the app's settings and state as a [`Host`].
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
