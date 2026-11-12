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
//! run       the framework's own command to run the app, in a terminal
//! grpc      the emulator's gRPC messages and calls the screen uses
//! screen    an emulator's screen streamed into the panel, and input sent back
//! ```
//!
//! Every function here that runs a tool blocks, so the app calls them off its UI thread.

// Rust guideline compliant 2026-10-09

mod avd;
mod detect;
mod devices;
mod emulator;
mod grpc;
mod images;
mod run;
mod screen;
mod sdk;
#[cfg(test)]
mod tests;
mod tool;
mod types;

#[doc(inline)]
pub use avd::{create, profiles, suggest_name};
#[doc(inline)]
pub use detect::{detect, is_marker};
#[doc(inline)]
pub use devices::problem;
#[doc(inline)]
pub use emulator::{Emulators, acceleration};
#[doc(inline)]
pub use images::{available_images, install_image, installed_images};
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
    Avd, Boot, Booting, Device, DeviceState, Framework, HardwareProfile, InstallEvent,
    MobileProject, RunCommand, ScreenFrame, ScreenInput, Sdk, SdkSource, Snapshot, SystemImage,
};
