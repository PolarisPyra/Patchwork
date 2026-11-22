//! Locking the vault under what runs: an agent or a server that started with a secret holds
//! it until it ends, so a lock ends them, and an unattended vault locks itself as its timeout
//! says: after a time without use, or when the screen is locked or the computer goes to sleep.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::vault::{Method, Status, VaultTimeout, status};
use crate::{Core, Error};

/// How often the vault is checked for having been left alone.
const CHECK_EVERY: Duration = Duration::from_secs(20);

/// The login manager's name on the system bus.
#[cfg(target_os = "linux")]
const LOGIN: &str = "org.freedesktop.login1";

impl Core {
    /// Stops every warm idle agent process so later turns start with current credentials.
    pub fn retire_idle_agents(&self) {
        self.runner.retire_all_idle();
    }

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
        for chat_id in self.runner.running() {
            self.stop_turn(&chat_id, false);
        }
        self.runner.retire_all_idle();
        Ok(())
    }

    /// Locks the vault once it is left alone, for as long as Patchwork runs: when the window
    /// has gone unused for as long as the settings say, and when the screen is locked or the
    /// computer goes to sleep. A reply that is running holds the lock off until it ends, so
    /// work left to run alone is not stopped.
    pub(crate) fn lock_when_idle(self: &Arc<Self>) {
        let left = Arc::new(AtomicBool::new(false));
        #[cfg(target_os = "linux")]
        for bus in [Bus::Login, Bus::Desktop] {
            let (core, left) = (Arc::downgrade(self), Arc::clone(&left));
            std::thread::spawn(move || {
                if let Err(e) = watch(bus, &core, &left) {
                    log::warn!(
                        bus:? = bus, error:% = e;
                        "the vault will not lock with the screen: {{error}}"
                    );
                }
            });
        }
        let core = Arc::downgrade(self);
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(CHECK_EVERY);
                let Some(core) = core.upgrade() else {
                    return;
                };
                let after = match core.store.settings().vault.timeout {
                    VaultTimeout::Minutes(mins) => u64::from(mins) * 60_000,
                    _ => 0,
                };
                if after != 0 && core.idle_ms() >= after {
                    core.lock_left("Patchwork's window went unused");
                } else if left.load(Ordering::Relaxed)
                    && core.lock_left("the reply that ran when the screen was locked ended")
                {
                    left.store(false, Ordering::Relaxed);
                }
            }
        });
    }

    /// Locks a vault with a password that nothing is using. Says whether no reply held it off.
    fn lock_left(self: &Arc<Self>, why: &str) -> bool {
        if status() != Status::Open(Method::Password) {
            return true;
        }
        if self.running_turns() > 0 {
            return false;
        }
        log::info!(why; "the vault locks: {{why}}");
        if let Err(e) = self.lock_vault() {
            log::warn!(error:% = e; "could not lock the vault: {{error}}");
        }
        true
    }
}

/// Where the desktop says the person left: the login manager, which knows of sleep and of a
/// session it was told to lock, and the session's own bus, where screen lockers announce
/// themselves.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy)]
enum Bus {
    Login,
    Desktop,
}

/// This session's object at the login manager, as `method` finds it from `arg`.
#[cfg(target_os = "linux")]
fn session_by<A>(
    conn: &zbus::blocking::Connection,
    method: &str,
    arg: &A,
) -> Option<zbus::zvariant::OwnedObjectPath>
where
    A: serde::Serialize + zbus::zvariant::DynamicType,
{
    conn.call_method(
        Some(LOGIN),
        "/org/freedesktop/login1",
        Some("org.freedesktop.login1.Manager"),
        method,
        arg,
    )
    .and_then(|reply| reply.body().deserialize())
    .ok()
}

/// Follows `bus` for as long as Patchwork runs, and locks the vault when the screen is locked
/// or the computer goes to sleep. With a reply running, `left` is set, and the vault locks
/// once the reply ends, unless the screen was unlocked by then.
#[cfg(target_os = "linux")]
fn watch(bus: Bus, core: &std::sync::Weak<Core>, left: &AtomicBool) -> zbus::Result<()> {
    use zbus::blocking::{Connection, MessageIterator};

    let rule = zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal);
    let (conn, rule, session) = match bus {
        Bus::Login => {
            let conn = Connection::system()?;
            // This session's object, whose `Lock` and `Unlock` are the only ones to follow.
            // A desktop that starts apps outside the session still names it to them.
            let session = session_by(&conn, "GetSessionByPID", &std::process::id()).or_else(|| {
                session_by(&conn, "GetSession", &std::env::var("XDG_SESSION_ID").ok()?)
            });
            (conn, rule.sender(LOGIN)?, session)
        }
        Bus::Desktop => (Connection::session()?, rule.member("ActiveChanged")?, None),
    };
    for message in MessageIterator::for_match_rule(rule.build(), &conn, Some(16))? {
        let message = message?;
        let header = message.header();
        let (interface, member) = (
            header.interface().map_or("", |i| i.as_str()),
            header.member().map_or("", |m| m.as_str()),
        );
        let ours = header.path().map(zbus::zvariant::ObjectPath::as_str)
            == session.as_deref().map(zbus::zvariant::ObjectPath::as_str);
        let gone = match (interface, member) {
            ("org.freedesktop.login1.Manager", "PrepareForSleep") => {
                // On waking, the vault stays as the sleep left it.
                if !message.body().deserialize::<bool>()? {
                    continue;
                }
                true
            }
            ("org.freedesktop.login1.Session", "Lock") if ours => true,
            ("org.freedesktop.login1.Session", "Unlock") if ours => false,
            (locker, "ActiveChanged") if locker.ends_with(".ScreenSaver") => {
                message.body().deserialize::<bool>()?
            }
            _ => continue,
        };
        let Some(core) = core.upgrade() else {
            return Ok(());
        };
        let held_off = gone
            && core.store.settings().vault.timeout == VaultTimeout::SystemLock
            && !core.lock_left("the screen was locked or the computer went to sleep");
        left.store(held_off, Ordering::Relaxed);
    }
    Ok(())
}
