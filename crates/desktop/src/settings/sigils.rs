//! Settings → Shortcuts → Sigils: the character that opens each of
//! the composer's menus. Clicking one takes the next symbol typed as its new one.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, KeyDownEvent, SharedString, div, prelude::*, px};

use crate::settings::SettingsDialog;
use crate::ui::sigils::{SIGIL_KINDS, Sigils, default_sigil, save};
use crate::ui::theme::{FS_SM, RADIUS_SM, Theme};
use crate::ui::{button, icon, tooltip};

pub const SIGILS_PAGE: &str = "Sigils";

impl SettingsDialog {
    pub fn sigils(&self, cx: &Context<Self>) -> Sigils {
        cx.try_global::<Sigils>().cloned().unwrap_or_default()
    }

    pub fn save_sigils(&mut self, sigils: Sigils, cx: &mut Context<Self>) {
        save(&self.server, sigils, cx);
        cx.notify();
    }

    /// Takes the symbol typed while a sigil is being recorded. True when one is.
    pub fn record_sigil(&mut self, e: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let Some(kind) = self.shortcuts.recording_sigil else {
            return false;
        };
        if e.keystroke.key == "escape" {
            self.shortcuts.recording_sigil = None;
            cx.notify();
            return true;
        }
        let typed = e.keystroke.key_char.as_deref().unwrap_or(&e.keystroke.key);
        let mut chars = typed.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return true;
        };
        let sigils = self.sigils(cx);
        if let Some(problem) = sigils.problem(kind, c) {
            self.shortcuts.refused = Some(problem);
        } else {
            self.shortcuts.recording_sigil = None;
            self.save_sigils(sigils.with(kind, c), cx);
        }
        cx.notify();
        true
    }
}

/// The sigil rows to show: all of them on their page, or the ones a search matches.
pub fn rows(
    d: &SettingsDialog,
    page: &str,
    query: &str,
    t: &Theme,
    cx: &mut Context<SettingsDialog>,
) -> Vec<AnyElement> {
    let sigils = d.sigils(cx);
    let (hover, muted, fg) = (t.bg_hover, t.text_muted, t.text);
    SIGIL_KINDS
        .iter()
        .filter(|&&(kind, _, title)| {
            if query.is_empty() {
                page == SIGILS_PAGE
            } else {
                title.to_lowercase().contains(query)
                    || SIGILS_PAGE.to_lowercase().contains(query)
                    || sigils.of(kind).to_string() == query
            }
        })
        .map(|&(kind, key, title)| {
            let recording = d.shortcuts.recording_sigil == Some(kind);
            let changed = sigils.of(kind) != default_sigil(kind);
            div()
                .border_b_1()
                .border_color(t.border)
                .py(px(6.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(div().min_w_0().flex_1().text_color(fg).child(title))
                        .when(changed && !recording, |r| {
                            r.child(
                                button(
                                    SharedString::from(format!("reset-sigil-{key}")),
                                    format!("Reset sigil for {title}"),
                                )
                                .size(px(24.))
                                .justify_center()
                                .rounded(px(RADIUS_SM))
                                .text_color(muted)
                                .hover(move |s| s.bg(hover).text_color(fg))
                                .child(icon("arrow-counter-clockwise", 12.))
                                .tooltip(tooltip(format!("Reset to {}", default_sigil(kind))))
                                .on_click(cx.listener(
                                    move |d, _, _, cx| {
                                        let next = d.sigils(cx).reset(kind);
                                        d.save_sigils(next, cx);
                                    },
                                )),
                            )
                        })
                        .child(
                            button(
                                SharedString::from(format!("change-sigil-{key}")),
                                format!("Change sigil for {title}"),
                            )
                            .rounded(px(RADIUS_SM))
                            .border_1()
                            .px(px(8.))
                            .py(px(4.))
                            .map(|b| {
                                if recording {
                                    b.border_color(t.accent).bg(t.bg_accent_soft)
                                } else {
                                    b.border_color(gpui::transparent_black())
                                        .hover(move |s| s.bg(hover))
                                }
                            })
                            .tooltip(tooltip("Change sigil"))
                            .map(|b| {
                                if recording {
                                    b.child(
                                        div()
                                            .text_size(px(FS_SM))
                                            .text_color(t.accent_text)
                                            .child("Type a symbol…"),
                                    )
                                } else {
                                    b.child(super::shortcuts::kbd(
                                        &[sigils.of(kind).to_string()],
                                        t,
                                    ))
                                }
                            })
                            .on_click(cx.listener(
                                move |d, _, window, cx| {
                                    d.shortcuts.refused = None;
                                    d.shortcuts.recording = None;
                                    d.shortcuts.recording_sigil =
                                        if d.shortcuts.recording_sigil == Some(kind) {
                                            None
                                        } else {
                                            Some(kind)
                                        };
                                    window.focus(&d.focus, cx);
                                    cx.notify();
                                },
                            )),
                        ),
                )
                .when(recording, |r| {
                    r.children(d.shortcuts.refused.clone().map(|e| {
                        div()
                            .mt(px(2.))
                            .text_right()
                            .text_size(px(FS_SM))
                            .text_color(t.danger)
                            .child(e)
                    }))
                })
                .into_any_element()
        })
        .collect()
}
