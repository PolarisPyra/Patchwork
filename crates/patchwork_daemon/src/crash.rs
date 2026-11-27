//! Crash containment and panic recording for Patchwork processes.

// Rust guideline compliant 2026-10-09

use std::io;
use std::sync::Once;

/// Stops core dumps from writing Patchwork's memory to disk if the process crashes.
///
/// # Errors
///
/// Returns an error when the platform refuses the memory dump restriction.
pub fn harden_process() -> io::Result<()> {
    patchwork_platform::machine::memory::deny_dumps()
}

/// Installs Patchwork's panic hook once, logging each panic to `main.log` before forwarding to
/// the previous panic handler.
pub fn install_panic_hook() {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        let report = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            log::error!(info:%; "panic: {{info}}");
            report(info);
        }));
    });
}
