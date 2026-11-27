//! Settings → Logs: how much each log records, and the folder that holds them.

// Rust guideline compliant 2026-11-23

use std::sync::Arc;

use gpui::{AnyElement, SharedString, div, prelude::*, px};
use patchwork_backend::settings::{LogLevel, LoggingSettings};

use crate::settings::SettingsDialog;
use crate::settings::controls::{Section, alert, choices, select_box, switch_row, token_row};
use patchwork_ui_theme::theme;
use patchwork_ui_components::{BtnKind, btn};

/// The log levels, from the most to the least recorded.
pub const LOG_LEVELS: [(LogLevel, &str); 6] = [
    (LogLevel::Trace, "Trace"),
    (LogLevel::Debug, "Debug"),
    (LogLevel::Info, "Info"),
    (LogLevel::Warning, "Warning"),
    (LogLevel::Error, "Error"),
    (LogLevel::Off, "Off"),
];

/// The log files Patchwork writes: their title, what each holds, and its field of the settings.
type LogField = fn(&mut LoggingSettings) -> &mut LogLevel;
const LOGS: [(&str, LogField); 6] = [
    ("Main", |l| &mut l.app),
    ("Window", |l| &mut l.window),
    ("Agents", |l| &mut l.agents),
    ("Imports", |l| &mut l.imports),
    ("Themes", |l| &mut l.themes),
    ("Chats", |l| &mut l.chats),
];

/// The pane's own state: what the log buttons are doing.
#[derive(Debug, Default)]
pub struct LogsState {
    exporting: bool,
    error: Option<String>,
}

impl SettingsDialog {
    /// Asks where to save a zip of the logs, and writes it there.
    fn export_logs(&mut self, cx: &mut Context<Self>) {
        let stamp = chrono::Local::now().format("%Y%m%dT%H%M%S");
        let home = patchwork_platform::files::paths::home_dir().unwrap_or_default();
        let picked = cx.prompt_for_new_path(&home, Some(&format!("patchwork-logs-{stamp}.zip")));
        let core = Arc::clone(&self.core);
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let _ = this.update(cx, |d, cx| {
                d.logs.error = None;
                d.logs.exporting = true;
                cx.notify();
            });
            let exported = cx
                .background_spawn(async move { core.export_logs(&path) })
                .await;
            let _ = this.update(cx, |d, cx| {
                d.logs.exporting = false;
                d.logs.error = exported.err().map(|e| e.to_string());
                cx.notify();
            });
        })
        .detach();
    }
}

pub fn render(d: &mut SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let s = &d.settings;
    let log_rows = LOGS.iter().enumerate().map(|(i, &(title, field))| {
        let mut logging = s.logging.clone();
        let level = *field(&mut logging);
        let current = LOG_LEVELS
            .iter()
            .find(|(l, _)| *l == level)
            .map_or("", |(_, label)| label);
        let key = SharedString::from(format!("log-{i}"));
        let open = d.menu_open(&key);
        token_row(title, &t).child(
            select_box(
                key.clone(),
                format!("{title} log level: {current}"),
                current,
                open,
                &t,
            )
            .on_click(choices(
                key,
                format!("{title} log level"),
                LOG_LEVELS
                    .iter()
                    .map(|&(l, label)| (l, label.into()))
                    .collect(),
                level,
                move |d, l, cx| d.update(|s| *field(&mut s.logging) = l, cx),
                cx,
            )),
        )
    });
    let log_rows: Vec<_> = log_rows.collect();

    let exporting = d.logs.exporting;
    let levels = Section::new("Log levels").children(log_rows);
    let folder = Section::new("Log folder")
        .child(switch_row("chat-menu", "Open logs in chat menus", s.logging.chat_menu,
            |d, on, cx| d.update(|s| s.logging.chat_menu = on, cx),
            cx,
        ))
        .child(
            token_row(
                "Log folder",&t,
            )
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
                    .on_click(|_, _, cx| match patchwork_backend::logging::logs_folder() {
                        Ok(folder) => cx.open_with_system(&folder),
                        Err(e) => {
                            log::warn!(error:% = e; "could not open the logs folder: {{error}}");
                        }
                    }),
            ),
        );
    div()
        .flex()
        .flex_col()
        .children(
            d.logs
                .error
                .clone()
                .map(|e| alert("log-error", e, t.danger).pt_0().pb(px(12.))),
        )
        .child(levels)
        .child(folder)
        .into_any_element()
}
