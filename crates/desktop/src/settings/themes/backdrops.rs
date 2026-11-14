//! Settings → Backgrounds: what is behind the Orchestrator page and behind chats. The
//! Orchestrator has a card for none, the butterfly, each animated background, each of the
//! backgrounds chats come with and each picture,
//! and the card that adds a picture; an agent can be asked to write an animated one.
//!
//! Matching the system, light windows and dark ones each have their own background, and the
//! cards pick for the kind of window that is showing.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{AnyElement, App, Entity, FontWeight, ObjectFit, Window, div, img, prelude::*, px};
use patchwork_server::backgrounds::ChatBackground;
use patchwork_server::settings::Theme as ThemeSetting;
use patchwork_server::themes::{BackdropChoice, Scheme};

use super::cards::{DELETE, Extras, Tool, add_card, card, grid};
use super::files::{Filed, file_tools};
use super::generator_pick::generator_row;
use super::{Kind, generator};
use crate::orchestrator::{butterfly_preview, scene_preview};
use crate::settings::controls::{alert, anchored, section_heading};
use crate::settings::{SettingsDialog, SettingsEvent};
use crate::ui::backdrop::{PRESETS, preset_fill};
use crate::ui::background_picker::{BackgroundPicker, PickerEvent};
use crate::ui::icon;
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
                            |s| *s.backdrop_mut(slot) = BackdropChoice::Image { file },
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
        // The card the chat background grid draws for no background.
        (
            "No background",
            div()
                .flex()
                .items_center()
                .justify_center()
                .bg(t.bg_hover)
                .child(icon("prohibit", 26.)),
            BackdropChoice::None,
        ),
        ("Butterfly", butterfly, BackdropChoice::Butterfly),
    ];
    let presets = PRESETS.iter().map(|&(id, name)| {
        (
            name.to_string(),
            div().relative().bg(t.bg_app).children(preset_fill(id, &t)),
            BackdropChoice::Preset { id: id.to_string() },
            None,
        )
    });
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
            Some(File::Scene(id, (saved.custom, saved.edited))),
        )
    });
    let pictures = d.themes.pictures.iter().map(|picture| {
        let shown = img(picture.path.clone())
            .size_full()
            .object_fit(ObjectFit::Cover);
        (
            picture.name.clone().unwrap_or_else(|| "Picture".into()),
            div().bg(t.bg_app).child(shown),
            BackdropChoice::Image {
                file: picture.file.clone(),
            },
            Some(File::Picture(picture.file.clone())),
        )
    });
    let all: Vec<_> = fixed
        .into_iter()
        .map(|(name, swatch, choice)| (name.to_string(), swatch, choice, None))
        .chain(scenes)
        .chain(presets)
        .chain(pictures)
        .collect();
    all.into_iter()
        .enumerate()
        .map(|(n, (name, swatch, choice, file))| {
            let checked = choice == current;
            let key = format!("backdrop-{n}");
            let tools = match file {
                Some(File::Scene(id, state)) => file_tools(&key, &name, Filed::Scene, &id, state),
                Some(File::Picture(file)) => {
                    let removed = name.clone();
                    vec![Tool::new(
                        format!("delete-{key}"),
                        (DELETE, format!("Delete {name}")),
                        "trash",
                        move |d, cx| d.remove_picture(&file, removed.clone(), cx),
                    )]
                }
                None => Vec::new(),
            };
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

/// The file behind a background's card, which its buttons and its menu change.
enum File {
    /// An animated background's id, and whether it is the person's own and whether it is edited.
    Scene(String, (bool, bool)),
    /// A picture's file name.
    Picture(String),
}

/// The background picker, made on the first look at the pane, showing the saved background.
impl SettingsDialog {
    /// Deletes a picture through the chat background picker, which shows it removed and
    /// offers to take that back.
    fn remove_picture(&mut self, file: &str, name: String, cx: &mut Context<Self>) {
        if let Some(picker) = &self.themes.picker {
            picker.update(cx, |p, cx| p.remove_picture(file, name, cx));
        }
    }

    /// Makes a picture of the chat background a chat drew, and puts it behind every chat.
    pub(super) fn keep_drawn_background(&mut self, id: String, cx: &mut Context<Self>) {
        let server = Arc::clone(&self.server);
        self.run(
            move || server.import_drawn_background(&id),
            |d, drawn, cx| {
                d.themes.writing[Kind::Picture as usize] = None;
                match drawn {
                    Ok(file) => match &d.themes.picker {
                        Some(picker) => picker.update(cx, |p, cx| p.added(file, cx)),
                        None => {
                            d.update(|s| s.chat_background = ChatBackground::Image { file }, cx);
                        }
                    },
                    Err(e) => d.themes.error = Some(e.to_string()),
                }
                cx.notify();
            },
            cx,
        );
    }
}

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
    let scenes_dir = d.server.scenes_dir();
    let new_scene = generator(d, Kind::Scene, "A koi pond at dusk", scenes_dir, window, cx);
    let pictures_dir = d.server.backgrounds_dir();
    let new_picture = generator(
        d,
        Kind::Picture,
        "Soft hills in morning fog",
        pictures_dir,
        window,
        cx,
    );
    let generate_with = generator_row(d, &t, cx);
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
        .child(
            anchored(
                div().id("label-orchestrator-background"),
                "Orchestrator background",
            )
            .pt(px(10.))
            .pb(px(8.))
            .font_weight(FontWeight::SEMIBOLD)
            .child("Orchestrator background"),
        )
        .child(grid(
            "backdrops",
            "Orchestrator background",
            backdrops,
            Some(add_picture),
        ))
        .child(new_scene)
        .child(section_heading("Chats", false, &t))
        .child(
            anchored(div().id("label-chat-background"), "Chat background")
                .py(px(10.))
                .child(
                    div()
                        .pb(px(8.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Chat background"),
                )
                .child(chat_background),
        )
        // The row that draws a picture comes right under the pictures, with no line between.
        .child(new_picture)
        .child(generate_with)
        .into_any_element()
}
