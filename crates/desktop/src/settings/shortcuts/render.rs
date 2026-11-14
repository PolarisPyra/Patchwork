//! The shortcuts pane: its state, recording a key, and the list drawn.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, Entity, Focusable, FontWeight, KeyDownEvent, Role, SharedString, Window, div,
    prelude::*, px,
};

use crate::ui::text_area::{Changed, TextArea};
use crate::settings::SettingsDialog;
use crate::settings::sigils::SIGILS_PAGE;
use crate::ui::theme::{fs_md, fs_sm, fs_xs, radius_sm, Theme, theme};
use crate::ui::{BtnKind, btn, button, icon, tooltip};
use super::keys::{
    COMMANDS, Command, DEFAULT_RULES, FIXED_KEYS, Overrides, PAGES, STATE_KEY, bind_keys,
    conflicts, format_key, key_for, key_from_event, key_problem, load,
};

/// The pane's state: the page, the search, and the command whose key is being recorded.
pub struct ShortcutsState {
    page: &'static str,
    search: Option<Entity<TextArea>>,
    overrides: Option<Overrides>,
    pub recording: Option<&'static str>,
    pub recording_sigil: Option<crate::ui::sigils::SigilKind>,
    pub refused: Option<String>,
    fixed_note: Option<&'static str>,
}

impl Default for ShortcutsState {
    fn default() -> Self {
        Self {
            page: PAGES[0],
            search: None,
            overrides: None,
            recording: None,
            recording_sigil: None,
            refused: None,
            fixed_note: None,
        }
    }
}

impl SettingsDialog {
    fn save_keys(&mut self, overrides: Overrides, cx: &mut Context<Self>) {
        let json = serde_json::to_string(&overrides).unwrap_or_default();
        if let Err(e) = self.server.store.set_state(STATE_KEY, &json) {
            self.error = Some(e.to_string());
        }
        self.shortcuts.overrides = Some(overrides);
        bind_keys(&self.server, cx);
        cx.notify();
    }

    fn set_key(&mut self, id: &str, key: Option<String>, cx: &mut Context<Self>) {
        let mut overrides = self.shortcuts.overrides.clone().unwrap_or_default();
        let default = DEFAULT_RULES
            .iter()
            .find(|(c, _, _)| *c == id)
            .map(|(_, k, _)| k.to_string());
        if key == default {
            overrides.remove(id);
        } else {
            overrides.insert(id.to_string(), key);
        }
        self.save_keys(overrides, cx);
    }

    /// Takes the key pressed while one is being recorded. True when it was used.
    pub fn record_key(&mut self, e: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.shortcuts.recording else {
            return false;
        };
        let Some(key) = key_from_event(e) else {
            return true;
        };
        if key == "escape" {
            self.shortcuts.recording = None;
        } else if let Some(problem) = key_problem(id, &key) {
            self.shortcuts.refused = Some(problem);
        } else {
            self.shortcuts.recording = None;
            self.set_key(id, Some(key), cx);
        }
        cx.notify();
        true
    }
}

pub fn kbd(parts: &[String], t: &Theme) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.))
        .children(parts.iter().map(|p| {
            div()
                .min_w(px(20.))
                .h(px(20.))
                .flex()
                .items_center()
                .justify_center()
                .px(px(5.))
                .rounded(px(radius_sm()))
                .bg(t.bg_selected_muted)
                .text_size(px(fs_xs()))
                .font_weight(FontWeight::MEDIUM)
                .text_color(t.text_muted)
                .child(p.clone())
        }))
}

pub fn render(
    d: &mut SettingsDialog,
    window: &Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    if d.shortcuts.overrides.is_none() {
        d.shortcuts.overrides = Some(load(&d.server));
    }
    let search = if let Some(search) = &d.shortcuts.search {
        search.clone()
    } else {
        let search = cx.new(|cx| {
            let mut area = TextArea::new("Search by name or key", "Search shortcuts", cx);
            area.single_line = true;
            area.text_color = t.text;
            area.placeholder_color = crate::ui::theme::input_placeholder();
            area.selection_color = t.accent.opacity(0.3);
            area
        });
        cx.subscribe(&search, |_, _, _: &Changed, cx| cx.notify())
            .detach();
        d.shortcuts.search = Some(search.clone());
        search
    };
    let query = search.read(cx).content.trim().to_lowercase();
    let searching = !query.is_empty();
    let search_focused = search
        .read(cx)
        .focus_handle(cx)
        .contains_focused(window, cx);
    let overrides = d.shortcuts.overrides.clone().unwrap_or_default();
    let page = d.shortcuts.page;
    let matches = |texts: &[&str]| texts.iter().any(|x| x.to_lowercase().contains(&query));

    let mut sections = Vec::new();
    for p in PAGES.iter().filter(|p| searching || **p == page) {
        let commands: Vec<&Command> = COMMANDS
            .iter()
            .filter(|c| c.page == *p && !c.fixed)
            .filter(|c| {
                let key = key_for(&overrides, c.id)
                    .map(|k| format_key(&k).join(" "))
                    .unwrap_or_default();
                !searching || matches(&[c.title, c.desc.unwrap_or_default(), p, &key])
            })
            .collect();
        let keys: Vec<_> = FIXED_KEYS
            .iter()
            .filter(|(fp, keys, action)| {
                *fp == *p && (!searching || matches(&[action, p, &keys.join(" ")]))
            })
            .collect();
        if !commands.is_empty() || !keys.is_empty() {
            sections.push((*p, commands, keys));
        }
    }
    let has_commands = sections.iter().any(|(_, c, _)| !c.is_empty());
    let (hover, muted, fg) = (t.bg_hover, t.text_muted, t.text);

    let sections: Vec<_> = sections
        .into_iter()
        .enumerate()
        .map(|(i, (p, commands, keys))| {
            let rows = commands.into_iter().map(|c| {
                let key = key_for(&overrides, c.id);
                let changed = overrides.contains_key(c.id);
                let recording = d.shortcuts.recording == Some(c.id);
                let clash = conflicts(&overrides, c.id);
                let id = c.id;
                let group = SharedString::from(format!("shortcut-{id}"));
                let small = |name: &str, label: String, glyph: &str| {
                    button(SharedString::from(format!("{name}-{id}")), label)
                        .size(px(24.))
                        .justify_center()
                        .rounded(px(radius_sm()))
                        .text_color(muted)
                        .hover(move |s| s.bg(hover).text_color(fg))
                        .child(icon(glyph, 12.))
                };
                div()
                    .group(group.clone())
                    .border_b_1()
                    .border_color(t.border)
                    .py(px(6.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .child(div().text_color(t.text).child(c.title))
                                    .children(c.desc.map(|desc| {
                                        div().text_size(px(fs_sm())).text_color(muted).child(desc)
                                    })),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_none()
                                    .items_center()
                                    .justify_end()
                                    .when(key.is_some() && !recording, |r| {
                                        r.child(
                                            small(
                                                "unbind",
                                                format!("Remove shortcut for {}", c.title),
                                                "x",
                                            )
                                            .opacity(0.)
                                            .group_hover(group.clone(), |s| s.opacity(1.))
                                            .focus(|s| s.opacity(1.))
                                            .tooltip(tooltip("Remove shortcut"))
                                            .on_click(
                                                cx.listener(move |d, _, _, cx| {
                                                    d.set_key(id, None, cx);
                                                }),
                                            ),
                                        )
                                    })
                                    .when(changed && !recording, |r| {
                                        r.child(
                                            small(
                                                "reset",
                                                format!("Reset shortcut for {}", c.title),
                                                "arrow-counter-clockwise",
                                            )
                                            .tooltip(tooltip("Reset to default"))
                                            .on_click(
                                                cx.listener(move |d, _, _, cx| {
                                                    let mut overrides = d
                                                        .shortcuts
                                                        .overrides
                                                        .clone()
                                                        .unwrap_or_default();
                                                    overrides.remove(id);
                                                    d.save_keys(overrides, cx);
                                                }),
                                            ),
                                        )
                                    }),
                            )
                            .child(
                                button(
                                    SharedString::from(format!("change-{id}")),
                                    format!("Change shortcut for {}", c.title),
                                )
                                .rounded(px(radius_sm()))
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
                                .tooltip(tooltip("Change shortcut"))
                                .map(|b| match (&key, recording) {
                                    (_, true) => b.child(
                                        div()
                                            .text_size(px(fs_sm()))
                                            .text_color(t.accent_text)
                                            .child("Press keys…"),
                                    ),
                                    (Some(key), false) => b.child(kbd(&format_key(key), &t)),
                                    (None, false) => b.child(
                                        div()
                                            .text_size(px(fs_sm()))
                                            .text_color(t.text_subtle)
                                            .child("Unbound"),
                                    ),
                                })
                                .on_click(cx.listener(
                                    move |d, _, window, cx| {
                                        d.shortcuts.refused = None;
                                        d.shortcuts.recording_sigil = None;
                                        d.shortcuts.recording = if d.shortcuts.recording == Some(id)
                                        {
                                            None
                                        } else {
                                            Some(id)
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
                                .id("shortcut-refused")
                                .role(Role::Alert)
                                .mt(px(2.))
                                .text_right()
                                .text_size(px(fs_sm()))
                                .text_color(t.danger)
                                .child(e)
                        }))
                    })
                    .when(!recording && !clash.is_empty(), |r| {
                        let names: Vec<String> = clash
                            .iter()
                            .map(|x| format!("{}: {}", x.page, x.title))
                            .collect();
                        r.child(
                            div()
                                .mt(px(2.))
                                .text_right()
                                .text_size(px(fs_sm()))
                                .text_color(t.warning)
                                .child(format!("Also used by {}", names.join(", "))),
                        )
                    })
            });
            let fixed_rows = keys.into_iter().map(|&(_, keys, action)| {
                let noted = d.shortcuts.fixed_note == Some(action);
                let parts: Vec<String> =
                    keys.iter().map(std::string::ToString::to_string).collect();
                div()
                    .border_b_1()
                    .border_color(t.border)
                    .py(px(6.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().min_w_0().flex_1().text_color(muted).child(action))
                            .child(
                                button(
                                    SharedString::from(format!("fixed-{action}")),
                                    format!("{action}: fixed shortcut"),
                                )
                                .rounded(px(radius_sm()))
                                .border_1()
                                .border_color(gpui::transparent_black())
                                .px(px(8.))
                                .py(px(4.))
                                .hover(move |s| s.bg(hover))
                                .tooltip(tooltip("Fixed: this shortcut cannot be changed"))
                                .child(kbd(&parts, &t))
                                .on_click(cx.listener(
                                    move |d, _, _, cx| {
                                        d.shortcuts.fixed_note =
                                            if d.shortcuts.fixed_note == Some(action) {
                                                None
                                            } else {
                                                Some(action)
                                            };
                                        cx.notify();
                                    },
                                )),
                            ),
                    )
                    .when(noted, |r| {
                        r.child(
                            div()
                                .mt(px(2.))
                                .text_right()
                                .text_size(px(fs_sm()))
                                .text_color(t.text_subtle)
                                .child("Fixed: this shortcut cannot be changed"),
                        )
                    })
            });
            div()
                .mb(px(12.))
                .when(searching, |s| {
                    s.child(
                        div()
                            .mt(px(if i == 0 { 4. } else { 0. }))
                            .mb(px(2.))
                            .text_size(px(fs_sm()))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.text_subtle)
                            .child(p),
                    )
                })
                .children(rows)
                .children(fixed_rows)
        })
        .collect();

    let sigil_rows = crate::settings::sigils::rows(d, page, &query, &t, cx);
    let has_sigils = !sigil_rows.is_empty();
    let nothing = sections.is_empty() && !has_sigils;
    // A heading's margin folds into the one above it, as the web page's block margins do.
    let sigils_first = sections.is_empty();
    let none_changed = overrides.is_empty() && !d.sigils(cx).changed();
    let tabs = PAGES.iter().chain([&SIGILS_PAGE]).map(|&p| {
        let selected = p == page;
        div()
            .id(SharedString::from(format!("page-{p}")))
            .role(Role::Tab)
            .aria_label(p)
            .aria_selected(selected)
            .tab_index(0)
            .focus_visible(crate::ui::focus_ring)
            .cursor_pointer()
            .flex()
            .h(px(28.))
            .items_center()
            .rounded(px(radius_sm()))
            .px(px(10.))
            .text_size(px(fs_sm()))
            .map(|tab| {
                if selected {
                    tab.bg(t.bg_selected).text_color(fg)
                } else {
                    tab.text_color(muted)
                        .hover(move |s| s.bg(hover).text_color(fg))
                }
            })
            .child(p)
            .on_click(cx.listener(move |d, _, _, cx| {
                d.shortcuts.recording = None;
                d.shortcuts.recording_sigil = None;
                d.shortcuts.fixed_note = None;
                d.shortcuts.page = p;
                cx.notify();
            }))
    });
    let tabs: Vec<_> = tabs.collect();
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .mb(px(4.))
                .pb(px(4.))
                .child(
                    div()
                        .mb(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h(px(28.))
                                .flex()
                                .items_center()
                                .rounded(px(radius_sm()))
                                .border_1()
                                .border_color(if search_focused {
                                    t.accent
                                } else {
                                    t.border_strong
                                })
                                .bg(t.bg_editor)
                                .px(px(8.))
                                .text_size(px(fs_md()))
                                .child(div().flex_1().min_w_0().child(search)),
                        )
                        .child(
                            btn(
                                "reset-all-shortcuts",
                                "Reset all",
                                BtnKind::Plain,
                                none_changed,
                                &t,
                            )
                            .child("Reset all")
                            .when(!none_changed, |b| {
                                b.on_click(cx.listener(|d, _, _, cx| {
                                    d.shortcuts.recording = None;
                                    d.shortcuts.recording_sigil = None;
                                    d.save_keys(Overrides::new(), cx);
                                    d.save_sigils(crate::ui::sigils::Sigils::default(), cx);
                                }))
                            }),
                        ),
                )
                .when(!searching, |s| {
                    s.child(
                        div()
                            .id("shortcut-pages")
                            .role(Role::TabList)
                            .aria_label("Shortcut pages")
                            .flex()
                            .gap(px(4.))
                            .children(tabs),
                    )
                }),
        )
        .when(has_commands, |s| {
            s.child(
                div()
                    .mb(px(4.))
                    .text_size(px(fs_sm()))
                    .text_color(muted)
                    .child("Click a shortcut, then press the new keys. Esc cancels."),
            )
        })
        .when(has_sigils && !searching, |s| {
            s.child(
                div()
                    .mb(px(4.))
                    .text_size(px(fs_sm()))
                    .text_color(muted)
                    .child("Click a sigil, then type the new one. Esc cancels."),
            )
        })
        .when(nothing, |s| {
            s.child(
                div()
                    .py(px(16.))
                    .text_color(muted)
                    .child("No shortcuts match."),
            )
        })
        .children(sections)
        .when(has_sigils, |s| {
            s.child(
                div()
                    .mb(px(12.))
                    .when(searching, |s| {
                        s.child(
                            div()
                                .mt(px(if sigils_first { 4. } else { 0. }))
                                .mb(px(2.))
                                .text_size(px(fs_sm()))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(t.text_subtle)
                                .child(SIGILS_PAGE),
                        )
                    })
                    .children(sigil_rows),
            )
        })
        .into_any_element()
}
