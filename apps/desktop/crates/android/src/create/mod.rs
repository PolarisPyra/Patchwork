//! Creating a virtual device: a hardware profile, then a system image, which is downloaded first
//! when it is not installed, with its progress and its licence shown.

// Rust guideline compliant 2026-10-09

mod layout;
mod picks;

pub(super) use picks::Create;
