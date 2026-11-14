//! Settings → Themes: the color scheme the app is drawn in, the animated background of the
//! Orchestrator page, having an agent write a new one of either, and the background of chats.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::{AnyElement, Entity, FontWeight, Role, SharedString, Window, div, prelude::*, px};
use patchwork_server::backgrounds::{BackgroundImage, BackgroundStrength, ChatBackground};
use patchwork_server::settings::Theme as ThemeSetting;
use patchwork_server::themes::{BackdropChoice, SavedScene, Scheme, Theme};

use self::cards::{Slot, add_picture_card, backdrop_cards, grid, scheme_cards};
use self::reading::{code_theme_row, font_row, installed_fonts, motion_row, text_size_row};
use super::{SettingsDialog, SettingsEvent};
use crate::orchestrator::PreviewFlight;
use crate::settings::controls::{alert, section_heading, segmented, switch, token_row};
use crate::ui::background_picker::{BackgroundPicker, PickerEvent};
use crate::ui::theme::{FS_SM, theme};
use crate::ui::{BtnKind, btn, icon_button, spinner};

mod cards;
mod reading;

const STRENGTHS: [(BackgroundStrength, &str); 3] = [
    (BackgroundStrength::Subtle, "Subtle"),
    (BackgroundStrength::Medium, "Medium"),
    (BackgroundStrength::Strong, "Strong"),
];

/// How often a folder is looked at for the file a chat is writing, and how many times before
/// giving up: ten minutes.
const LOOK_EVERY: Duration = Duration::from_secs(2);
const LOOKS: u32 = 300;

/// What an agent can be asked to write.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Scheme,
    Scene,
}

impl Kind {
    /// The field it is described in.
    fn field(self) -> &'static str {
        match self {
            Self::Scheme => "describe-scheme",
            Self::Scene => "describe-scene",
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Self::Scheme => "color scheme",
            Self::Scene => "background",
        }
    }
}

/// The pane's own state: what can be picked, and the files chats are writing.
#[derive(Default)]
pub struct ThemesState {
    picker: Option<Entity<BackgroundPicker>>,
    themes: Vec<Theme>,
    scenes: Vec<Arc<SavedScene>>,
    /// The butterfly on its backdrop's card.
    flight: PreviewFlight,
    pictures: Vec<BackgroundImage>,
    /// The reading fonts this computer has, for the chat font.
    fonts: Vec<SharedString>,
    /// The lists were asked for. Cleared to read them again.
    pub(super) loaded: bool,
    /// The id of the color scheme a chat is writing, and of the background.
    writing: [Option<String>; 2],
    error: Option<String>,
}

impl SettingsDialog {
    /// Reads what can be picked, off the UI thread.
    fn load_themes(&mut self, cx: &mut Context<Self>) {
        self.themes.loaded = true;
        self.themes.fonts = installed_fonts(cx);
        let server = Arc::clone(&self.server);
        self.run(
            move || {
                let pictures = server.list_background_images().unwrap_or_default();
                (server.list_themes(), server.list_scenes(), pictures)
            },
            |d, (themes, scenes, pictures), _| {
                d.themes.themes = themes;
                d.themes.scenes = scenes.into_iter().map(Arc::new).collect();
                d.themes.pictures = pictures;
            },
            cx,
        );
    }

    /// Deletes a file. A setting that picked it goes back to what it is without one.
    fn remove_themed(&mut self, kind: Kind, id: String, cx: &mut Context<Self>) {
        let server = Arc::clone(&self.server);
        let removing = id.clone();
        self.run(
            move || {
                match kind {
                    Kind::Scheme => server.remove_theme(&removing),
                    Kind::Scene => server.remove_scene(&removing),
                }
                .map_err(|e| e.to_string())
            },
            move |d, removed, cx| {
                d.themes.error = removed.err();
                let scene = BackdropChoice::Scene { id: id.clone() };
                if kind == Kind::Scheme {
                    let gone = Some(id.as_str());
                    let s = &d.settings;
                    let picked = [&s.theme_id, &s.system_light, &s.system_dark];
                    if picked.iter().any(|picked| picked.as_deref() == gone) {
                        d.update(
                            |s| {
                                for picked in
                                    [&mut s.theme_id, &mut s.system_light, &mut s.system_dark]
                                {
                                    if picked.as_deref() == gone {
                                        *picked = None;
                                    }
                                }
                            },
                            cx,
                        );
                    }
                } else if kind == Kind::Scene && d.settings.orchestrator_backdrop == scene {
                    d.update(|s| s.orchestrator_backdrop = BackdropChoice::default(), cx);
                }
                d.load_themes(cx);
            },
            cx,
        );
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
                        d.update(
                            |s| s.orchestrator_backdrop = BackdropChoice::Image { file },
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

    /// Starts a chat whose agent writes what the field of `kind` describes.
    fn generate_themed(&mut self, kind: Kind, cx: &mut Context<Self>) {
        let Some(area) = self.field_area(kind.field()) else {
            return;
        };
        let description = area.read(cx).content.trim().to_string();
        if description.is_empty() || self.themes.writing[kind as usize].is_some() {
            return;
        }
        self.themes.error = None;
        area.update(cx, |area, cx| area.set_text("", cx));
        let server = Arc::clone(&self.server);
        self.run(
            move || match kind {
                Kind::Scheme => server.theme_prompt(&description),
                Kind::Scene => server.scene_prompt(&description),
            },
            move |d, (prompt, id), cx| {
                d.themes.writing[kind as usize] = Some(id.clone());
                cx.emit(SettingsEvent::GenerateTheme(prompt));
                d.await_themed(kind, id, cx);
            },
            cx,
        );
    }

    /// Looks for the file a chat is writing until it is there, then lists what can be picked
    /// again. It gives up after ten minutes, or once the dialog is closed.
    fn await_themed(&mut self, kind: Kind, id: String, cx: &mut Context<Self>) {
        let server = Arc::clone(&self.server);
        cx.spawn(async move |this, cx| {
            let mut written = false;
            for _ in 0..LOOKS {
                cx.background_executor().timer(LOOK_EVERY).await;
                let waiting = this.read_with(cx, |d, _| {
                    d.themes.writing[kind as usize].as_ref() == Some(&id)
                });
                if !waiting.unwrap_or(false) {
                    return;
                }
                let (server, id) = (Arc::clone(&server), id.clone());
                written = cx
                    .background_spawn(async move {
                        match kind {
                            Kind::Scheme => server.theme_written(&id),
                            Kind::Scene => server.scene_written(&id),
                        }
                    })
                    .await;
                if written {
                    break;
                }
            }
            // The agent reads its file back and may correct it once more.
            cx.background_executor().timer(LOOK_EVERY).await;
            let listed = Arc::clone(&server);
            let (themes, scenes) = cx
                .background_spawn(async move { (listed.list_themes(), listed.list_scenes()) })
                .await;
            let _ = this.update(cx, |d, cx| {
                let readable = match kind {
                    Kind::Scheme => themes.iter().any(|t| t.custom && t.id == id),
                    Kind::Scene => scenes.iter().any(|s| s.custom && s.id == id),
                };
                let noun = kind.noun();
                d.themes.writing[kind as usize] = None;
                d.themes.themes = themes;
                d.themes.scenes = scenes.into_iter().map(Arc::new).collect();
                d.themes.error = match (written, readable) {
                    (_, true) => None,
                    (true, false) => Some(format!("The {noun} the chat wrote cannot be read.")),
                    (false, false) => Some(format!("The chat has not written a {noun}.")),
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// The chat that was to write a file could not be started.
    pub fn theme_failed(&mut self, error: String, cx: &mut Context<Self>) {
        self.themes.writing = [None, None];
        self.themes.error = Some(error);
        cx.notify();
    }

    /// Shows a folder in the file manager, making it first.
    fn open_themed_folder(folder: PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |_, cx| {
            let made = folder.clone();
            cx.background_spawn(async move {
                if let Err(e) = std::fs::create_dir_all(&made) {
                    log::warn!(error:% = e; "could not make {}: {{error}}", made.display());
                }
            })
            .await;
            cx.update(|cx| cx.open_with_system(&folder));
        })
        .detach();
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

/// The row an agent is asked from: a description, Generate, and the folder its file goes in.
fn generator(
    d: &mut SettingsDialog,
    kind: Kind,
    placeholder: &'static str,
    folder: PathBuf,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let noun = kind.noun();
    let describe = d.text_field(
        kind.field(),
        format!("Describe a {noun}"),
        "",
        placeholder,
        None,
        |_, _, _| {},
        window,
        cx,
    );
    d.on_submit(kind.field(), move |d, _, cx| d.generate_themed(kind, cx));
    let writing = d.themes.writing[kind as usize].clone();
    let busy = writing.is_some();
    let row = div()
        .pb(px(10.))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(div().flex_1().min_w_0().child(describe))
        .child(
            btn(
                SharedString::from(format!("generate-{}", kind.field())),
                format!("Generate a {noun}"),
                BtnKind::Primary,
                busy,
                &t,
            )
            .child("Generate")
            .when(!busy, |b| {
                b.on_click(cx.listener(move |d, _, _, cx| d.generate_themed(kind, cx)))
            }),
        )
        .child(
            icon_button(
                SharedString::from(format!("folder-{}", kind.field())),
                format!("Open the {noun} folder"),
                "folder-open",
                16.,
                cx,
            )
            .on_click(cx.listener(move |_, _, _, cx| {
                SettingsDialog::open_themed_folder(folder.clone(), cx);
            })),
        );
    div()
        .flex()
        .flex_col()
        .child(row)
        .children(writing.map(|id| {
            div()
                .id(SharedString::from(format!("writing-{}", kind.field())))
                .role(Role::Status)
                .aria_label(format!("A chat is writing {id}"))
                .pb(px(8.))
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(FS_SM))
                .text_color(t.text_muted)
                .child(spinner(14., t.text_muted))
                .child(format!("A chat is writing {id}"))
        }))
        .into_any_element()
}

pub fn render(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    if !d.themes.loaded {
        d.load_themes(cx);
    }
    let chat_background = picker(d, cx);
    let follows = d.settings.theme == ThemeSetting::System && d.settings.theme_id.is_none();
    // Leaving the system's lead keeps the kind of window that is showing.
    let dark = t.is_dark();
    let follow = token_row("Match the system", None, &t).child(
        switch("match-system", "Match the system", follows, &t).on_click(cx.listener(
            move |d, _, _, cx| {
                d.update(
                    |s| {
                        s.theme_id = None;
                        s.theme = match (follows, dark) {
                            (false, _) => ThemeSetting::System,
                            (true, true) => ThemeSetting::Dark,
                            (true, false) => ThemeSetting::Light,
                        };
                    },
                    cx,
                );
            },
        )),
    );
    let part = |name: &'static str| {
        div()
            .pt(px(10.))
            .text_size(px(FS_SM))
            .text_color(t.text_muted)
            .child(name)
            .into_any_element()
    };
    let schemes: Vec<AnyElement> = if follows {
        let light = scheme_cards(d, Slot::Light, cx);
        let night = scheme_cards(d, Slot::Dark, cx);
        vec![
            part("While the system is light"),
            grid("schemes-light", "Color scheme in light", light, None),
            part("While the system is dark"),
            grid("schemes-dark", "Color scheme in dark", night, None),
        ]
    } else {
        let all = scheme_cards(d, Slot::Fixed, cx);
        vec![grid("color-schemes", "Color scheme", all, None)]
    };
    let backdrops = backdrop_cards(d, cx);
    let add_picture = add_picture_card(cx);
    let (themes_dir, scenes_dir) = (d.server.themes_dir(), d.server.scenes_dir());
    let new_scheme = generator(
        d,
        Kind::Scheme,
        "Warm paper, ink blue",
        themes_dir,
        window,
        cx,
    );
    let new_scene = generator(d, Kind::Scene, "A koi pond at dusk", scenes_dir, window, cx);
    let code = [
        code_theme_row(d, Scheme::Light, cx),
        code_theme_row(d, Scheme::Dark, cx),
    ];
    let (font, text_size, motion) = (font_row(d, cx), text_size_row(d, cx), motion_row(d, cx));

    div()
        .flex()
        .flex_col()
        .children(
            d.themes
                .error
                .clone()
                .map(|e| alert("themes-error", e, t.danger)),
        )
        .child(section_heading("Color scheme", true, &t))
        .child(follow)
        .children(schemes)
        .child(new_scheme)
        .child(section_heading("Code", false, &t))
        .children(code)
        .child(section_heading("Orchestrator background", false, &t))
        .child(grid(
            "backdrops",
            "Orchestrator background",
            backdrops,
            Some(add_picture),
        ))
        .child(new_scene)
        .child(motion)
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
        .child(token_row("Background strength", None, &t).child(segmented(
            "strength",
            "Background strength",
            d.settings.chat_background_strength,
            &STRENGTHS,
            &t,
            |d, strength, cx| d.update(|s| s.chat_background_strength = strength, cx),
            cx,
        )))
        .child(font)
        .child(text_size)
        .into_any_element()
}
