//! The parts of the settings that change every color scheme at once: the accent and when the app
//! turns light or dark in Settings → Themes, and how much room rows take in Settings →
//! Interface. A project's own color scheme and accent are picked here too, from Settings →
//! Projects.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, ClickEvent, Role, SharedString, Toggled, div, prelude::*, px, rgb};
use patchwork_server::themes::{Color, Density, Follow, ProjectLook, clock_minutes, clock_text};

use super::cards::{Extras, card, grid};
use super::reading::titled;
use crate::settings::SettingsDialog;
use crate::settings::controls::{choice, segmented, select_box, token_row};
use crate::ui::focus_ring;
use crate::ui::theme::theme;

/// The accents offered in place of a color scheme's own.
const ACCENTS: [(&str, u32); 9] = [
    ("Blue", 0x003b_82f6),
    ("Indigo", 0x0063_66f1),
    ("Violet", 0x008b_5cf6),
    ("Pink", 0x00ec_4899),
    ("Red", 0x00ef_4444),
    ("Orange", 0x00f9_7316),
    ("Amber", 0x00f5_9e0b),
    ("Green", 0x0022_c55e),
    ("Teal", 0x0014_b8a6),
];

const DENSITIES: [(Density, &str); 3] = [
    (Density::Compact, "Compact"),
    (Density::Comfortable, "Comfortable"),
    (Density::Roomy, "Roomy"),
];

/// The built-in color schemes a project can be drawn in, by the id the settings keep.
const BUILT_IN: [(&str, &str); 3] = [("light", "Light"), ("dark", "Dark"), ("oled", "OLED")];

/// What decides whether the app is light or dark while it matches the system.
#[derive(Clone, Copy, PartialEq, Eq)]
enum By {
    System,
    Clock,
    Sun,
}

const BYS: [(By, &str); 3] = [
    (By::System, "System"),
    (By::Clock, "Clock"),
    (By::Sun, "Sun"),
];

/// The accent every color scheme is drawn with: its own, or one of the swatches.
pub(super) fn accent_row(d: &SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let current = d.settings.accent;
    let own = std::iter::once(("From the color scheme", None));
    let named = ACCENTS
        .iter()
        .map(|(name, code)| (*name, Some(Color(*code))));
    let swatches = own.chain(named).map(|(name, accent)| {
        let checked = current == accent;
        div()
            .id(SharedString::from(format!("accent-{name}")))
            .role(Role::RadioButton)
            .aria_label(name)
            .aria_toggled(if checked {
                Toggled::True
            } else {
                Toggled::False
            })
            .tab_index(0)
            .focus_visible(focus_ring)
            .cursor_pointer()
            .flex_none()
            .size(px(22.))
            .rounded_full()
            .border_2()
            .border_color(if checked {
                t.text
            } else {
                gpui::transparent_black()
            })
            .p(px(2.))
            .child(match accent {
                Some(Color(code)) => div().size_full().rounded_full().bg(rgb(code)),
                None => div()
                    .size_full()
                    .rounded_full()
                    .border_1()
                    .border_dashed()
                    .border_color(t.text_muted),
            })
            .on_click(cx.listener(move |d, _, _, cx| d.update(|s| s.accent = accent, cx)))
    });
    let group = div()
        .id("accents")
        .role(Role::RadioGroup)
        .aria_label("Accent")
        .flex()
        .flex_wrap()
        .gap(px(4.))
        .children(swatches);
    token_row("Accent", None, &t)
        .child(group)
        .into_any_element()
}

/// How much room rows take: a card for each, drawn as three rows that far apart.
pub(super) fn density_cards(d: &SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let cards = DENSITIES
        .into_iter()
        .map(|(density, name)| {
            let scale = density.scale();
            let rows = (0..3_u8).map(|n| {
                div().h(px(8. * scale)).rounded(px(2.)).bg(if n == 1 {
                    t.bg_selected
                } else {
                    t.bg_hover
                })
            });
            let swatch = div()
                .bg(t.bg_panel)
                .px(px(10.))
                .flex()
                .flex_col()
                .justify_center()
                .gap(px(4. * scale))
                .children(rows);
            card(
                format!("density-{name}"),
                name,
                swatch,
                d.settings.density == density,
                Extras::default(),
                move |d, cx| d.update(|s| s.density = density, cx),
                cx,
            )
        })
        .collect();
    titled("Density", grid("density", "Density", cards, None), &t)
}

/// When the app turns light or dark while it matches the system: with it, at two times of
/// day, or with the sun at a place.
pub(super) fn schedule_rows(
    d: &mut SettingsDialog,
    window: &mut gpui::Window,
    cx: &mut Context<SettingsDialog>,
) -> Vec<AnyElement> {
    let t = *theme(cx);
    let follow = d.settings.follow;
    let by = match follow {
        Follow::System => By::System,
        Follow::Clock { .. } => By::Clock,
        Follow::Sun { .. } => By::Sun,
    };
    let pick = segmented(
        "follow-by",
        "Switch with",
        by,
        &BYS,
        &t,
        |d, by, cx| {
            d.update(
                |s| {
                    s.follow = match by {
                        By::System => Follow::System,
                        By::Clock => Follow::Clock {
                            light_at: 7 * 60,
                            dark_at: 19 * 60,
                        },
                        By::Sun => Follow::Sun {
                            latitude: 0.,
                            longitude: 0.,
                        },
                    };
                },
                cx,
            );
        },
        cx,
    );
    let mut rows = vec![
        token_row("Switch with", None, &t)
            .child(pick)
            .into_any_element(),
    ];
    match follow {
        Follow::System => {}
        Follow::Clock { light_at, dark_at } => {
            for (key, title, at, light) in [
                ("follow-light-at", "Light from", light_at, true),
                ("follow-dark-at", "Dark from", dark_at, false),
            ] {
                let field = d.text_field(
                    key,
                    title,
                    &clock_text(at),
                    "7:00",
                    Some(80.),
                    move |d, text, cx| {
                        let Some(minutes) = clock_minutes(&text) else {
                            return;
                        };
                        d.update(
                            |s| {
                                if let Follow::Clock { light_at, dark_at } = &mut s.follow {
                                    *(if light { light_at } else { dark_at }) = minutes;
                                }
                            },
                            cx,
                        );
                    },
                    window,
                    cx,
                );
                rows.push(token_row(title, None, &t).child(field).into_any_element());
            }
        }
        Follow::Sun {
            latitude,
            longitude,
        } => {
            for (key, title, value, most, north) in [
                ("follow-latitude", "Latitude", latitude, 90., true),
                ("follow-longitude", "Longitude", longitude, 180., false),
            ] {
                let field = d.number_field(
                    key,
                    title,
                    f64::from(value),
                    0.01,
                    Some(-most),
                    Some(most),
                    move |d, degrees, cx| {
                        #[expect(
                            clippy::cast_possible_truncation,
                            reason = "a place on Earth to a hundredth of a degree"
                        )]
                        let degrees = degrees as f32;
                        d.update(
                            |s| {
                                if let Follow::Sun {
                                    latitude,
                                    longitude,
                                } = &mut s.follow
                                {
                                    *(if north { latitude } else { longitude }) = degrees;
                                }
                            },
                            cx,
                        );
                    },
                    window,
                    cx,
                );
                rows.push(token_row(title, None, &t).child(field).into_any_element());
            }
        }
    }
    rows
}

/// A select of a project's row: what is picked, and a menu of the looks it can change to.
fn look_select(
    d: &SettingsDialog,
    key: String,
    label: String,
    project: &str,
    options: Vec<(SharedString, bool, ProjectLook)>,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let shown = options
        .iter()
        .find(|(_, checked, _)| *checked)
        .map(|(name, _, _)| name.clone())
        .unwrap_or_default();
    let project = project.to_string();
    let (menu, title) = (SharedString::from(key), SharedString::from(label));
    select_box(menu.clone(), title.clone(), shown, d.menu_open(&menu), &t)
        .w(px(130.))
        .on_click(cx.listener(move |d, e: &ClickEvent, window, cx| {
            let weak = cx.entity().downgrade();
            let items = options
                .iter()
                .map(|(name, checked, look)| {
                    let (look, project) = (look.clone(), project.clone());
                    choice(weak.clone(), name.clone(), *checked, move |d, cx| {
                        let look = look.clone();
                        d.menu = None;
                        d.update(|s| s.set_project_look(&project, |was| *was = look), cx);
                    })
                })
                .collect();
            d.toggle_menu(menu.clone(), title.clone(), items, e.position(), window, cx);
        }))
        .into_any_element()
}

/// The color scheme and the accent the project `id` is drawn in while it is the open one.
pub(in crate::settings) fn project_look(
    d: &mut SettingsDialog,
    id: &str,
    name: &str,
    cx: &mut Context<SettingsDialog>,
) -> [AnyElement; 2] {
    if !d.themes.loaded {
        d.load_themes(cx);
    }
    let look = d
        .settings
        .project_looks
        .get(id)
        .cloned()
        .unwrap_or_default();
    let built_in = BUILT_IN
        .iter()
        .map(|(id, name)| ((*id).to_string(), SharedString::new_static(name)));
    let files = d.themes.themes.iter().map(|theme| {
        (
            theme.id.clone(),
            SharedString::from(theme.file.name.clone()),
        )
    });
    let schemes = std::iter::once((None, SharedString::new_static("Default colors")))
        .chain(built_in.chain(files).map(|(id, name)| (Some(id), name)))
        .map(|(theme, name)| {
            let checked = look.theme == theme;
            let accent = look.accent;
            (name, checked, ProjectLook { theme, accent })
        })
        .collect();
    let named = ACCENTS
        .iter()
        .map(|(name, code)| (Some(Color(*code)), SharedString::new_static(name)));
    let mut accents: Vec<_> = std::iter::once((None, SharedString::new_static("Default accent")))
        .chain(named)
        .map(|(accent, name)| {
            let theme = look.theme.clone();
            (name, look.accent == accent, ProjectLook { theme, accent })
        })
        .collect();
    // An accent written into the settings by hand is none of the swatches.
    if let Some(Color(code)) = look.accent
        && !accents.iter().any(|(_, checked, _)| *checked)
    {
        accents.push((format!("#{code:06x}").into(), true, look.clone()));
    }
    [
        look_select(
            d,
            format!("project-scheme-{id}"),
            format!("Color scheme of {name}"),
            id,
            schemes,
            cx,
        ),
        look_select(
            d,
            format!("project-accent-{id}"),
            format!("Accent of {name}"),
            id,
            accents,
            cx,
        ),
    ]
}
