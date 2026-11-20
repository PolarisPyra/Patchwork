//! The running gateway: a connection per enabled chat app, each on its own thread, and the queue
//! of thread events that go back out to them.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use super::notify::Notice;
use super::platform::Platform;
use super::types::{Connector, Incoming, Listener, Status};
use crate::Core;
use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::threads::Message;

/// How long a connection waits before its first retry; each failure doubles it.
const FIRST_RETRY: Duration = Duration::from_secs(5);

/// The longest a connection waits between retries.
const MAX_RETRY: Duration = Duration::from_secs(60);

/// How long a connection waits before opening again after the platform ended it cleanly.
const RECONNECT: Duration = Duration::from_secs(1);

/// How often a waiting connection checks whether it should stop.
const STOP_CHECK: Duration = Duration::from_millis(250);

/// The gateway's connections and what it remembers while Patchwork runs.
#[derive(Default)]
pub struct Hub {
    state: Mutex<State>,
    /// Feeds the thread that sends bound threads' events to their chats, once the gateway has
    /// started.
    events: Mutex<Option<Sender<Out>>>,
    /// Held while the pairings or bindings are read and written back.
    pub(super) records: Mutex<()>,
    /// Held while the connections are stopped and opened, so two restarts cannot overlap.
    pub(super) restarting: Mutex<()>,
}

impl std::fmt::Debug for Hub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hub").finish_non_exhaustive()
    }
}

/// What the gateway remembers while Patchwork runs.
#[derive(Default)]
pub(super) struct State {
    pub links: HashMap<Platform, Link>,
    pub statuses: HashMap<Platform, Status>,
    /// The pairing code Settings shows, and when it stops working, in epoch milliseconds.
    pub code: Option<(String, u64)>,
    /// How many wrong guesses the pairing code has had.
    pub wrong_codes: u32,
    /// Senders already told how to pair, so a stranger hears it once.
    pub told: HashSet<(Platform, String)>,
    pub chats: HashMap<(Platform, String), Chat>,
    /// The conversation each request was answered from, by approval id, until the other chats
    /// are told.
    pub answered: HashMap<String, (Platform, String)>,
    /// The requests sent to chat apps from chats no conversation continues, oldest first.
    pub notices: Vec<Notice>,
    /// The messages taken lately, oldest first, so one a chat app delivers twice is taken once.
    pub seen: VecDeque<Seen>,
    /// The chat each thread's agent is working with this turn, by the thread's id, for the
    /// threads a conversation talks to.
    pub visiting: HashMap<String, String>,
    /// When the conversations of each thread last asked for a reply or got one, by the
    /// thread's id, in epoch milliseconds.
    pub used: HashMap<String, u64>,
    /// The chat each thread's agent last worked with and when, in epoch milliseconds, by the
    /// thread's id. The next turn is told it so that "it" and "that" find it.
    pub subject: HashMap<String, (String, u64)>,
}

/// A message the gateway took.
pub(super) struct Seen {
    pub platform: Platform,
    pub chat: String,
    /// The platform's id of the message, or its sender and text when the platform gives none.
    pub key: String,
    /// Whether `key` is the platform's id.
    pub by_id: bool,
    pub at: Instant,
}

/// What the forwarding thread sends out to chat apps.
pub(super) enum Out {
    /// A thread event a chat app may need to hear about.
    Event(AgentEventEnvelope),
    /// A message the person sent to a thread, from the computer or from a chat app.
    Sent {
        thread_id: String,
        message: Message,
        /// The conversation it came from, which already shows it.
        from: Option<(Platform, String)>,
    },
}

/// What a conversation on a chat app asked for that applies later.
#[derive(Default)]
pub(super) struct Chat {
    /// Where its next new thread opens, chosen with `/new`; none opens it where the settings say.
    pub project: Option<Place>,
}

/// Where a new thread opens.
pub(crate) enum Place {
    /// Outside any project.
    Chat,
    /// In the project with this id.
    Project(String),
}

/// A running connection.
pub(super) struct Link {
    pub connector: Arc<dyn Connector>,
    pub stop: Arc<AtomicBool>,
}

impl Hub {
    pub(super) fn lock(&self) -> MutexGuard<'_, State> {
        self.state.locked()
    }

    /// The connection to `platform`, while it runs.
    pub(super) fn connector(&self, platform: Platform) -> Option<Arc<dyn Connector>> {
        self.lock()
            .links
            .get(&platform)
            .map(|link| Arc::clone(&link.connector))
    }

    /// Records where `platform`'s connection stands, unless that connection was stopped.
    fn set_status(&self, platform: Platform, stop: &AtomicBool, status: Status) {
        let mut state = self.lock();
        if !stop.load(Ordering::Relaxed) {
            state.statuses.insert(platform, status);
        }
    }

    /// Stops every connection.
    pub(super) fn stop_all(&self) {
        let mut state = self.lock();
        for (_, link) in state.links.drain() {
            link.stop.store(true, Ordering::Relaxed);
        }
        state.statuses.clear();
    }

    /// Whether the thread that sends events out was started.
    pub(super) fn forwarding(&self) -> bool {
        self.events.locked().is_some()
    }

    pub(super) fn set_forwarder(&self, sender: Sender<Out>) {
        *self.events.locked() = Some(sender);
    }

    /// Passes on an event a chat app may need to hear about.
    pub(crate) fn observe(&self, event: &AgentEventEnvelope) {
        let wanted = matches!(
            event.event,
            AgentEvent::MessageStarted { .. }
                | AgentEvent::TextDelta { .. }
                | AgentEvent::ToolStarted { .. }
                | AgentEvent::ApprovalRequested { .. }
                | AgentEvent::ApprovalResolved { .. }
                | AgentEvent::TurnCompleted { .. }
                | AgentEvent::ThreadDeleted
        );
        if !wanted {
            return;
        }
        self.send(Out::Event(event.clone()));
    }

    /// Shows a message the person sent to `thread_id` in the chats that continue it, except the
    /// conversation `from` it came from.
    pub(crate) fn mirror(
        &self,
        thread_id: String,
        message: &Message,
        from: Option<(Platform, &str)>,
    ) {
        if !self.forwarding() {
            return;
        }
        self.send(Out::Sent {
            thread_id,
            message: message.clone(),
            from: from.map(|(platform, chat)| (platform, chat.to_string())),
        });
    }

    fn send(&self, out: Out) {
        if let Some(sender) = self.events.locked().as_ref() {
            // The forwarding thread only ends with the server, so a failed send has no reader.
            let _ = sender.send(out);
        }
    }
}

/// Starts a connection to `platform` on its own thread, retrying with a growing wait until it is
/// stopped.
pub(super) fn spawn(core: &Arc<Core>, platform: Platform, connector: Arc<dyn Connector>) {
    let stop = Arc::new(AtomicBool::new(false));
    let mut state = core.gateway.lock();
    state.statuses.insert(platform, Status::Connecting);
    state.links.insert(
        platform,
        Link {
            connector: Arc::clone(&connector),
            stop: Arc::clone(&stop),
        },
    );
    drop(state);
    let weak = Arc::downgrade(core);
    let spawned = std::thread::Builder::new()
        .name(format!("gateway-{}", platform.id()))
        .spawn(move || run(&weak, platform, connector.as_ref(), &stop));
    if let Err(error) = spawned {
        log::error!(error:% = error; "could not start a gateway connection: {{error}}");
    }
}

fn run(core: &Weak<Core>, platform: Platform, connector: &dyn Connector, stop: &AtomicBool) {
    let mut retry = FIRST_RETRY;
    while !stop.load(Ordering::Relaxed) {
        let Some(hub) = core.upgrade() else {
            return;
        };
        hub.gateway.set_status(platform, stop, Status::Connecting);
        hub.connection_changed();
        drop(hub);
        let reached = AtomicBool::new(false);
        let connected = |name: String| {
            reached.store(true, Ordering::Relaxed);
            log::info!(platform = platform.id(), name:% = name; "gateway connected to {{platform}} as {{name}}");
            if let Some(hub) = core.upgrade() {
                hub.gateway
                    .set_status(platform, stop, Status::Connected(name));
                hub.connection_changed();
            }
        };
        let inbox = |message: Incoming| {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            if let Some(hub) = core.upgrade() {
                hub.gateway_incoming(&message);
            }
        };
        let result = connector.listen(&Listener {
            stop,
            connected: &connected,
            inbox: &inbox,
        });
        if stop.load(Ordering::Relaxed) {
            return;
        }
        if reached.load(Ordering::Relaxed) {
            retry = FIRST_RETRY;
        }
        match result {
            Ok(()) => pause(RECONNECT, stop),
            Err(error) => {
                log::warn!(platform = platform.id(), error:% = error; "gateway lost {{platform}}: {{error}}");
                if let Some(hub) = core.upgrade() {
                    hub.gateway
                        .set_status(platform, stop, Status::Error(error.to_string()));
                    hub.connection_changed();
                }
                pause(retry, stop);
                retry = (retry * 2).min(MAX_RETRY);
            }
        }
    }
}

impl Core {
    /// Tells the window that a chat app connected, dropped or was switched off, so the
    /// sidebar shows where each one stands.
    pub(super) fn connection_changed(&self) {
        self.emit(AgentEventEnvelope {
            thread_id: String::new(),
            event: AgentEvent::ChatAppsChanged,
        });
    }
}

/// Sleeps for `time`, or until `stop` is set.
pub(super) fn pause(time: Duration, stop: &AtomicBool) {
    let mut left = time;
    while !left.is_zero() && !stop.load(Ordering::Relaxed) {
        let nap = left.min(STOP_CHECK);
        std::thread::sleep(nap);
        left -= nap;
    }
}
