use std::sync::Arc;
use std::sync::PoisonError;
use std::time::Duration;

use patchwork_server::Server;
use tauri::{AppHandle, Manager};

use super::Updates;

const FIRST_CHECK: Duration = Duration::from_secs(30);
const TICK: Duration = Duration::from_secs(60);

impl Updates {
    fn due(&self, every: Duration) -> bool {
        let last = *self
            .last_check
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        last.is_none_or(|last| last.elapsed().map_or(true, |since| since >= every))
    }
}

fn check_every(app: &AppHandle) -> Duration {
    let minutes = app
        .state::<Arc<Server>>()
        .store
        .settings()
        .updates
        .check_every_minutes;
    Duration::from_secs(u64::from(minutes.clamp(10, 60)) * 60)
}

/// Starts the background checks.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK);
        loop {
            let handle = app.clone();
            if handle.state::<Updates>().due(check_every(&handle)) {
                // In its own thread, so a panic in one check does not end the checks.
                let check =
                    std::thread::spawn(move || handle.state::<Updates>().check(&handle, false));
                if check.join().is_err() {
                    log::warn!("update check stopped");
                }
            }
            std::thread::sleep(TICK);
        }
    });
}
