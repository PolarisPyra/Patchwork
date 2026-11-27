//! The inspector beside the directory, shown only while something is picked: a command with its
//! keys, to change, add, take off or reset, and what else shares them; a fixed key; or every
//! command on one key.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, FontWeight, Role, SharedString, div, prelude::*, px};
use patchwork_ui_theme::{IconName, Theme, fs_lg, fs_sm, fs_xs, radius_sm};
use patchwork_ui_components::{BtnKind, btn, tooltip};
use patchwork_ui_theme::icon;

use super::bindings::{Command, FIXED_KEYS, command, default_keys, keys_for};
use super::chord::Chord;
use super::list::{Index, Item, Spot, kbd};
use crate::settings::SettingsDialog;

pub(super) fn render(
    d: &SettingsDialog,
    index: &Index,
    t: &Theme,
    cx: &mut Context<SettingsDialog>,
) -> Option<AnyElement> {
    Some(match d.shortcuts.spot {
        Spot::Command(id) => command_card(d, command(id)?, index, t, cx),
        Spot::Fixed(at) => fixed_key(at, t),
        Spot::Chord(chord) => on_chord(chord, index, t, cx),
        Spot::None => return None,
    })
}

fn title(text: impl Into<SharedString>, t: &Theme) -> gpui::Div {
    div()
        .text_size(px(fs_lg()))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(t.text)
        .child(text.into())
}

fn label(text: &'static str, t: &Theme) -> gpui::Div {
    div()
        .mt(px(12.))
        .mb(px(4.))
        .text_size(px(fs_xs()))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(t.text_subtle)
        .child(text)
}

fn note(text: impl Into<SharedString>, color: gpui::Hsla) -> gpui::Div {
    div()
        .text_size(px(fs_sm()))
        .text_color(color)
        .child(text.into())
}

/// A command: its name, page and what it does, its keys, and what else is on them.
fn command_card(
    d: &SettingsDialog,
    c: &'static Command,
    index: &Index,
    t: &Theme,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let id = c.id;
    let parts = index.parts(Item::Command(c));
    let recording = d.shortcuts.recording == Some(id);
    let changed = index.overrides.contains_key(id);
    let clash = index.conflicts(id);
    let (hover, muted, fg) = (t.bg_hover, t.text_muted, t.text);
    let small = |name: &str, at: usize, label: String, glyph: IconName| {
        patchwork_ui_components::button(SharedString::from(format!("{name}-{at}")), label)
            .size(px(22.))
            .flex_none()
            .justify_center()
            .rounded(px(radius_sm()))
            .text_color(muted)
            .hover(move |s| s.bg(hover).text_color(fg))
            .child(icon(glyph, 12.))
    };
    let keys =
        parts.iter().enumerate().map(|(at, p)| {
            let replacing = recording && d.shortcuts.slot == Some(at);
            div()
                .id(("binding", at))
                .flex()
                .items_center()
                .gap(px(4.))
                .p(px(4.))
                .rounded(px(radius_sm()))
                .border_1()
                .border_color(if replacing {
                    t.accent
                } else {
                    gpui::transparent_black()
                })
                .when(replacing, |r| r.bg(t.bg_accent_soft))
                .child(
                    div()
                        .id(("binding-keys", at))
                        .flex_1()
                        .cursor_pointer()
                        .tooltip(tooltip("See what else runs on this key"))
                        .on_click(cx.listener(move |d, _, _, cx| {
                            if let Some(Some(chord)) = d.shortcuts.overrides.as_ref().map(|o| {
                                keys_for(o, id).get(at).and_then(|k| Chord::from_stored(k))
                            }) {
                                d.show_spot(Spot::Chord(chord), cx);
                            }
                        }))
                        .map(|r| {
                            if replacing {
                                r.child(note("Press the new keys…", t.accent_text))
                            } else {
                                r.child(kbd(p, t))
                            }
                        }),
                )
                .when(!c.fixed, |r| {
                    r.child(
                        small(
                            "change-key",
                            at,
                            format!("Change this key for {}", c.title),
                            IconName::PencilSimple,
                        )
                        .tooltip(tooltip("Change"))
                        .on_click(cx.listener(move |d, _, window, cx| {
                            d.record(id, Some(at), window, cx);
                        })),
                    )
                    .child(
                        small(
                            "remove-key",
                            at,
                            format!("Remove this key from {}", c.title),
                            IconName::X,
                        )
                        .tooltip(tooltip("Remove"))
                        .on_click(cx.listener(move |d, _, _, cx| {
                            let mut keys = keys_for(
                                d.shortcuts
                                    .overrides
                                    .as_ref()
                                    .unwrap_or(&std::collections::HashMap::default()),
                                id,
                            );
                            if at < keys.len() {
                                keys.remove(at);
                            }
                            d.shortcuts.shown_binding = 0;
                            d.set_keys(id, &keys, cx);
                        })),
                    )
                })
        });
    let adding = recording && d.shortcuts.slot.is_none();
    let clashes = clash.iter().enumerate().map(|(at, (key, other))| {
        let other_id = other.id;
        let key = key.clone();
        let shared = Chord::from_stored(&key).map_or_else(|| key.clone(), |c| c.parts().join("+"));
        div()
            .flex()
            .items_start()
            .gap(px(6.))
            .py(px(4.))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .h(px(18.))
                    .text_color(t.warning)
                    .child(icon(IconName::Warning, 12.)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(fs_sm()))
                    .line_height(px(18.))
                    .text_color(t.text_muted)
                    .child(format!(
                        "{shared} also runs {} ({})",
                        other.title, other.page
                    )),
            )
            .when(!other.fixed, |r| {
                r.child(
                    btn(
                        ("unbind-there", at),
                        "Unbind there",
                        BtnKind::Plain,
                        false,
                        t,
                    )
                    .flex_none()
                    .child("Unbind there")
                    .on_click(cx.listener(move |d, _, _, cx| {
                        let overrides = d.shortcuts.overrides.clone().unwrap_or_default();
                        let keys: Vec<String> = keys_for(&overrides, other_id)
                            .into_iter()
                            .filter(|k| *k != key)
                            .collect();
                        d.set_keys(other_id, &keys, cx);
                    })),
                )
            })
    });
    let clashes: Vec<_> = clashes.collect();
    div()
        .flex()
        .flex_col()
        .child(title(c.title, t))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(fs_xs()))
                .text_color(t.text_subtle)
                .child(c.page)
                .when(c.fixed, |r| {
                    r.child("·")
                        .child(icon(IconName::LockSimple, 10.))
                        .child("Built in")
                })
                .when(changed, |r| r.child("·").child("Changed")),
        )
        .child(label("KEYS", t))
        .children(keys)
        .when(parts.is_empty() && !adding, |r| {
            r.child(note("No key runs this yet.", t.text_subtle))
        })
        .when(adding, |r| {
            r.child(
                div()
                    .p(px(6.))
                    .rounded(px(radius_sm()))
                    .border_1()
                    .border_color(t.accent)
                    .bg(t.bg_accent_soft)
                    .child(note("Press the new keys… Esc cancels.", t.accent_text)),
            )
        })
        .when(recording, |r| {
            r.children(d.shortcuts.refused.clone().map(|e| {
                div()
                    .id("shortcut-refused")
                    .role(Role::Alert)
                    .mt(px(4.))
                    .child(note(e, t.danger))
            }))
        })
        .when(c.fixed, |r| {
            r.child(div().mt(px(8.)).child(note(
                "This key is built in and can't be changed.",
                t.text_subtle,
            )))
        })
        .when(!c.fixed, |r| {
            r.child(
                div()
                    .mt(px(8.))
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .child(
                        btn("add-key", "Add a key", BtnKind::Plain, recording, t)
                            .child(icon(IconName::Plus, 12.))
                            .child("Add a key")
                            .when(!recording, |b| {
                                b.on_click(cx.listener(move |d, _, window, cx| {
                                    d.record(id, None, window, cx);
                                }))
                            }),
                    )
                    .when(recording, |r| {
                        r.child(
                            btn("cancel-key", "Cancel", BtnKind::Plain, false, t)
                                .child("Cancel")
                                .on_click(cx.listener(move |d, _, _, cx| {
                                    d.show_spot(Spot::Command(id), cx);
                                })),
                        )
                    })
                    .when(changed && !recording, |r| {
                        r.child(
                            btn("reset-key", "Reset to default", BtnKind::Plain, false, t)
                                .child(icon(IconName::ArrowCounterClockwise, 12.))
                                .child("Reset")
                                .tooltip(tooltip(default_label(id)))
                                .on_click(cx.listener(move |d, _, _, cx| {
                                    d.shortcuts.shown_binding = 0;
                                    d.set_keys(id, &default_keys(id), cx);
                                })),
                        )
                    }),
            )
        })
        .when(!clashes.is_empty(), |r| {
            r.child(label("SHARED KEYS", t)).children(clashes)
        })
        .into_any_element()
}

/// What Reset gives back, for its tooltip.
fn default_label(id: &str) -> String {
    let keys: Vec<String> = default_keys(id)
        .iter()
        .map(|k| Chord::from_stored(k).map_or_else(|| k.clone(), |c| c.parts().join("+")))
        .collect();
    if keys.is_empty() {
        "Back to no key".to_owned()
    } else {
        format!("Back to {}", keys.join(", "))
    }
}

/// A key the editor or the mouse owns.
fn fixed_key(at: usize, t: &Theme) -> AnyElement {
    let (page, keys, action) = FIXED_KEYS[at];
    let parts: Vec<String> = keys.iter().map(|k| (*k).to_owned()).collect();
    div()
        .flex()
        .flex_col()
        .child(title(action, t))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(fs_xs()))
                .text_color(t.text_subtle)
                .child(page)
                .child("·")
                .child(icon(IconName::LockSimple, 10.))
                .child("Built in"),
        )
        .child(label("KEYS", t))
        .child(kbd(&parts, t))
        .child(div().mt(px(10.)).child(note(
            "The editor or the mouse owns this key, so it can't be changed.",
            t.text_subtle,
        )))
        .into_any_element()
}

/// Everything on one key.
fn on_chord(
    chord: Chord,
    index: &Index,
    t: &Theme,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let name = chord.parts().join(" + ");
    let mut rows: Vec<AnyElement> = Vec::new();
    let hover = t.bg_hover;
    let pick = |spot: Spot, at: usize, text: String| {
        div()
            .id(("on-chord", at))
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(6.))
            .py(px(4.))
            .rounded(px(radius_sm()))
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .on_click(cx.listener(move |d, _, _, cx| d.show_spot(spot, cx)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(fs_sm()))
                    .text_color(t.text)
                    .child(text),
            )
            .into_any_element()
    };
    for c in index.on(chord) {
        rows.push(pick(
            Spot::Command(c.id),
            rows.len(),
            format!("{} · {}", c.title, c.page),
        ));
    }
    for (at, _) in FIXED_KEYS.iter().enumerate() {
        if index.chords(Item::Fixed(at)).contains(&chord) {
            let (page, _, action) = FIXED_KEYS[at];
            rows.push(pick(
                Spot::Fixed(at),
                rows.len(),
                format!("{action} · {page}"),
            ));
        }
    }
    let empty = rows.is_empty();
    div()
        .flex()
        .flex_col()
        .child(kbd(&chord.parts(), t))
        .child(label("RUNS", t))
        .children(rows)
        .when(empty, |r| {
            r.child(note(
                format!("Nothing uses {name}. Pick a command and Add a key to give it this one."),
                t.text_subtle,
            ))
        })
        .into_any_element()
}
