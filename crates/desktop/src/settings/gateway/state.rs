//! The pane's state: the connections' status, the saved credentials, the pairings and the
//! personalities on offer, and the operations that read and change them off the main thread.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::*;
use patchwork_core::gateway::{Pairing, Personality, Platform, PlatformSettings, Status};

use crate::settings::SettingsDialog;

/// How often the open pane reads the connections' status again.
const POLL_EVERY: Duration = Duration::from_secs(1);

#[derive(Default)]
pub struct GatewayState {
    /// Whether the pane is reading the status on a timer.
    pub(super) polling: bool,
    pub(super) statuses: HashMap<Platform, Status>,
    /// The chat app whose settings are open under the tiles.
    pub(super) selected: Option<Platform>,
    /// The credentials that are saved, by platform and key, once they have been read.
    pub(super) saved: Option<HashSet<(Platform, &'static str)>>,
    /// The credentials saved since the pane opened, by platform and key.
    pub(super) just_saved: HashSet<(Platform, &'static str)>,
    pub(super) pairings: Vec<Pairing>,
    /// The pairing code and when it stops working, in epoch milliseconds.
    pub(super) code: Option<(String, u64)>,
    /// The personalities a chat app's chats can talk in, and the folder they are in.
    pub(super) personalities: Vec<Personality>,
    pub(super) personalities_dir: String,
    /// Why the last credential or removal failed.
    pub(super) error: Option<String>,
}

impl SettingsDialog {
    /// Goes back from a chat app's page to the grid. False when none is open.
    pub(in crate::settings) fn close_gateway(&mut self, cx: &mut Context<Self>) -> bool {
        if self.gateway.selected.take().is_none() {
            return false;
        }
        cx.notify();
        true
    }

    /// Saves `platform`'s settings as `change` leaves them.
    pub(super) fn set_platform(
        &mut self,
        platform: Platform,
        change: impl FnOnce(&mut PlatformSettings),
        cx: &mut Context<Self>,
    ) {
        self.update(
            |s| change(s.gateway.platforms.entry(platform).or_default()),
            cx,
        );
    }

    /// Reads which credentials are saved, off the main thread.
    pub(super) fn read_credentials(&mut self, cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
        self.run(
            move || core.chat_app_credentials(),
            |d, saved, _| d.gateway.saved = Some(saved.into_iter().collect()),
            cx,
        );
    }

    /// Saves or, when `value` is empty, removes a credential, off the main thread.
    pub(super) fn save_credential(
        &mut self,
        platform: Platform,
        key: &'static str,
        value: String,
        cx: &mut Context<Self>,
    ) {
        let removed = value.trim().is_empty();
        self.gateway.just_saved.remove(&(platform, key));
        let core = Arc::clone(&self.core);
        self.run(
            move || core.set_chat_app_credential(platform, key, &value),
            move |d, out, cx| {
                if out.is_ok() && !removed {
                    d.gateway.just_saved.insert((platform, key));
                    // A saved credential is not kept in the window, or read into it.
                    if let Some(area) = d.field_area(&format!("gw-{}-{key}", platform.id())) {
                        area.update(cx, |a, cx| a.set_text("", cx));
                    }
                }
                d.gateway.error = out.err().map(|e| e.to_string());
                d.read_credentials(cx);
            },
            cx,
        );
    }

    pub(super) fn remove_paired(&mut self, platform: Platform, id: String, cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
        self.run(
            move || core.remove_pairing(platform, &id).map(|()| core.pairings()),
            |d, out, _| match out {
                Ok(paired) => {
                    d.gateway.pairings = paired;
                    d.gateway.error = None;
                }
                Err(e) => d.gateway.error = Some(e.to_string()),
            },
            cx,
        );
    }

    /// Reads the status, the pairing code and the pairings every second while the pane is
    /// open.
    pub(super) fn poll_gateway(&mut self, cx: &mut Context<Self>) {
        self.gateway.polling = true;
        let core = Arc::clone(&self.core);
        cx.spawn(async move |this, cx| {
            loop {
                let open = this.read_with(cx, |d, _| d.section == "gateway");
                if !open.unwrap_or(false) {
                    let _ = this.update(cx, |d, _| d.gateway.polling = false);
                    return;
                }
                let core = Arc::clone(&core);
                let (statuses, pairing, paired, personalities, dir) = cx
                    .background_spawn(async move {
                        (
                            core.gateway_status(),
                            core.pairing_code(),
                            core.pairings(),
                            core.personalities(),
                            core.personalities_dir().to_string_lossy().into_owned(),
                        )
                    })
                    .await;
                let applied = this.update(cx, |d, cx| {
                    d.gateway.statuses = statuses.into_iter().collect();
                    d.gateway.code = pairing;
                    d.gateway.pairings = paired;
                    d.gateway.personalities = personalities;
                    d.gateway.personalities_dir = dir;
                    cx.notify();
                });
                if applied.is_err() {
                    return;
                }
                cx.background_executor().timer(POLL_EVERY).await;
            }
        })
        .detach();
    }
}
