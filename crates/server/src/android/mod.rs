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
//! ```
//!
//! The emulator shows in its own window. What the panel draws is a [`Snapshot`], so a live view
//! of a device's screen is one more thing a snapshot's device can carry later.
//!
//! Every function here that runs a tool blocks, so the app calls them off its UI thread.

// Rust guideline compliant 2026-10-09

mod avd;
mod detect;
mod devices;
mod emulator;
mod images;
mod run;
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
pub use sdk::{abi_for, host_abi, locate, source_label};
#[doc(inline)]
pub use tool::missing_tools;
#[doc(inline)]
pub use types::{
    Avd, Boot, Booting, Device, DeviceState, Framework, HardwareProfile, InstallEvent,
    MobileProject, RunCommand, Sdk, SdkSource, Snapshot, SystemImage,
};
