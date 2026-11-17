//! Locking the vault under what runs: an agent or a server that started with a secret holds
//! it until it ends, so a lock ends them, and an unattended vault locks itself.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::time::Duration;

use crate::vault::{Method, Status, status};
use crate::{Core, Error};

/// How often the vault is checked for having been left alone.
const CHECK_EVERY: Duration = Duration::from_secs(20);

impl Core {
    /// Locks a vault that has a password, and takes its secrets from everything that holds
    /// one: the terminal variables, the MCP servers, the chat apps, and every agent, whose
    /// running replies stop. It opens when the password is entered.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault has no password, or the settings cannot be saved.
    pub fn lock_vault(self: &Arc<Self>) -> Result<(), Error> {
        self.shut_vault()?;
        // An agent keeps the keys it was started with, replying or not.
        for thread_id in self.runner.running() {
            self.stop_turn(&thread_id, false);
        }
        self.runner.retire_all_idle();
        Ok(())
    }

    /// Locks the vault once the window has gone unused for as long as the settings say, for
    /// as long as Patchwork runs. A reply that is running holds the lock off, so work left
    /// to run alone is not stopped.
    pub(crate) fn lock_when_idle(self: &Arc<Self>) {
        let core = Arc::downgrade(self);
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(CHECK_EVERY);
                let Some(core) = core.upgrade() else {
                    return;
                };
                let after = u64::from(core.store.settings().vault.lock_after_mins) * 60_000;
                if after == 0
                    || status() != Status::Open(Method::Password)
                    || core.idle_ms() < after
                    || core.running_turns() > 0
                {
                    continue;
                }
                log::info!("the vault locks: Patchwork's window went unused");
                if let Err(e) = core.lock_vault() {
                    log::warn!(error:% = e; "could not lock the vault: {{error}}");
                }
            }
        });
    }
}
