//! Settings → General: the app icon, which finished work raises a notification, and below them,
//! how much each log records.

// Rust guideline compliant 2026-10-09

use std::sync::{Arc, OnceLock};

use gpui::{AnyElement, FocusHandle, FontWeight, Role, SharedString, Toggled, div, img, prelude::*, px};
use patchwork_server::settings::{LogLevel, LoggingSettings};

use crate::settings::SettingsDialog;
use crate::settings::controls::{alert, choice, section_heading, switch, token_row};
use crate::ui::theme::{RADIUS_SM, theme};
use crate::ui::{BtnKind, btn, dropdown_chip, tooltip};

/// The log levels, from the most to the least recorded.
pub(crate) const LOG_LEVELS: [(LogLevel, &str); 6] = [
    (LogLevel::Trace, "Trace"),
    (LogLevel::Debug, "Debug"),
    (LogLevel::Info, "Info"),
    (LogLevel::Warning, "Warning"),
    (LogLevel::Error, "Error"),
    (LogLevel::Off, "Off"),
];

/// The log files Patchwork writes: their title, what each holds, and its field of the settings.
type LogField = fn(&mut LoggingSettings) -> &mut LogLevel;
const LOGS: [(&str, &str, LogField); 5] = [
    ("Main", "Patchwork's own messages, in main.log", |l| {
        &mut l.app
    }),
    (
        "Window",
        "The window's errors and warnings, in window.log",
        |l| &mut l.window,
    ),
    ("Agents", "Each agent's runs, in agent-<agent>.log", |l| {
        &mut l.agents
    }),
    ("Imports", "Each scan and import, in imports.log", |l| {
        &mut l.imports
    }),
    (
        "Chats",
        "Each chat with a warning or error, in chat-<id>.log",
        |l| &mut l.threads,
    ),
];

/// The pane's own state: what the log buttons are doing.
#[derive(Default)]
pub struct GeneralState {
    /// The section list, so Up and Down move along it while it has focus.
    pub nav_focus: Option<FocusHandle>,
    exporting: bool,
    log_error: Option<String>,
}

/// One app icon of `assets/logos/manifest.json`.
struct Logo {
    id: String,
    name: String,
}

/// The app icon picked in General, as an asset path.
pub(super) fn app_icon(s: &patchwork_server::settings::Settings) -> SharedString {
    let id = s.logo.clone().unwrap_or_else(|| logos().1.clone());
    SharedString::from(format!("app-icons/{id}.svg"))
}

/// The app icons, and the one used when none is picked.
fn logos() -> &'static (Vec<Logo>, String) {
    static LOGOS: OnceLock<(Vec<Logo>, String)> = OnceLock::new();
    LOGOS.get_or_init(|| {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../../../assets/logos/manifest.json"))
                .unwrap_or_default();
        let text =
            |v: &serde_json::Value, key: &str| v[key].as_str().unwrap_or_default().to_string();
        let icons = manifest["icons"]
            .as_array()
            .map(|icons| {
                icons
                    .iter()
                    .map(|i| Logo {
                        id: text(i, "id"),
                        name: text(i, "name"),
                    })
                    .collect()
            })
            .unwrap_or_default();
        (icons, text(&manifest, "default"))
    })
}

impl SettingsDialog {
    /// Asks where to save a zip of the logs, and writes it there.
    fn export_logs(&mut self, cx: &mut Context<Self>) {
        let stamp = chrono::Local::now().format("%Y%m%dT%H%M%S");
        let home = patchwork_server::system::fs::home_dir().unwrap_or_default();
        let picked = cx.prompt_for_new_path(&home, Some(&format!("patchwork-logs-{stamp}.zip")));
        let server = Arc::clone(&self.server);
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let _ = this.update(cx, |d, cx| {
                d.general.log_error = None;
                d.general.exporting = true;
                cx.notify();
            });
            let exported = cx
                .background_spawn(async move {
                    let mut system = patchwork_server::logging::system::report().clone();
                    system["settings"] = serde_json::json!(server.store.settings());
                    patchwork_server::logging::export(&path, &system)
                })
                .await;
            let _ = this.update(cx, |d, cx| {
                d.general.exporting = false;
                d.general.log_error = exported.err().map(|e| e.to_string());
                cx.notify();
            });
        })
        .detach();
    }
}

pub fn render(d: &mut SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let s = &d.settings;
    let notifications = s.notifications.clone();
    let (logos, default_logo) = logos();
    let logo = s.logo.clone().unwrap_or_else(|| default_logo.clone());

    let toggle_row = |id: &'static str,
                      title: &'static str,
                      desc: Option<&'static str>,
                      on: bool,
                      flip: fn(&mut patchwork_server::settings::Settings, bool),
                      cx: &mut Context<SettingsDialog>| {
        token_row(title, desc, &t).child(
            switch(id, title, on, &t)
                .on_click(cx.listener(move |d, _, _, cx| d.update(|s| flip(s, !on), cx))),
        )
    };

    let icons = div()
        .id("app-icons")
        .role(Role::RadioGroup)
        .aria_label("App icon")
        .mt(px(8.))
        .flex()
        .flex_col()
        .gap(px(4.))
        .children(logos.chunks(8).map(|row| {
            div().flex().gap(px(4.)).children(row.iter().map(|l| {
                let checked = l.id == logo;
                let id = l.id.clone();
                div()
                    .id(SharedString::from(format!("logo-{}", l.id)))
                    .role(Role::RadioButton)
                    .aria_label(l.name.clone())
                    .aria_toggled(if checked {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
                    .tab_index(0)
                    .focus_visible(crate::ui::focus_ring)
                    .cursor_pointer()
                    .size(px(82.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(RADIUS_SM))
                    .border_1()
                    .map(|b| {
                        if checked {
                            b.border_color(t.accent).bg(t.bg_accent_soft)
                        } else {
                            b.border_color(gpui::transparent_black())
                                .hover(move |s| s.bg(t.bg_hover))
                        }
                    })
                    .tooltip(tooltip(l.name.clone()))
                    .child(img(SharedString::from(format!("app-icons/{}.svg", l.id))).size(px(64.)))
                    .on_click(cx.listener(move |d, _, _, cx| {
                        let id = id.clone();
                        d.update(|s| s.logo = Some(id), cx);
                    }))
            }))
        }));

    let log_rows = LOGS.iter().enumerate().map(|(i, &(title, desc, field))| {
        let mut logging = s.logging.clone();
        let level = *field(&mut logging);
        let current = LOG_LEVELS
            .iter()
            .find(|(l, _)| *l == level)
            .map_or("", |(_, label)| label);
        let key = SharedString::from(format!("log-{i}"));
        let open = d.menu_open(&key);
        token_row(title, Some(desc), &t).child(
            dropdown_chip(
                key.clone(),
                format!("{title} log level: {current}"),
                current,
                open,
                &t,
            )
            .on_click(cx.listener(move |d, e: &gpui::ClickEvent, window, cx| {
                let this = cx.entity().downgrade();
                let items = LOG_LEVELS
                    .iter()
                    .map(|&(l, label)| {
                        choice(this.clone(), label, l == level, move |d, cx| {
                            d.update(|s| *field(&mut s.logging) = l, cx);
                        })
                    })
                    .collect();
                let at = e.position();
                d.toggle_menu(
                    key.clone(),
                    format!("{title} log level"),
                    items,
                    at,
                    window,
                    cx,
                );
            })),
        )
    });
    let log_rows: Vec<_> = log_rows.collect();

    let exporting = d.general.exporting;
    let logs_dir = patchwork_server::system::dirs().logs.clone();
    div()
        .flex()
        .flex_col()
        .child(section_heading("Appearance", true, &t))
        .child(
            div()
                .border_b_1()
                .border_color(t.border)
                .py(px(10.))
                .child(div().font_weight(FontWeight::SEMIBOLD).child("App icon"))
                .child(icons),
        )
        .child(section_heading("Notifications", false, &t))
        .child(toggle_row(
            "goal-complete",
            "Goal completed",
            Some("When a task meets its goal, while Patchwork is not the focused window"),
            notifications.goal_complete,
            |s, on| s.notifications.goal_complete = on,
            cx,
        ))
        .child(toggle_row(
            "task-complete",
            "Task completed",
            Some("When an agent finishes replying in a task without a running goal"),
            notifications.task_complete,
            |s, on| s.notifications.task_complete = on,
            cx,
        ))
        .child(section_heading("Logs", false, &t))
        .children(log_rows)
        .child(toggle_row(
            "thread-menu",
            "Open logs in chat menus",
            None,
            s.logging.thread_menu,
            |s, on| s.logging.thread_menu = on,
            cx,
        ))
        .child(
            token_row("Log folder", None, &t)
                .child(
                    btn("export-logs", "Export…", BtnKind::Plain, exporting, &t)
                        .child(if exporting {
                            "Exporting…"
                        } else {
                            "Export…"
                        })
                        .when(!exporting, |b| {
                            b.on_click(cx.listener(|d, _, _, cx| d.export_logs(cx)))
                        }),
                )
                .child(
                    btn("open-logs", "Open", BtnKind::Plain, false, &t)
                        .child("Open")
                        .on_click(move |_, _, cx| {
                            let _ = std::fs::create_dir_all(&logs_dir);
                            cx.open_with_system(&logs_dir);
                        }),
                ),
        )
        .children(
            d.general
                .log_error
                .clone()
                .map(|e| alert("log-error", e, t.danger)),
        )
        .into_any_element()
}
