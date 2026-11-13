//! The Android panel beside the chat: the project's virtual and connected devices, starting and
//! stopping emulators, creating a device, and running the app on the one picked: as a build
//! variant, reloaded while its run stays attached, with its log.
//!
//! It draws what `patchwork_server::android` reads and asks it to act. A running device picked
//! shows its screen in the panel, in a phone's frame.
//!
//! | File | Holds |
//! |---|---|
//! | `state.rs` | The panel's state, the devices as last read, starting and running |
//! | `render.rs` | The panel drawn: header, alerts, device lists, the SDK |
//! | `rows.rs` | A virtual device's row and a connected device's |
//! | `app.rs` | The build variant, reloading and stopping a run, installing an APK |
//! | `log.rs` | The app's log |
//! | `tools.rs` | A device's menu, and the forms for a place, saved states and Wi-Fi pairing |
//! | `screen.rs` | The screen of the device picked |
//! | `create/` | Creating a virtual device |

// Rust guideline compliant 2026-10-09

mod app;
mod create;
mod log;
mod render;
mod rows;
mod screen;
mod state;
mod tools;

pub use state::AndroidPanel;
