//! Settings → Gateways: the chat apps Patchwork answers on. Each has its tokens, kept in the
//! system credential store, its connection's status, and a switch; the people paired with a
//! one-time code are listed below them.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AnyElement, ClickEvent, FontWeight, Role, SharedString, Toggled, Window, div, prelude::*, px,
};
use patchwork_core::gateway::{PairedUser, Platform, PlatformSettings, Status};
use patchwork_core::system::now_ms;

use crate::settings::SettingsDialog;
use crate::settings::gateway_setup::setup;
use crate::settings::controls::{
    ACCESS_MODES, FieldKind, alert, choice, list_box, section_heading, segmented, select_box,
    switch, tile, tile_grid, token_row,
};
use crate::ui::theme::{font_mono, fs_lg, fs_sm, theme};
use crate::ui::{BtnKind, btn, icon, icon_button, platform_logo};

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
    /// The saved credentials shown in the clear, by platform and key.
    revealed: HashMap<(Platform, &'static str), String>,
    /// The credentials saved since the pane opened, by platform and key.
    just_saved: HashSet<(Platform, &'static str)>,
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
    fn toggle_credential(&mut self, platform: Platform, key: &'static str, cx: &mut Context<Self>) {
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
    fn hide_credential(&mut self, platform: Platform, key: &'static str, cx: &mut Context<Self>) {
        self.gateways.revealed.remove(&(platform, key));
        self.mask_credential(platform, key, true, cx);
    }

    fn mask_credential(&self, platform: Platform, key: &str, masked: bool, cx: &mut Context<Self>) {
        if let Some(area) = self.field_area(&format!("gw-{}-{key}", platform.id())) {
            area.update(cx, |a, cx| {
                a.masked = masked;
                cx.notify();
            });
        }
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

/// The one-time code a new person sends a bot to pair, the button that makes one, and who has
/// paired on `platform`.
fn pairing_row(
    d: &SettingsDialog,
    platform: Platform,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
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
    let names: Vec<&str> = d
        .gateways
        .users
        .iter()
        .filter(|u| u.platform == platform)
        .map(|u| u.name.as_str())
        .collect();
    let paired = (!names.is_empty()).then(|| {
        let text = format!("Paired with {}", names.join(", "));
        div()
            .id("gw-paired-now")
            .role(Role::Status)
            .aria_label(text.clone())
            .flex()
            .items_center()
            .gap(px(6.))
            .pb(px(8.))
            .text_color(t.success)
            .child(icon("check-circle", 15.).text_color(t.success))
            .child(text)
    });
    div()
        .flex()
        .flex_col()
        .child(pairing)
        .children(paired)
        .into_any_element()
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
    let switch_label = format!("Enable {label} gateway");
    let mut rows = vec![
        token_row("Enable gateway", None, &t)
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
        let shown = d.gateways.revealed.get(&(platform, key)).cloned();
        let revealed = shown.is_some();
        let field = d
            .field(
                format!("gw-{id}-{key}"),
                name.clone(),
                shown.as_deref().unwrap_or_default(),
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
            .w_full();
        // The field takes its own line under the label, wide enough for a whole token.
        rows.push(
            token_row(credential.label, None, &t)
                .flex_wrap()
                .when(saved, |row| {
                    let (icon, action) = if revealed {
                        ("eye-slash", "Hide")
                    } else {
                        ("eye", "Show")
                    };
                    row.child(
                        icon_button(
                            SharedString::from(format!("gw-{id}-{key}-show")),
                            format!("Show {name}"),
                            icon,
                            14.,
                            cx,
                        )
                        .aria_toggled(if revealed {
                            Toggled::True
                        } else {
                            Toggled::False
                        })
                        .tooltip(crate::ui::tooltip(format!(
                            "{action} {}",
                            credential.label.to_lowercase()
                        )))
                        .on_click(cx.listener(move |d, _, _, cx| {
                            d.toggle_credential(platform, key, cx);
                        })),
                    )
                })
                .child(field)
                .child(credential_actions(d, platform, key, &name, saved, cx))
                .into_any_element(),
        );
    }
    for field in platform.settings() {
        let key = field.key;
        let input = d
            .field(
                format!("gw-{id}-{key}"),
                format!("{label} {}", field.label.to_lowercase()),
                settings.value(key),
                field.placeholder,
                FieldKind {
                    mono: true,
                    ..Default::default()
                },
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
            )
            .w(px(FIELD_WIDTH))
            .flex_none();
        rows.push(
            token_row(field.label, None, &t)
                .child(input)
                .into_any_element(),
        );
    }
    rows
}

/// The buttons under a credential's field that clear it and save it, and whether it saved.
fn credential_actions(
    d: &SettingsDialog,
    platform: Platform,
    key: &'static str,
    name: &str,
    saved: bool,
    cx: &mut Context<SettingsDialog>,
) -> impl IntoElement {
    let t = *theme(cx);
    let id = platform.id();
    let field = SharedString::from(format!("gw-{id}-{key}"));
    let done = d.gateways.just_saved.contains(&(platform, key)).then(|| {
        div()
            .id(SharedString::from(format!("gw-{id}-{key}-saved")))
            .role(Role::Status)
            .aria_label(format!("{name} saved"))
            .flex()
            .items_center()
            .gap(px(4.))
            .text_size(px(fs_sm()))
            .text_color(t.success)
            .child(icon("check", 13.).text_color(t.success))
            .child("Saved")
    });
    let cleared = field.clone();
    div()
        .w_full()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(8.))
        .pt(px(6.))
        .children(done)
        .child(
            btn(
                SharedString::from(format!("gw-{id}-{key}-clear")),
                format!("Clear {name}"),
                BtnKind::Plain,
                false,
                &t,
            )
            .child("Clear")
            .on_click(cx.listener(move |d, _, _, cx| {
                if let Some(area) = d.field_area(&cleared) {
                    area.update(cx, |a, cx| a.set_text("", cx));
                }
                d.gateways.just_saved.remove(&(platform, key));
                if saved {
                    d.save_credential(platform, key, String::new(), cx);
                }
                cx.notify();
            })),
        )
        .child(
            btn(
                SharedString::from(format!("gw-{id}-{key}-save")),
                format!("Save {name}"),
                BtnKind::Primary,
                false,
                &t,
            )
            .child("Save")
            .on_click(cx.listener(move |d, _, _, cx| d.commit_field(&field, cx))),
        )
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
    let pairing = pairing_row(d, each, cx);
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
