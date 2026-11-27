//! Signals: catching Ctrl-C, and stopping a child with everything it started.
//!
//! On Unix each child leads a process group of its own, so a stop reaches the compilers cargo
//! runs and the helpers the app runs, and Ctrl-C in the terminal reaches only Oxidation, which
//! then stops them in order.

// Rust guideline compliant 2026-10-09

use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static STOPPED: AtomicBool = AtomicBool::new(false);

/// Notes Ctrl-C, a hangup or a terminate instead of dying, for the main loop to see.
#[cfg(unix)]
pub fn catch() {
    extern "C" fn on_signal(_: libc::c_int) {
        STOPPED.store(true, Ordering::SeqCst);
    }
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        #[expect(
            clippy::fn_to_numeric_cast_any,
            reason = "libc takes the handler as an address"
        )]
        let handler = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
        // SAFETY: the handler only stores to an atomic, which is async-signal-safe.
        unsafe {
            libc::signal(signal, handler);
        }
    }
}

/// Leaves the default handling, under which Ctrl-C ends Oxidation and its console's children.
#[cfg(not(unix))]
pub fn catch() {}

/// Whether Oxidation was asked to stop.
pub fn stopped() -> bool {
    STOPPED.load(Ordering::SeqCst)
}

/// Makes the command's child lead a process group of its own.
pub fn own_group(command: &mut Command) -> &mut Command {
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(command, 0);
    command
}

/// Asks the child and its group to quit, then kills them if they're still there after `within`.
pub fn stop(child: &mut Child, within: Duration) {
    if child.try_wait().ok().flatten().is_some() {
        return;
    }
    #[cfg(unix)]
    if ask_group(child, within) {
        return;
    }
    #[cfg(not(unix))]
    let _ = (within, child.kill());
    let _ = child.wait();
}

/// Waits up to `within` for the child to quit. Whether it did.
pub fn wait(child: &mut Child, within: Duration) -> bool {
    let asked = Instant::now();
    loop {
        if child.try_wait().ok().flatten().is_some() {
            return true;
        }
        if asked.elapsed() >= within {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Asks what the child started in its group, and is still running, to stop.
pub fn end_group(child: &Child) {
    #[cfg(unix)]
    signal_group(child, libc::SIGTERM);
    #[cfg(not(unix))]
    let _ = child;
}

/// Sends the group a terminate, and kills it if the child hasn't quit within `within`.
///
/// Whether the child quit when asked.
#[cfg(unix)]
fn ask_group(child: &mut Child, within: Duration) -> bool {
    signal_group(child, libc::SIGTERM);
    if wait(child, within) {
        return true;
    }
    signal_group(child, libc::SIGKILL);
    false
}

#[cfg(unix)]
fn signal_group(child: &Child, signal: libc::c_int) {
    let Ok(pid) = libc::pid_t::try_from(child.id()) else {
        return;
    };
    // SAFETY: `kill` takes any pid and signal; a negative pid names the child's group.
    unsafe {
        libc::kill(-pid, signal);
    }
}
