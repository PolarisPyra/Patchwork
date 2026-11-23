//! A turn the agent's usage limit ended: when that limit resets, and resuming the turn then.
//!
//! The reply of such a turn ends with an error while one of the agent's usage windows is used
//! up. The user can ask for the turn to resume once the window resets; the wait is saved, so it
//! outlasts a restart, and a reset that passed while the app was closed resumes at launch.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use std::sync::Arc;
use std::time::Duration;

use crate::system::now_ms;
use serde::{Deserialize, Serialize};

use crate::events::AgentEvent;
use crate::provider::AgentKind;
use crate::chats::{Message, MessageStatus, Part, Chat};
use crate::usage::ProviderUsage;
use crate::{Error, Core};

/// A window counts as used up from here. Providers report a limit that was hit as 100, and one
/// rounds a little under.
const USED_UP: f64 = 99.5;

/// How long after the reset the turn resumes. A provider lets the limit go a little after the
/// time it gives, and a turn resumed too early only fails on the same limit again.
const MARGIN_MS: u64 = 60_000;

/// How often the waits are checked. A reset is hours away, so half a minute late is unseen.
const CHECK_EVERY: Duration = Duration::from_secs(30);

/// When the last of the used-up windows in `usage` resets, in Unix epoch milliseconds. None
/// while no window is used up and still to reset.
fn used_up_until(usage: &ProviderUsage) -> Option<u64> {
    let now = now_ms();
    usage
        .windows
        .iter()
        .filter(|w| w.used_percent >= USED_UP)
        .filter_map(|w| w.resets_at)
        .filter(|at| *at > now)
        .max()
}

/// One turn waiting for a limit to reset, or one whose wait the user cancelled.
#[derive(Debug, Clone)]
pub struct LimitWait {
    /// When it resumes, in Unix epoch milliseconds.
    at: u64,
    /// The reply that failed. The wait is dropped once the chat has moved past it.
    message_id: String,
    /// Whether the turn resumes at `at`. A cancelled wait is kept until then for the time it
    /// holds: the usage numbers that gave it may be gone, after a restart, when the user asks
    /// for the wait again.
    on: bool,
}

impl LimitWait {
    /// The wait as it is saved: its time, its reply, and `off` once it is cancelled.
    fn saved(&self) -> String {
        let off = if self.on { "" } else { "\noff" };
        format!("{}\n{}{off}", self.at, self.message_id)
    }

    /// A wait read back from how `saved` wrote it.
    fn read(note: &str) -> Option<Self> {
        let mut lines = note.lines();
        Some(Self {
            at: lines.next()?.parse().ok()?,
            message_id: lines.next()?.into(),
            on: lines.next() != Some("off"),
        })
    }
}

/// The replies of a chat that failed on its agent's usage limit and still warn of it, and
/// that agent.
#[derive(Debug, Serialize, Deserialize)]
struct LimitFailures {
    agent: AgentKind,
    replies: Vec<String>,
    /// When the limit they failed on resets, in Unix epoch milliseconds; their warnings are
    /// stale from then. 0 once they have been taken away and only the reply to resume is left.
    #[serde(default)]
    reset_at: u64,
}

/// `reply` without its usage limit warning: the error, and the same words where the agent also
/// ended its text with them. `keep_text` leaves the text, for a reply whose words are the only
/// thing that says why it stopped.
fn without_warning(reply: &Message, keep_text: bool) -> Message {
    let mut reply = reply.clone();
    let Some(warning) = reply.error.take() else {
        return reply;
    };
    let warning = warning.trim();
    if keep_text || warning.is_empty() {
        return reply;
    }
    let cut = |text: &mut String| {
        if let Some(kept) = text.trim_end().strip_suffix(warning) {
            *text = kept.trim_end().to_string();
        }
    };
    cut(&mut reply.text);
    if let Some(Part::Text { text }) = reply
        .parts
        .iter_mut()
        .rev()
        .find(|p| matches!(p, Part::Text { .. }))
    {
        cut(text);
    }
    reply
}

/// The `app_state` key of a chat's `LimitFailures`.
fn failed_key(chat_id: &str) -> String {
    format!("limit-failed:{chat_id}")
}

fn key(chat_id: &str) -> String {
    format!("resume-at:{chat_id}")
}

impl Core {
    /// When the usage limit that ended the chat's last reply resets, in Unix epoch
    /// milliseconds. None unless that reply failed while one of its agent's usage windows was
    /// used up and is still to reset. Reads only what is already known, so it is cheap to call
    /// while drawing.
    #[must_use]
    pub fn limit_reset(&self, chat: &Chat) -> Option<u64> {
        let reply = chat
            .resumable_reply()
            .filter(|m| m.status == MessageStatus::Error)?;
        let known = self.usage.known(chat.agent);
        if let Some(reset) = known.as_ref().and_then(used_up_until) {
            return Some(reset);
        }
        // The numbers are gone or have moved, but a wait set on this reply holds its reset.
        self.runner
            .limit_waits
            .locked()
            .get(&chat.id)
            .filter(|wait| wait.message_id == reply.id)
            .map(|wait| wait.at.saturating_sub(MARGIN_MS))
            .filter(|reset| *reset > now_ms())
    }

    /// Notes that the chat's reply `message_id`, by `agent`, failed on the agent's usage limit,
    /// as far as the app can tell: it is the reply the chat would resume, and the agent's usage
    /// shows a limit used up, which is when the app offers to resume once the limit resets.
    pub(in crate::orchestration) fn note_limit_failure(
        &self,
        chat_id: &str,
        message_id: &str,
        agent: AgentKind,
    ) {
        let Some(chat) = self.store.chat(chat_id) else {
            return;
        };
        let failed = chat.resumable_reply().is_some_and(|m| m.id == message_id);
        let Some(reset) = self.limit_reset(&chat).filter(|_| failed) else {
            return;
        };
        let noted = self
            .store
            .state(&failed_key(chat_id))
            .and_then(|json| serde_json::from_str::<LimitFailures>(&json).ok())
            .filter(|noted| noted.agent == agent);
        let mut failures = noted.unwrap_or(LimitFailures {
            agent,
            replies: Vec::new(),
            reset_at: 0,
        });
        failures.replies.push(message_id.to_string());
        failures.reset_at = failures.reset_at.max(reset);
        let saved = serde_json::to_string(&failures)
            .map_err(|e| Error::from(e.to_string()))
            .and_then(|json| self.store.set_state(&failed_key(chat_id), &json));
        if let Err(e) = saved {
            log::warn!(error:% = e; "could not note a reply that failed on a usage limit: {{error}}");
        }
    }

    /// Takes the usage limit warnings off the chat's earlier replies, now that `agent` has
    /// replied in it again: the limit they warned of has reset, or was never reached. A reply
    /// that holds nothing but the warning goes; one with work in it keeps its work. Replies that
    /// failed on another agent's limit keep their warning, which says why the chat moved off it.
    pub(in crate::orchestration) fn clear_limit_failures(&self, chat_id: &str, agent: AgentKind) {
        let Some(failures) = self
            .store
            .state(&failed_key(chat_id))
            .and_then(|json| serde_json::from_str::<LimitFailures>(&json).ok())
            .filter(|noted| noted.agent == agent)
        else {
            return;
        };
        self.lift_limit_failures(chat_id, &failures, false);
    }

    /// Takes the warnings off the replies whose limit has since reset, in every chat: they
    /// said when it would, and that time has passed. The reply a chat would resume stays, as
    /// what its Resume button carries on, without its warning, until the agent replies again.
    fn expire_limit_failures(&self) {
        let now = now_ms();
        let chats: Vec<String> = self.store.with_chats(|chats| {
            chats
                .iter()
                .filter(|t| t.messages.iter().any(|m| m.error.is_some()))
                .map(|t| t.id.clone())
                .collect()
        });
        for chat_id in chats {
            let failures = self
                .store
                .state(&failed_key(&chat_id))
                .and_then(|json| serde_json::from_str::<LimitFailures>(&json).ok())
                .filter(|noted| noted.reset_at != 0 && noted.reset_at <= now);
            if let Some(failures) = failures {
                self.lift_limit_failures(&chat_id, &failures, true);
            }
        }
    }

    /// Takes the warning off each of `failures`, removing the replies that hold nothing else.
    /// With `expiring`, the reply the chat would resume is kept, and noted again for the agent's
    /// next reply to take away.
    fn lift_limit_failures(&self, chat_id: &str, failures: &LimitFailures, expiring: bool) {
        let Some(chat) = self.store.chat(chat_id) else {
            return;
        };
        let resumable = chat.resumable_reply().map(|m| m.id.clone());
        let mut kept = Vec::new();
        for id in &failures.replies {
            let Some(reply) = chat.messages.iter().find(|m| &m.id == id) else {
                continue;
            };
            let text = reply.text.trim();
            let only_warning = (text.is_empty()
                || reply.error.as_deref().is_some_and(|e| text == e.trim()))
                && reply.tools.is_empty()
                && reply.files.is_empty()
                && reply.approvals.is_empty();
            let keep = expiring && resumable.as_deref() == Some(id.as_str());
            if keep {
                kept.push(id.clone());
            }
            let event = if only_warning && !keep {
                AgentEvent::MessageRemoved {
                    message_id: id.clone(),
                }
            } else if reply.error.is_some() {
                AgentEvent::MessageEdited {
                    message: without_warning(reply, keep),
                }
            } else {
                continue;
            };
            crate::orchestration::publish(self, chat_id, event);
        }
        let left = (!kept.is_empty()).then_some(LimitFailures {
            agent: failures.agent,
            replies: kept,
            reset_at: 0,
        });
        let saved = match left {
            Some(left) => serde_json::to_string(&left)
                .map_err(|e| Error::from(e.to_string()))
                .and_then(|json| self.store.set_state(&failed_key(chat_id), &json)),
            None => self.store.clear_state(&failed_key(chat_id)),
        };
        if let Err(e) = saved {
            log::warn!(error:% = e; "could not save what is left of the usage limit warnings: {{error}}");
        }
    }

    /// When the chat's turn is set to resume, if it is waiting for a limit to reset.
    #[must_use]
    pub fn limit_resume_at(&self, chat_id: &str) -> Option<u64> {
        self.runner
            .limit_waits
            .locked()
            .get(chat_id)
            .filter(|wait| wait.on)
            .map(|wait| wait.at)
    }

    /// Sets the chat's failed turn to resume once its agent's usage limit has reset, and
    /// returns when.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is unknown, when its last reply did not fail on a usage
    /// limit that is still to reset, or when the wait cannot be saved.
    pub fn resume_at_limit_reset(&self, chat_id: &str) -> Result<u64, Error> {
        let chat = self.chat(chat_id)?;
        let reset = self
            .limit_reset(&chat)
            .ok_or("This reply did not stop on a usage limit that is still to reset")?;
        let message_id = chat
            .resumable_reply()
            .map(|m| m.id.clone())
            .ok_or("This reply cannot be resumed")?;
        let at = reset + MARGIN_MS;
        // Saved and noted under the one lock, so a cancel lands before both or after both.
        let mut waits = self.runner.limit_waits.lock();
        let waits = waits.as_mut().unwrap_or_else(|e| e.get_mut());
        let wait = LimitWait {
            at,
            message_id,
            on: true,
        };
        self.store.set_state(&key(chat_id), &wait.saved())?;
        waits.insert(chat_id.into(), wait);
        self.drop_reset_notices(chat_id);
        Ok(at)
    }

    /// Stops the chat's turn from resuming when its limit resets. The wait is kept, switched
    /// off, so it can be asked for again.
    ///
    /// # Errors
    ///
    /// Returns an error when the change to the saved wait cannot be saved.
    pub fn cancel_limit_resume(&self, chat_id: &str) -> Result<(), Error> {
        let mut waits = self.runner.limit_waits.lock();
        let waits = waits.as_mut().unwrap_or_else(|e| e.get_mut());
        let Some(wait) = waits.get_mut(chat_id) else {
            return self.store.clear_state(&key(chat_id));
        };
        wait.on = false;
        self.store.set_state(&key(chat_id), &wait.saved())
    }

    /// Takes up the waits an earlier session saved, and resumes each turn as its limit resets,
    /// for the life of the app. A Patchwork running beside another waits on none, as it resumes
    /// none.
    pub fn start_limit_resumes(self: &Arc<Self>) {
        if !self.store.owns_data() {
            return;
        }
        let saved: Vec<(String, LimitWait)> = self
            .store
            .with_chats(|chats| {
                chats
                    .iter()
                    .filter(|t| t.resumable_reply().is_some())
                    .map(|t| t.id.clone())
                    .collect::<Vec<_>>()
            })
            .into_iter()
            .filter_map(|id| {
                let wait = LimitWait::read(&self.store.state(&key(&id))?)?;
                Some((id, wait))
            })
            .collect();
        self.runner.limit_waits.locked().extend(saved);
        let core = Arc::clone(self);
        let started = std::thread::Builder::new()
            .name("limit-resume".into())
            .spawn(move || {
                core.learn_limits();
                loop {
                    core.expire_limit_failures();
                    core.resume_due();
                    std::thread::sleep(CHECK_EVERY);
                }
            });
        if let Err(e) = started {
            log::warn!(error:% = e; "could not start waiting on usage limits: {{error}}");
        }
    }

    /// Asks for the usage of each agent a chat's last reply failed on, once, at launch. Whether
    /// such a reply stopped on a limit, and when the limit resets, is read from the usage
    /// numbers, and none are known yet in an app that has just started.
    fn learn_limits(&self) {
        let mut agents: Vec<_> = self.store.with_chats(|chats| {
            chats
                .iter()
                .filter(|t| t.archived_at.is_none())
                .filter(|t| {
                    t.resumable_reply()
                        .is_some_and(|m| m.status == MessageStatus::Error)
                })
                .map(|t| t.agent)
                .collect()
        });
        agents.dedup();
        for agent in agents {
            if self.usage.known(agent).is_none() {
                self.usage.get(agent, false);
            }
        }
    }

    /// Resumes the turns whose wait is over. A wait is used once, whether or not its turn
    /// starts: a chat that has moved on is left alone, and a turn that fails again on the
    /// limit can be set to wait again.
    fn resume_due(self: &Arc<Self>) {
        let now = now_ms();
        let due: Vec<(String, LimitWait)> = {
            let mut waits = self.runner.limit_waits.lock();
            let waits = waits.as_mut().unwrap_or_else(|e| e.get_mut());
            let ids: Vec<String> = waits
                .iter()
                .filter(|(_, wait)| wait.at <= now)
                .map(|(id, _)| id.clone())
                .collect();
            ids.into_iter()
                .filter_map(|id| waits.remove(&id).map(|wait| (id, wait)))
                .collect()
        };
        for (chat_id, wait) in due {
            if let Err(e) = self.store.clear_state(&key(&chat_id)) {
                log::warn!(error:% = e; "could not drop a finished limit wait: {{error}}");
            }
            let still_waiting = self
                .store
                .chat(&chat_id)
                .is_some_and(|t| t.resumable_reply().is_some_and(|m| m.id == wait.message_id));
            // A cancelled wait was kept only for its time, which has now passed.
            if !still_waiting || !wait.on {
                continue;
            }
            if let Err(e) = self.resume_turn(&chat_id) {
                log::warn!(
                    chat = chat_id.as_str(), error:% = e;
                    "could not resume {{chat}} after its usage limit reset: {{error}}"
                );
            }
        }
    }
}
