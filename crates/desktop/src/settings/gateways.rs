//! Settings → Gateways: the chat apps Patchwork answers on. Each has its tokens, kept in the
//! system credential store, its connection's status, and a switch; the people paired with a
//! one-time code are listed below them.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use gpui::{AnyElement, ClickEvent, FontWeight, Role, SharedString, Window, div, prelude::*, px};
use patchwork_core::gateway::{PairedUser, Platform, PlatformSettings, Status};
use patchwork_core::system::now_ms;

use crate::settings::SettingsDialog;
use crate::settings::gateway_setup::setup;
use crate::settings::controls::{
    ACCESS_MODES, FieldKind, alert, choice, list_box, section_heading, segmented, select_box,
    switch, tile, tile_grid, token_row,
};
use crate::ui::theme::{font_mono, fs_lg, fs_sm, theme};
use crate::ui::{BtnKind, btn, icon_button, platform_logo};

/// How often the open pane reads the connections' status again.
const POLL_EVERY: Duration = Duration::from_secs(1);

/// The width of a token or address field.
const FIELD_WIDTH: f32 = 260.;

/// The milliseconds in a minute, for the pairing code's time left.
const MINUTE_MS: u64 = 60_000;

#[derive(Default)]
pub struct GatewaysState {
    /// Whether the pane is reading the status on a timer.
    polling: bool,
    statuses: HashMap<Platform, Status>,
    /// The chat app whose settings are open under the tiles.
    selected: Option<Platform>,
    /// The credentials that are saved, by platform and key, once they have been read.
    saved: Option<HashSet<(Platform, &'static str)>>,
    users: Vec<PairedUser>,
    /// The pairing code and when it stops working, in epoch milliseconds.
    code: Option<(String, u64)>,
    /// Why the last credential or removal failed.
    pub(super) error: Option<String>,
}

impl SettingsDialog {
    /// Goes back from a chat app's page to the grid. False when none is open.
    pub(super) fn close_gateway(&mut self, cx: &mut Context<Self>) -> bool {
        if self.gateways.selected.take().is_none() {
            return false;
        }
        cx.notify();
        true
    }

    /// Saves `platform`'s settings as `change` leaves them.
    fn set_platform(
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
    fn save_credential(
        &mut self,
        platform: Platform,
        key: &'static str,
        value: String,
        cx: &mut Context<Self>,
    ) {
        let core = Arc::clone(&self.core);
        self.run(
            move || core.set_gateway_credential(platform, key, &value),
            |d, out, cx| {
                d.gateways.error = out.err().map(|e| e.to_string());
                d.read_credentials(cx);
            },
            cx,
        );
    }

    fn remove_paired(&mut self, platform: Platform, id: String, cx: &mut Context<Self>) {
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
    fn poll_gateways(&mut self, cx: &mut Context<Self>) {
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
                let (statuses, pairing, users) = cx
                    .background_spawn(async move {
                        (
                            core.gateway_status(),
                            core.gateway_code(),
                            core.gateway_users(),
                        )
                    })
                    .await;
                let applied = this.update(cx, |d, cx| {
                    d.gateways.statuses = statuses.into_iter().collect();
                    d.gateways.code = pairing;
                    d.gateways.users = users;
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

/// What a platform's status says, and whether it is an error.
fn status_line(status: &Status) -> (String, bool) {
    match status {
        Status::Off => ("Off".to_string(), false),
        Status::Connecting => ("Connecting…".to_string(), false),
        Status::Connected(name) => (format!("Connected as {name}"), false),
        Status::Error(error) => (error.clone(), true),
    }
}

/// What chats started from a chat app may do, and the project they open in.
fn general(d: &mut SettingsDialog, cx: &mut Context<SettingsDialog>) -> Vec<AnyElement> {
    let t = *theme(cx);
    let gateway = d.settings.gateway.clone();
    let access = token_row("Access", None, &t).child(
        segmented(
            "gw-access",
            "Access",
            gateway.access,
            &ACCESS_MODES,
            &t,
            |d, mode, cx| d.update(|s| s.gateway.access = mode, cx),
            cx,
        )
        // Each choice as wide as its name: the longest one would stretch the others.
        .flex(),
    );
    let projects = d.settings.projects.clone();
    let current = gateway
        .project_id
        .as_ref()
        .and_then(|id| projects.iter().find(|p| &p.id == id))
        .map_or_else(|| "No project".to_string(), |p| p.name.clone());
    let chosen = gateway.project_id.clone();
    let project = token_row("Project", None, &t).child(
        select_box(
            "gw-project",
            "Project",
            current,
            d.menu_open("gw-project"),
            &t,
        )
        .w(px(200.))
        .on_click(cx.listener(move |d, e: &ClickEvent, window, cx| {
            let weak = cx.entity().downgrade();
            let none = choice(weak.clone(), "No project", chosen.is_none(), |d, cx| {
                d.menu = None;
                d.update(|s| s.gateway.project_id = None, cx);
            });
            let items = std::iter::once(none)
                .chain(projects.iter().map(|p| {
                    let id = p.id.clone();
                    let checked = chosen.as_ref() == Some(&p.id);
                    choice(weak.clone(), p.name.clone(), checked, move |d, cx| {
                        let id = id.clone();
                        d.menu = None;
                        d.update(|s| s.gateway.project_id = Some(id), cx);
                    })
                }))
                .collect();
            d.toggle_menu("gw-project", "Project", items, e.position(), window, cx);
        })),
    );
    vec![access.into_any_element(), project.into_any_element()]
}

/// The one-time code a new person sends a bot to pair, and the button that makes one.
fn pairing_row(d: &SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let code = d
        .gateways
        .code
        .clone()
        .filter(|(_, until)| *until > now_ms());
    let shown = code.map(|(code, until)| {
        let minutes = until.saturating_sub(now_ms()).div_ceil(MINUTE_MS);
        div()
            .id("gw-code")
            .role(Role::Status)
            .aria_label(format!("Pairing code {code}, {minutes} min left"))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .font_family(font_mono())
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(code),
            )
            .child(
                div()
                    .text_size(px(fs_sm()))
                    .text_color(t.text_muted)
                    .child(format!("{minutes} min")),
            )
    });
    let pairing = token_row("Pairing code", None, &t).children(shown).child(
        btn("gw-new-code", "New code", BtnKind::Plain, false, &t)
            .child("New code")
            .on_click(cx.listener(|d, _, _, cx| {
                d.gateways.code = Some(d.core.new_gateway_code());
                cx.notify();
            })),
    );
    pairing.into_any_element()
}

/// One platform: its switch and status, its tokens, and its server and account.
fn platform(
    d: &mut SettingsDialog,
    platform: Platform,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> Vec<AnyElement> {
    let t = *theme(cx);
    let settings = d.settings.gateway.platform(platform);
    let id = platform.id();
    let label = platform.label();
    let enabled = settings.enabled;
    let status = if enabled {
        d.gateways
            .statuses
            .get(&platform)
            .cloned()
            .unwrap_or_default()
    } else {
        Status::Off
    };
    let (line, failed) = status_line(&status);
    let line = div()
        .id(SharedString::from(format!("gw-status-{id}")))
        .role(if failed { Role::Alert } else { Role::Status })
        .aria_label(format!("{label}: {line}"))
        .min_w_0()
        .max_w(px(320.))
        .truncate()
        .text_size(px(fs_sm()))
        .text_color(if failed { t.danger } else { t.text_muted })
        .child(line);
    let switch_label = format!("Answer on {label}");
    let mut rows = vec![
        token_row(switch_label.clone(), None, &t)
            .child(line)
            .child(
                switch(format!("gw-{id}-enabled"), switch_label, enabled, &t).on_click(
                    cx.listener(move |d, _, _, cx| {
                        d.set_platform(platform, |s| s.enabled = !enabled, cx);
                    }),
                ),
            )
            .into_any_element(),
    ];
    let kind = FieldKind {
        masked: true,
        mono: true,
        ..Default::default()
    };
    for credential in platform.credentials() {
        let key = credential.key;
        let saved = d
            .gateways
            .saved
            .as_ref()
            .is_some_and(|s| s.contains(&(platform, key)));
        let name = format!("{label} {}", credential.label.to_lowercase());
        let field = d
            .field(
                format!("gw-{id}-{key}"),
                name.clone(),
                "",
                if saved {
                    "Saved"
                } else {
                    credential.placeholder
                },
                kind,
                move |d, text, cx| {
                    if !text.trim().is_empty() {
                        d.save_credential(platform, key, text, cx);
                    }
                },
                window,
                cx,
            )
            .w(px(FIELD_WIDTH))
            .flex_none();
        rows.push(
            token_row(credential.label, None, &t)
                .when(saved, |row| {
                    row.child(
                        icon_button(
                            SharedString::from(format!("gw-{id}-{key}-remove")),
                            format!("Remove {name}"),
                            "x",
                            14.,
                            cx,
                        )
                        .on_click(cx.listener(move |d, _, _, cx| {
                            d.save_credential(platform, key, String::new(), cx);
                        })),
                    )
                })
                .child(field)
                .into_any_element(),
        );
    }
    for field in platform.settings() {
        let key = field.key;
        let input = d.text_field(
            format!("gw-{id}-{key}"),
            format!("{label} {}", field.label.to_lowercase()),
            settings.value(key),
            field.placeholder,
            Some(FIELD_WIDTH),
            move |d, text, cx| {
                d.set_platform(
                    platform,
                    |s| {
                        s.values.insert(key.to_string(), text.trim().to_string());
                    },
                    cx,
                );
            },
            window,
            cx,
        );
        rows.push(
            token_row(field.label, None, &t)
                .child(input)
                .into_any_element(),
        );
    }
    rows
}

/// One tile per chat app, with a dot for each one that is on; a click opens its settings.
fn tiles(d: &SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let gateway = &d.settings.gateway;
    let tiles = Platform::ALL.map(|platform| {
        let (id, label) = (platform.id(), platform.label());
        let enabled = gateway.platform(platform).enabled;
        let status = d
            .gateways
            .statuses
            .get(&platform)
            .cloned()
            .unwrap_or_default();
        let (state, dot) = match status {
            _ if !enabled => ("off", None),
            Status::Off => ("on", Some(t.border_strong)),
            Status::Connecting => ("connecting", Some(t.warning)),
            Status::Connected(_) => ("connected", Some(t.success)),
            Status::Error(_) => ("failed", Some(t.danger)),
        };
        let open = d.gateways.selected == Some(platform);
        tile(format!("gw-tile-{id}"), format!("{label}, {state}"), &t)
            .aria_expanded(open)
            .border_color(if open { t.accent } else { t.border })
            .bg(t.bg_panel)
            .text_color(t.text)
            .children(dot.map(|dot| {
                div()
                    .absolute()
                    .top(px(8.))
                    .right(px(8.))
                    .size(px(8.))
                    .rounded_full()
                    .bg(dot)
            }))
            .child(platform_logo(platform, 22.))
            .child(div().max_w_full().truncate().child(label))
            .on_click(cx.listener(move |d, _, _, cx| {
                d.gateways.selected = Some(platform);
                cx.notify();
            }))
    });
    tile_grid("Chat apps").children(tiles).into_any_element()
}

/// A chat app's own page: how to set it up, its switch and fields, and the pairing code.
fn page(
    d: &mut SettingsDialog,
    each: Platform,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let label = each.label();
    let header = div()
        .flex()
        .items_center()
        .gap(px(12.))
        .pb(px(8.))
        .child(
            icon_button("gw-back", "Back", "caret-left", 16., cx)
                .tooltip(crate::ui::tooltip("Back"))
                .on_click(cx.listener(|d, _, _, cx| {
                    d.close_gateway(cx);
                })),
        )
        .child(platform_logo(each, 24.))
        .child(
            div()
                .id("gw-title")
                .role(Role::Heading)
                .text_size(px(fs_lg()))
                .font_weight(FontWeight::SEMIBOLD)
                .child(label),
        );
    let steps = setup(each, cx);
    let rows = platform(d, each, window, cx);
    let pairing = pairing_row(d, cx);
    div()
        .id("gw-platform")
        .role(Role::Group)
        .aria_label(label)
        .flex()
        .flex_col()
        .child(header)
        .child(steps)
        .children(rows)
        .child(pairing)
        .children(
            d.gateways
                .error
                .clone()
                .map(|e| alert("gw-error", e, t.danger)),
        )
        .into_any_element()
}

/// The people who can use Patchwork from a chat app.
fn paired(d: &SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    if d.gateways.users.is_empty() {
        return div()
            .py(px(10.))
            .text_color(t.text_muted)
            .child("No apps are paired")
            .into_any_element();
    }
    let rows = d
        .gateways
        .users
        .iter()
        .map(|user| {
            let (platform, id) = (user.platform, user.id.clone());
            let name = format!("{} on {}", user.name, platform.label());
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .px(px(12.))
                .py(px(8.))
                .child(div().min_w_0().flex_1().truncate().child(name.clone()))
                .child(
                    icon_button(
                        SharedString::from(format!("gw-unpair-{}-{}", platform.id(), user.id)),
                        format!("Remove {name}"),
                        "x",
                        14.,
                        cx,
                    )
                    .on_click(cx.listener(move |d, _, _, cx| {
                        d.remove_paired(platform, id.clone(), cx);
                    })),
                )
                .into_any_element()
        })
        .collect();
    div()
        .id("gw-paired")
        .role(Role::List)
        .aria_label("Paired apps")
        .mt(px(6.))
        .child(list_box(rows, &t))
        .into_any_element()
}

pub fn render(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    if d.gateways.saved.is_none() {
        d.gateways.saved = Some(HashSet::new());
        d.read_credentials(cx);
    }
    if !d.gateways.polling {
        d.poll_gateways(cx);
    }
    if let Some(each) = d.gateways.selected {
        return page(d, each, window, cx);
    }
    let rows = general(d, cx);
    let apps = tiles(d, cx);
    let people = paired(d, cx);
    div()
        .flex()
        .flex_col()
        .children(rows)
        .child(section_heading("Chat apps", false, &t))
        .child(apps)
        .child(section_heading("Paired apps", false, &t))
        .child(people)
        .children(
            d.gateways
                .error
                .clone()
                .map(|e| alert("gw-error", e, t.danger)),
        )
        .into_any_element()
}
