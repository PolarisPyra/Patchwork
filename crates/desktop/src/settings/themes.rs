//! Settings → Themes: the color scheme the app is drawn in and the colors of code, and having an
//! agent write a color scheme. It keeps what Settings → Backgrounds and Settings → Interface
//! pick from as well.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::{AnyElement, Entity, ExternalPaths, Role, SharedString, Window, div, prelude::*, px};
use patchwork_server::backgrounds::BackgroundImage;
use patchwork_server::settings::Theme as ThemeSetting;
use patchwork_server::themes::{CodeTheme, SavedScene, Scheme, Theme, Unreadable};

pub(super) use self::backdrops::backgrounds_pane;
use self::cards::{Slot, add_card, grid, scheme_cards};
use self::editor::{Editing, editor};
use self::interface::Fonts;
pub(super) use self::interface::interface_pane;
use self::look::{accent_row, schedule_rows};
pub(super) use self::look::project_look;
use self::reading::code_theme_cards;
use super::{SettingsDialog, SettingsEvent};
use crate::orchestrator::PreviewFlight;
use crate::settings::controls::{alert, section_heading, switch, token_row};
use crate::ui::background_picker::BackgroundPicker;
use crate::ui::theme::{fs_sm, theme};
use crate::ui::{BtnKind, btn, icon_button, spinner};

mod backdrops;
mod cards;
mod editor;
mod files;
mod interface;
mod look;
mod picture;
mod reading;
mod share;
mod slider;

/// How often a folder is looked at for the file a chat is writing, and how many times before
/// giving up: ten minutes.
const LOOK_EVERY: Duration = Duration::from_secs(2);
const LOOKS: u32 = 300;

/// The Themes pane, as the group its drop target answers to.
const PANE: &str = "themes-pane";

/// What an agent can be asked to write.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Scheme,
    Scene,
    Code,
    Picture,
}

impl Kind {
    /// The field it is described in.
    fn field(self) -> &'static str {
        match self {
            Self::Scheme => "describe-scheme",
            Self::Scene => "describe-scene",
            Self::Code => "describe-code-theme",
            Self::Picture => "describe-chat-background",
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Self::Scheme => "color scheme",
            Self::Scene => "background",
            Self::Code => "code theme",
            Self::Picture => "chat background",
        }
    }
}

/// The pane's own state: what can be picked, and the files chats are writing.
#[derive(Default)]
pub struct ThemesState {
    picker: Option<Entity<BackgroundPicker>>,
    themes: Vec<Theme>,
    scenes: Vec<Arc<SavedScene>>,
    codes: Vec<CodeTheme>,
    /// The files in the folders that cannot be used.
    unreadable: Vec<Unreadable>,
    /// The butterfly on its backdrop's card.
    flight: PreviewFlight,
    pictures: Vec<BackgroundImage>,
    fonts: Fonts,
    /// The color scheme open in the editor.
    editing: Option<Editing>,
    /// The lists were asked for. Cleared to read them again.
    pub(super) loaded: bool,
    /// The id of the color scheme a chat is writing, of the background, of the code theme and
    /// of the chat background.
    writing: [Option<String>; 4],
    error: Option<String>,
}

impl SettingsDialog {
    /// Reads what can be picked, off the UI thread.
    pub(super) fn load_themes(&mut self, cx: &mut Context<Self>) {
        self.themes.loaded = true;
        let server = Arc::clone(&self.server);
        self.run(
            move || {
                let pictures = server.list_background_images().unwrap_or_default();
                let files = (server.list_code_themes(), server.unreadable_themes());
                (server.list_themes(), server.list_scenes(), pictures, files)
            },
            |d, (themes, scenes, pictures, (codes, unreadable)), _| {
                d.themes.themes = themes;
                d.themes.scenes = scenes.into_iter().map(Arc::new).collect();
                d.themes.pictures = pictures;
                d.themes.codes = codes;
                d.themes.unreadable = unreadable;
            },
            cx,
        );
    }

    /// A file in one of the theme folders changed: what can be picked is read again.
    pub fn themes_changed(&mut self, cx: &mut Context<Self>) {
        self.themes.loaded = false;
        cx.notify();
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
        // A code theme is written for the kind of window that is showing, whose cards show.
        let scheme = if theme(cx).is_dark() {
            Scheme::Dark
        } else {
            Scheme::Light
        };
        let server = Arc::clone(&self.server);
        self.run(
            move || match kind {
                Kind::Scheme => server.theme_prompt(&description),
                Kind::Scene => server.scene_prompt(&description),
                Kind::Code => server.code_theme_prompt(&description, scheme),
                Kind::Picture => server.chat_background_prompt(&description),
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
                            Kind::Code => server.code_theme_written(&id),
                            Kind::Picture => server.chat_background_drawn(&id),
                        }
                    })
                    .await;
                if written {
                    break;
                }
            }
            // The agent reads its file back and may correct it once more.
            cx.background_executor().timer(LOOK_EVERY).await;
            if kind == Kind::Picture {
                let _ = this.update(cx, |d, cx| d.keep_drawn_background(id, cx));
                return;
            }
            let listed = Arc::clone(&server);
            let (themes, scenes, codes) = cx
                .background_spawn(async move {
                    let codes = listed.list_code_themes();
                    (listed.list_themes(), listed.list_scenes(), codes)
                })
                .await;
            let _ = this.update(cx, |d, cx| {
                let readable = match kind {
                    Kind::Scheme => themes.iter().any(|t| t.custom && t.id == id),
                    Kind::Scene => scenes.iter().any(|s| s.custom && s.id == id),
                    Kind::Code => codes.iter().any(|c| c.custom && c.id == id),
                    Kind::Picture => false,
                };
                let noun = kind.noun();
                d.themes.writing[kind as usize] = None;
                d.themes.themes = themes;
                d.themes.scenes = scenes.into_iter().map(Arc::new).collect();
                d.themes.codes = codes;
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
        self.themes.writing = Default::default();
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
                .text_size(px(fs_sm()))
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
            .text_size(px(fs_sm()))
            .text_color(t.text_muted)
            .child(name)
            .into_any_element()
    };
    // It lights up while a theme's file is held over the pane, which takes the drop.
    let add_theme = add_card("import-theme", "Add theme", &t)
        .group_drag_over::<ExternalPaths>(PANE, move |style| {
            style.border_color(t.accent).text_color(t.text)
        })
        .on_click(cx.listener(|d, _, _, cx| d.pick_themes(cx)))
        .into_any_element();
    let schemes: Vec<AnyElement> = if follows {
        let light = scheme_cards(d, Slot::Light, cx);
        let night = scheme_cards(d, Slot::Dark, cx);
        vec![
            part("While the system is light"),
            grid("schemes-light", "Color scheme in light", light, None),
            part("While the system is dark"),
            grid(
                "schemes-dark",
                "Color scheme in dark",
                night,
                Some(add_theme),
            ),
        ]
    } else {
        let all = scheme_cards(d, Slot::Fixed, cx);
        vec![grid("color-schemes", "Color scheme", all, Some(add_theme))]
    };
    let schedule = if follows {
        schedule_rows(d, window, cx)
    } else {
        Vec::new()
    };
    let editing = editor(d, window, cx);
    let accent = accent_row(d, cx);
    let themes_dir = d.server.themes_dir();
    let new_scheme = generator(
        d,
        Kind::Scheme,
        "Warm paper, ink blue",
        themes_dir,
        window,
        cx,
    );
    // Code is colored for the kind of window that is showing.
    let code = code_theme_cards(d, if dark { Scheme::Dark } else { Scheme::Light }, cx);
    let code_dir = d.server.code_themes_dir();
    let new_code = generator(
        d,
        Kind::Code,
        "Soft pastels, quiet comments",
        code_dir,
        window,
        cx,
    );
    let unreadable: Vec<_> = d
        .themes
        .unreadable
        .iter()
        .map(|broken| {
            alert(
                format!("unreadable-{}", broken.file),
                format!("{} cannot be used: {}", broken.file, broken.error),
                t.danger,
            )
        })
        .collect();

    div()
        .id(PANE)
        .group(PANE)
        .flex()
        .flex_col()
        // A theme's file dropped anywhere on the pane is imported.
        .on_drop(cx.listener(|d, files: &ExternalPaths, _, cx| {
            d.import_themes(files.paths().to_vec(), cx);
        }))
        .children(
            d.themes
                .error
                .clone()
                .map(|e| alert("themes-error", e, t.danger)),
        )
        .children(unreadable)
        .child(section_heading("Color scheme", true, &t))
        .child(follow)
        .children(schedule)
        .children(schemes)
        .children(editing)
        .child(new_scheme)
        .child(accent)
        .child(code)
        .child(new_code)
        .into_any_element()
}
