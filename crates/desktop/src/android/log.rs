//! The app's log under the devices: the lines the device's log holds for the project's app,
//! followed as they are written.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AnyElement, ClickEvent, ClipboardItem, FocusHandle, FontWeight, KeyDownEvent, Role,
    ScrollStrategy, SharedString, Task, UniformListScrollHandle, Window, div, prelude::*, px,
    uniform_list,
};
use patchwork_server::android::{LogLevel, LogLine, Logcat};

use super::state::AndroidPanel;
use crate::ui::menu::MenuItem;
use crate::ui::theme::{font_mono, FS_SM, FS_XS, Theme};
use crate::ui::{dropdown_chip, icon_button, tooltip};

/// How often the log is read again: often enough to look live, seldom enough to stay cheap
/// while an app writes thousands of lines.
const POLL: Duration = Duration::from_millis(300);

/// The log's height in the panel, about twelve lines.
const HEIGHT: f32 = 220.;

/// A line's height, which every line shares so the list draws only the ones in view.
const LINE: f32 = 16.;

/// How many lines Page Up and Page Down move by: most of what is in view.
const PAGE: usize = 10;

/// The levels a log can be kept to, least serious first, with their names.
const LEVELS: [(LogLevel, &str); 5] = [
    (LogLevel::Verbose, "Verbose"),
    (LogLevel::Debug, "Debug"),
    (LogLevel::Info, "Info"),
    (LogLevel::Warn, "Warnings"),
    (LogLevel::Error, "Errors"),
];

/// The log of one device, while it shows.
pub(super) struct LogPane {
    /// The device it reads, which the panel compares to the device picked.
    pub(super) serial: String,
    logcat: Arc<Logcat>,
    /// The lines at `level` or above.
    lines: Vec<LogLine>,
    level: LogLevel,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    /// The line the keyboard last scrolled to.
    at: usize,
    _poll: Task<()>,
}

fn level_name(level: LogLevel) -> &'static str {
    LEVELS
        .iter()
        .find(|(of, _)| *of == level)
        .map_or("Verbose", |(_, name)| name)
}

impl AndroidPanel {
    pub(super) fn toggle_log(&mut self, cx: &mut Context<Self>) {
        if self.log.take().is_none() {
            self.open_log(cx);
        }
        cx.notify();
    }

    /// Follows the log of the device picked, kept to the app as the variant picked installs it.
    pub(super) fn open_log(&mut self, cx: &mut Context<Self>) {
        let Some(device) = self.picked_device() else {
            self.failure = Some("Start or connect the device picked to read its log.".into());
            return cx.notify();
        };
        let (server, serial) = (Arc::clone(&self.server), device.serial.clone());
        let (project, variant) = (self.project.clone(), self.variant.clone());
        let task = cx.background_spawn({
            let serial = serial.clone();
            async move { server.android_logcat(&project, &serial, &variant) }
        });
        cx.spawn(async move |this, cx| {
            let opened = task.await;
            this.update(cx, |this, cx| {
                match opened {
                    Ok(logcat) => this.log = Some(LogPane::new(serial, logcat, cx)),
                    Err(e) => this.failure = Some(e),
                }
                cx.notify();
            })
        })
        .detach();
    }

    /// Keeps the log on the device picked: it follows the pick to another running device, and
    /// closes when the device is gone.
    pub(super) fn sync_log(&mut self, cx: &mut Context<Self>) {
        let Some(log) = &self.log else {
            return;
        };
        let picked = self.picked_device().map(|device| device.serial.clone());
        if picked.as_ref() == Some(&log.serial) {
            return;
        }
        self.log = None;
        if picked.is_some() {
            self.open_log(cx);
        }
    }

    fn open_level_menu(&mut self, e: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(current) = self.log.as_ref().map(|log| log.level) else {
            return;
        };
        let this = cx.entity().downgrade();
        let items = LEVELS
            .iter()
            .map(|(level, name)| {
                let (this, level) = (this.clone(), *level);
                MenuItem::new(*name, move |_, cx| {
                    let _ = this.update(cx, |this, cx| {
                        if let Some(log) = &mut this.log {
                            log.level = level;
                        }
                        cx.notify();
                    });
                })
                .radio(level == current)
            })
            .collect();
        self.open_menu("Lowest level shown", items, e, window, cx);
    }

    fn log_key(&mut self, e: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(log) = &mut self.log else {
            return;
        };
        let last = log.lines.len().saturating_sub(1);
        let at = log.at.min(last);
        log.at = match e.keystroke.key.as_str() {
            "up" => at.saturating_sub(1),
            "down" => (at + 1).min(last),
            "pageup" => at.saturating_sub(PAGE),
            "pagedown" => (at + PAGE).min(last),
            "home" => 0,
            "end" => last,
            _ => return,
        };
        if log.at == last {
            log.scroll.scroll_to_bottom();
        } else {
            log.scroll.scroll_to_item(log.at, ScrollStrategy::Top);
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// The log drawn under the devices and the screen.
    pub(super) fn log_view(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let log = self.log.as_ref()?;
        if self.create.is_some() || self.sheet.is_some() {
            return None;
        }
        let t = *t;
        let list = uniform_list(
            "android-log-lines",
            log.lines.len(),
            cx.processor(move |this, range: std::ops::Range<usize>, _, _| {
                let Some(log) = &this.log else {
                    return Vec::new();
                };
                range
                    .filter_map(|i| log.lines.get(i))
                    .map(|line| {
                        let color = match line.level {
                            Some(LogLevel::Error | LogLevel::Fatal) => t.danger,
                            Some(LogLevel::Warn) => t.warning,
                            Some(_) => t.text,
                            None => t.text_muted,
                        };
                        div()
                            .h(px(LINE))
                            .truncate()
                            .text_color(color)
                            .child(SharedString::from(line.text.clone()))
                            .into_any_element()
                    })
                    .collect()
            }),
        )
        .track_scroll(&log.scroll)
        .size_full();
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .min_h(px(36.))
            .pl(px(20.))
            .pr(px(14.))
            .child(
                div()
                    .id("android-log-title")
                    .role(Role::Heading)
                    .flex_none()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Log"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .pl(px(4.))
                    .text_size(px(FS_SM))
                    .text_color(t.text_muted)
                    .children(log.logcat.package().map(str::to_string)),
            )
            .child(
                dropdown_chip(
                    "android-log-level",
                    format!("Lowest level shown: {}", level_name(log.level)),
                    level_name(log.level),
                    self.menu_open("Lowest level shown"),
                    &t,
                )
                .on_click(cx.listener(Self::open_level_menu)),
            )
            .child(
                icon_button("android-log-copy", "Copy the log", "copy", 14., cx)
                    .tooltip(tooltip("Copy"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(log) = &this.log {
                            let text: Vec<&str> =
                                log.lines.iter().map(|line| line.text.as_str()).collect();
                            cx.write_to_clipboard(ClipboardItem::new_string(text.join("\n")));
                        }
                    })),
            )
            .child(
                icon_button("android-log-clear", "Clear the log", "prohibit", 14., cx)
                    .tooltip(tooltip("Clear"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(log) = &this.log {
                            let logcat = Arc::clone(&log.logcat);
                            cx.background_spawn(async move { logcat.clear() }).detach();
                        }
                    })),
            )
            .child(
                icon_button("android-log-close", "Close the log", "x", 14., cx)
                    .tooltip(tooltip("Close"))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_log(cx))),
            );
        Some(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .h(px(HEIGHT))
                .border_t_1()
                .border_color(t.border)
                .child(header)
                .child(
                    div()
                        .id("android-log")
                        .role(Role::Log)
                        .aria_label("App log")
                        .track_focus(&log.focus)
                        .tab_index(0)
                        .on_key_down(cx.listener(Self::log_key))
                        .flex_1()
                        .min_h_0()
                        .px(px(20.))
                        .pb(px(8.))
                        .font_family(font_mono())
                        .text_size(px(FS_XS))
                        .child(list),
                )
                .into_any_element(),
        )
    }
}

impl LogPane {
    fn new(serial: String, logcat: Logcat, cx: &mut Context<AndroidPanel>) -> Self {
        let logcat = Arc::new(logcat);
        let poll = cx.spawn({
            let logcat = Arc::clone(&logcat);
            async move |this, cx| {
                let mut seen = None;
                let mut shown = None;
                loop {
                    let Ok(Some(level)) =
                        this.update(cx, |this, _| this.log.as_ref().map(|log| log.level))
                    else {
                        break;
                    };
                    // Another level shows other lines of the same log.
                    if shown != Some(level) {
                        (seen, shown) = (None, Some(level));
                    }
                    let logcat = Arc::clone(&logcat);
                    let read = cx
                        .background_spawn(async move {
                            let (mut lines, revision) = logcat.lines(seen)?;
                            lines.retain(|line| line.level.is_none_or(|of| of >= level));
                            Some((lines, revision))
                        })
                        .await;
                    if let Some((lines, revision)) = read {
                        seen = Some(revision);
                        let applied = this.update(cx, |this, cx| {
                            if let Some(log) = &mut this.log {
                                let at_end = log.scroll.is_scrolled_to_end().unwrap_or(true);
                                log.lines = lines;
                                if at_end {
                                    log.scroll.scroll_to_bottom();
                                    log.at = log.lines.len().saturating_sub(1);
                                }
                                cx.notify();
                            }
                        });
                        if applied.is_err() {
                            break;
                        }
                    }
                    cx.background_executor().timer(POLL).await;
                }
            }
        });
        Self {
            serial,
            logcat,
            lines: Vec::new(),
            level: LogLevel::Verbose,
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            at: 0,
            _poll: poll,
        }
    }
}
