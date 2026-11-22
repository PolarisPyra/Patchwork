//! What other crates draw in the window, as the window drives it: the pages (Quilts, Android,
//! first-run setup) and the dialogs (granular settings, backgrounds, search).

// Rust guideline compliant 2026-10-09

pub(super) mod android;
pub(super) mod background;
pub(super) mod granular;
pub(super) mod onboarding;
pub(super) mod quilts;
pub(super) mod search;
