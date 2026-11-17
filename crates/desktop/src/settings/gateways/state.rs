//! The pane's state: the connections' status, the saved credentials, the paired people and the
//! personalities on offer, and the operations that read and change them off the main thread.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::*;
use patchwork_core::gateway::{PairedUser, Personality, Platform, PlatformSettings, Status};

use crate::settings::SettingsDialog;

/// How often the open pane reads the connections' status again.
const POLL_EVERY: Duration = Duration::from_secs(1);

#[derive(Default)]
pub struct GatewaysState {
    /// Whether the pane is reading the status on a timer.
    pub(super) polling: bool,
    pub(super) statuses: HashMap<Platform, Status>,
    /// The chat app whose settings are open under the tiles.
    pub(super) selected: Option<Platform>,
    /// The credentials that are saved, by platform and key, once they have been read.
    pub(super) saved: Option<HashSet<(Platform, &'static str)>>,
    /// The saved credentials shown in the clear, by platform and key.
    pub(super) revealed: HashMap<(Platform, &'static str), String>,
    /// The credentials saved since the pane opened, by platform and key.
    pub(super) just_saved: HashSet<(Platform, &'static str)>,
    pub(super) users: Vec<PairedUser>,
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
        if self.gateways.selected.take().is_none() {
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
            move || core.gateway_credentials(),
            |d, saved, _| d.gateways.saved = Some(saved.into_iter().collect()),
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
        let shown = value.trim();
        let removed = shown.is_empty();
        if removed {
            self.hide_credential(platform, key, cx);
        } else if let Some(text) = self.gateways.revealed.get_mut(&(platform, key)) {
            *text = shown.to_string();
        }
        self.gateways.just_saved.remove(&(platform, key));
        let core = Arc::clone(&self.core);
        self.run(
            move || core.set_gateway_credential(platform, key, &value),
            move |d, out, cx| {
                if out.is_ok() && !removed {
                    d.gateways.just_saved.insert((platform, key));
                }
                d.gateways.error = out.err().map(|e| e.to_string());
                d.read_credentials(cx);
            },
            cx,
        );
    }

    /// Shows a saved credential in the clear, read off the main thread, or masks it again.
    pub(super) fn toggle_credential(
        &mut self,
        platform: Platform,
        key: &'static str,
        cx: &mut Context<Self>,
    ) {
        if self.gateways.revealed.contains_key(&(platform, key)) {
            self.hide_credential(platform, key, cx);
            cx.notify();
            return;
        }
        let core = Arc::clone(&self.core);
        self.run(
            move || core.gateway_credential(platform, key),
            move |d, text, cx| {
                let Some(text) = text else {
                    return;
                };
                d.gateways.revealed.insert((platform, key), text);
                d.mask_credential(platform, key, false, cx);
            },
            cx,
        );
    }

    /// Masks a credential and forgets its shown text.
    pub(super) fn hide_credential(
        &mut self,
        platform: Platform,
        key: &'static str,
        cx: &mut Context<Self>,
    ) {
        self.gateways.revealed.remove(&(platform, key));
        self.mask_credential(platform, key, true, cx);
    }

    pub(super) fn mask_credential(
        &self,
        platform: Platform,
        key: &str,
        masked: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(area) = self.field_area(&format!("gw-{}-{key}", platform.id())) {
            area.update(cx, |a, cx| {
                a.masked = masked;
                cx.notify();
            });
        }
    }

    pub(super) fn remove_paired(&mut self, platform: Platform, id: String, cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
        self.run(
            move || {
                core.remove_gateway_user(platform, &id)
                    .map(|()| core.gateway_users())
            },
            |d, out, _| match out {
                Ok(users) => {
                    d.gateways.users = users;
                    d.gateways.error = None;
                }
                Err(e) => d.gateways.error = Some(e.to_string()),
            },
            cx,
        );
    }

    /// Reads the status, the pairing code and the paired people every second while the pane is
    /// open.
    pub(super) fn poll_gateways(&mut self, cx: &mut Context<Self>) {
        self.gateways.polling = true;
        let core = Arc::clone(&self.core);
        cx.spawn(async move |this, cx| {
            loop {
                let open = this.read_with(cx, |d, _| d.section == "gateways");
                if !open.unwrap_or(false) {
                    let _ = this.update(cx, |d, _| d.gateways.polling = false);
                    return;
                }
                let core = Arc::clone(&core);
                let (statuses, pairing, users, personalities, dir) = cx
                    .background_spawn(async move {
                        (
                            core.gateway_status(),
                            core.gateway_code(),
                            core.gateway_users(),
                            core.personalities(),
                            core.personalities_dir().to_string_lossy().into_owned(),
                        )
                    })
                    .await;
                let applied = this.update(cx, |d, cx| {
                    d.gateways.statuses = statuses.into_iter().collect();
                    d.gateways.code = pairing;
                    d.gateways.users = users;
                    d.gateways.personalities = personalities;
                    d.gateways.personalities_dir = dir;
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
