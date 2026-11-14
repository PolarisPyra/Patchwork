//! Settings → Backgrounds: what is behind the Orchestrator page and behind chats. The
//! Orchestrator has a card for none, the butterfly, each animated background and each picture,
//! and the card that adds a picture; an agent can be asked to write an animated one.
//!
//! Matching the system, light windows and dark ones each have their own background, and the
//! cards pick for the kind of window that is showing.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{AnyElement, App, Entity, FontWeight, ObjectFit, Window, div, img, prelude::*, px};
use patchwork_server::backgrounds::ChatBackground;
use patchwork_server::settings::Theme as ThemeSetting;
use patchwork_server::themes::{BackdropChoice, Dim, Fit, Scheme};

use super::cards::{Extras, add_card, card, grid};
use super::files::{Filed, file_tools};
use super::picture::picture_rows;
use super::reading::strength_cards;
use super::{Kind, generator};
use crate::orchestrator::{butterfly_preview, scene_preview};
use crate::settings::controls::{alert, section_heading};
use crate::settings::{SettingsDialog, SettingsEvent};
use crate::ui::background_picker::{BackgroundPicker, PickerEvent};
use crate::ui::theme::{fs_sm, theme};

impl SettingsDialog {
    /// The kind of window the background cards pick for: the one showing while the app matches
    /// the system, and none while every window shares one background.
    pub(super) fn backdrop_slot(&self, cx: &App) -> Option<Scheme> {
        let follows =
            self.settings.theme == ThemeSetting::System && self.settings.theme_id.is_none();
        follows.then(|| {
            if theme(cx).is_dark() {
                Scheme::Dark
            } else {
                Scheme::Light
            }
        })
    }

    /// Asks for a picture, copies it into Patchwork's folder, and puts it behind the
    /// Orchestrator page.
    fn add_backdrop_picture(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Add background".into()),
        });
        let server = Arc::clone(&self.server);
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.first().map(|p| p.to_string_lossy().into_owned()) else {
                return;
            };
            let imported = cx
                .background_spawn(async move {
                    server
                        .import_background_image(&path)
                        .map_err(|e| e.to_string())
                })
                .await;
            let _ = this.update(cx, |d, cx| {
                match imported {
                    Ok(file) => {
                        d.themes.error = None;
                        let slot = d.backdrop_slot(cx);
                        d.update(
                            |s| *s.backdrop_mut(slot) = BackdropChoice::picture(file),
                            cx,
                        );
                        d.load_themes(cx);
                    }
                    Err(e) => d.themes.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// The Orchestrator's backdrops: none, the butterfly, the animated backgrounds, the pictures.
pub(super) fn backdrop_cards(
    d: &SettingsDialog,
    cx: &mut Context<SettingsDialog>,
) -> Vec<AnyElement> {
    let t = *theme(cx);
    let slot = d.backdrop_slot(cx);
    let current = d.settings.backdrop_of(slot).clone();
    let still = d.settings.reduce_motion;
    let butterfly = div().child(butterfly_preview(&d.themes.flight, still));
    let fixed = [
        ("None", div().bg(t.bg_app), BackdropChoice::None),
        ("Butterfly", butterfly, BackdropChoice::Butterfly),
    ];
    let scenes = d.themes.scenes.iter().map(|saved| {
        let id = saved.id.clone();
        (
            saved.file.name.clone(),
            div().child(scene_preview(
                Arc::clone(saved),
                t.bg_app,
                t.text_muted,
                still,
            )),
            BackdropChoice::Scene { id: id.clone() },
            Some((id, (saved.custom, saved.edited))),
        )
    });
    let pictures = d.themes.pictures.iter().map(|picture| {
        // The picked picture keeps how it is shown, and its card shows it that way.
        let choice = if current.picture_file() == Some(picture.file.as_str()) {
            current.clone()
        } else {
            BackdropChoice::picture(picture.file.clone())
        };
        let (dim, fit) = match &choice {
            BackdropChoice::Image { dim, fit, .. } => (*dim, *fit),
            _ => (Dim::default(), Fit::default()),
        };
        let shown = img(picture.path.clone())
            .size_full()
            .object_fit(object_fit(fit));
        (
            picture.name.clone().unwrap_or_else(|| "Picture".into()),
            // Dimmed toward the window's color, as the page shows it.
            div()
                .relative()
                .bg(t.bg_app)
                .child(shown)
                .child(div().absolute().inset_0().bg(t.bg_app.opacity(dim.share()))),
            choice,
            None,
        )
    });
    let all: Vec<_> = fixed
        .into_iter()
        .map(|(name, swatch, choice)| (name.to_string(), swatch, choice, None))
        .chain(scenes)
        .chain(pictures)
        .collect();
    all.into_iter()
        .enumerate()
        .map(|(n, (name, swatch, choice, file))| {
            let checked = choice == current;
            let key = format!("backdrop-{n}");
            let tools = file.map_or_else(Vec::new, |(id, state)| {
                file_tools(&key, &name, Filed::Scene, &id, state, cx)
            });
            card(
                key,
                name,
                swatch,
                checked,
                Extras { notes: &[], tools },
                move |d, cx| {
                    let choice = choice.clone();
                    let slot = d.backdrop_slot(cx);
                    d.update(|s| *s.backdrop_mut(slot) = choice, cx);
                },
                cx,
            )
        })
        .collect()
}

/// How a picture is laid on a card for each way it can fill the page. A card is too small to
/// tile, so a tiled picture fills it.
fn object_fit(fit: Fit) -> ObjectFit {
    match fit {
        Fit::Cover | Fit::Tile => ObjectFit::Cover,
        Fit::Contain => ObjectFit::Contain,
        Fit::Stretch => ObjectFit::Fill,
    }
}

/// The background picker, made on the first look at the pane, showing the saved background.
fn picker(d: &mut SettingsDialog, cx: &mut Context<SettingsDialog>) -> Entity<BackgroundPicker> {
    let value = d.settings.chat_background.clone();
    if let Some(picker) = &d.themes.picker {
        picker.update(cx, |p, _| p.set_value(Some(value), None));
        return picker.clone();
    }
    let server = Arc::clone(&d.server);
    let picker = cx.new(|cx| {
        BackgroundPicker::new(
            server,
            "Chat background",
            Some(value),
            Some(ChatBackground::None),
            None,
            cx,
        )
    });
    cx.subscribe(&picker, |d, _, e: &PickerEvent, cx| match e {
        PickerEvent::Picked(Some(background)) => {
            let background = background.clone();
            d.update(|s| s.chat_background = background, cx);
        }
        PickerEvent::Picked(None) => {}
        PickerEvent::PicturesChanged => {
            d.settings = d.server.store.settings();
            d.themes.loaded = false;
            cx.emit(SettingsEvent::Changed);
            cx.notify();
        }
    })
    .detach();
    d.themes.picker = Some(picker.clone());
    picker
}

pub(in crate::settings) fn backgrounds_pane(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    if !d.themes.loaded {
        d.load_themes(cx);
    }
    let chat_background = picker(d, cx);
    // Matching the system, light windows and dark ones each have their own background.
    let slot = d.backdrop_slot(cx);
    let windows = slot.map(|scheme| {
        div()
            .pt(px(10.))
            .text_size(px(fs_sm()))
            .text_color(t.text_muted)
            .child(match scheme {
                Scheme::Light => "In light windows",
                Scheme::Dark => "In dark windows",
            })
    });
    let backdrops = backdrop_cards(d, cx);
    let add_picture = add_card("add-backdrop", "Add background", &t)
        .on_click(cx.listener(|d, _, _, cx| d.add_backdrop_picture(cx)))
        .into_any_element();
    let picture = match *d.settings.backdrop_of(slot) {
        BackdropChoice::Image { dim, fit, blur, .. } => picture_rows(dim, fit, blur, cx),
        _ => Vec::new(),
    };
    let scenes_dir = d.server.scenes_dir();
    let new_scene = generator(d, Kind::Scene, "A koi pond at dusk", scenes_dir, window, cx);
    let strength = strength_cards(d, cx);
    div()
        .id("backgrounds-pane")
        .flex()
        .flex_col()
        .children(
            d.themes
                .error
                .clone()
                .map(|e| alert("backgrounds-error", e, t.danger)),
        )
        .child(section_heading("Orchestrator", true, &t))
        .children(windows)
        .child(grid(
            "backdrops",
            "Orchestrator background",
            backdrops,
            Some(add_picture),
        ))
        .children(picture)
        .child(new_scene)
        .child(section_heading("Chats", false, &t))
        .child(
            div()
                .border_b_1()
                .border_color(t.border)
                .py(px(10.))
                .child(
                    div()
                        .pb(px(8.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Chat background"),
                )
                .child(chat_background),
        )
        .child(strength)
        .into_any_element()
}
