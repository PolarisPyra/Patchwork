//! Settings → Appearance: its state, asking an agent for a theme, and drawing the page: light or
//! dark and the accent, then the themes, one kind at a time (the palettes, and the themes of
//! code, of the Orchestrator, of chats and of Quilts), and the interface.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::{AnyElement, Entity, ExternalPaths, Role, SharedString, Window, div, prelude::*, px};
use patchwork_backend::backgrounds::BackgroundImage;
use patchwork_backend::logging::THEME_LOG;
use patchwork_backend::settings::{LogLevel, Theme as ThemeSetting};
use patchwork_themes::{CodeTheme, SavedScene, Scheme, Theme, Unreadable};

use super::cards::{Slot, add_card, grid, scheme_cards};
use super::editor::{Editing, editor};
use super::generator_pick::generator_row;
use super::interface::Fonts;
use super::look::{accent_row, schedule_rows};
use super::reading::code_theme_cards;
use crate::settings::{SettingsDialog, SettingsEvent};
use patchwork_ui_backdrop::PreviewFlight;
use crate::settings::controls::{FieldKind, Section, alert, anchored, segmented, sought, token_row};
use crate::background_picker::BackgroundPicker;
use patchwork_ui_theme::{IconName, fs_sm, theme};
use patchwork_ui_components::{BtnKind, btn, icon_button, spinner};

/// How often a folder is looked at for the file a chat is writing, and how many times before
/// giving up: ten minutes.
const LOOK_EVERY: Duration = Duration::from_secs(2);
const LOOKS: u32 = 300;

/// The Themes pane, as the group its drop target answers to.
const PANE: &str = "themes-pane";

/// What an agent can be asked to write.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Scheme,
    Scene,
    Code,
    Picture,
}

impl Kind {
    /// The field it is described in.
    pub(super) fn field(self) -> &'static str {
        match self {
            Self::Scheme => "describe-scheme",
            Self::Scene => "describe-scene",
            Self::Code => "describe-code-theme",
            Self::Picture => "describe-chat-background",
        }
    }

    pub(super) fn noun(self) -> &'static str {
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
    pub(super) picker: Option<Entity<BackgroundPicker>>,
    /// The Quilts theme's picker, made on the first look at its pane.
    pub(super) quilts_picker: Option<Entity<BackgroundPicker>>,
    pub(super) themes: Vec<Theme>,
    pub(super) scenes: Vec<Arc<SavedScene>>,
    pub(super) codes: Vec<CodeTheme>,
    /// The files in the folders that cannot be used.
    pub(super) unreadable: Vec<Unreadable>,
    /// The butterfly on its backdrop's card.
    pub(super) flight: PreviewFlight,
    pub(super) pictures: Vec<BackgroundImage>,
    pub(super) fonts: Fonts,
    /// The color scheme open in the editor.
    pub(super) editing: Option<Editing>,
    /// The lists were asked for. Cleared to read them again.
    pub(crate) loaded: bool,
    /// The id of the color scheme a chat is writing, of the background, of the code theme and
    /// of the chat background.
    pub(super) writing: [Option<String>; 4],
    /// What is typed in each kind's description, kept so a redraw does not clear it.
    pub(super) drafts: [String; 4],
    pub(super) error: Option<String>,
    /// The kind of theme showing.
    pub(super) shelf: Shelf,
}

impl SettingsDialog {
    /// Reads what can be picked, off the UI thread.
    pub(super) fn load_themes(&mut self, cx: &mut Context<Self>) {
        self.themes.loaded = true;
        let core = Arc::clone(&self.core);
        self.run(
            move || {
                let pictures = core.list_background_images().unwrap_or_default();
                let files = (core.list_code_themes(), core.unreadable_themes());
                (core.list_themes(), core.list_scenes(), pictures, files)
            },
            |d, (themes, scenes, pictures, (codes, unreadable)), cx| {
                // A background added or written from another pane shows in every picker.
                for picker in [&d.themes.picker, &d.themes.quilts_picker]
                    .into_iter()
                    .flatten()
                {
                    picker.update(cx, BackgroundPicker::load_images);
                }
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
    pub(super) fn generate_themed(&mut self, kind: Kind, cx: &mut Context<Self>) {
        let description = self.themes.drafts[kind as usize].trim().to_string();
        if description.is_empty() || self.themes.writing[kind as usize].is_some() {
            return;
        }
        self.themes.error = None;
        self.themes.drafts[kind as usize].clear();
        if let Some(area) = self.field_area(kind.field()) {
            area.update(cx, |area, cx| area.set_text("", cx));
        }
        // A code theme and a chat background are made for the kind of window that is showing.
        let scheme = if theme(cx).is_dark() {
            Scheme::Dark
        } else {
            Scheme::Light
        };
        let core = Arc::clone(&self.core);
        self.run(
            move || match kind {
                Kind::Scheme => core.theme_prompt(&description),
                Kind::Scene => core.scene_prompt(&description),
                Kind::Code => core.code_theme_prompt(&description, scheme),
                Kind::Picture => core.chat_background_prompt(&description, scheme),
            },
            move |d, (prompt, id), cx| {
                d.themes.writing[kind as usize] = Some(id.clone());
                let pick = d.settings.generator_pick().cloned();
                cx.emit(SettingsEvent::GenerateTheme(prompt, pick));
                d.await_themed(kind, id, cx);
            },
            cx,
        );
    }

    /// Looks for the file a chat is writing until it is there, then lists what can be picked
    /// again. It gives up after ten minutes, or once the dialog is closed.
    pub(super) fn await_themed(&mut self, kind: Kind, id: String, cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
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
                let (core, id) = (Arc::clone(&core), id.clone());
                written = cx
                    .background_spawn(async move {
                        match kind {
                            Kind::Scheme => core.theme_written(&id),
                            Kind::Scene => core.scene_written(&id),
                            Kind::Code => core.code_theme_written(&id),
                            Kind::Picture => core.chat_background_drawn(&id),
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
            let listed = Arc::clone(&core);
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
                if let Some(error) = &d.themes.error {
                    THEME_LOG.text(LogLevel::Error, format!("{noun} {id}: {error}"));
                }
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
    pub(super) fn open_themed_folder(folder: PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |_, cx| {
            let made = folder.clone();
            cx.background_spawn(async move {
                if let Err(e) = patchwork_platform::files::paths::make_folder(&made) {
                    log::warn!(error:% = e; "{{error}}");
                }
            })
            .await;
            cx.update(|cx| cx.open_with_system(&folder));
        })
        .detach();
    }
}

/// The row an agent is asked from: a description, Generate, and the folder its file goes in.
pub(super) fn generator(
    d: &mut SettingsDialog,
    kind: Kind,
    placeholder: &'static str,
    folder: PathBuf,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let noun = kind.noun();
    // A live field keeps each change in the draft, and Enter generates.
    let draft = d.themes.drafts[kind as usize].clone();
    let live = FieldKind {
        live: true,
        ..FieldKind::default()
    };
    let describe = d
        .field(
            kind.field(),
            format!("Describe a {noun}"),
            &draft,
            placeholder,
            live,
            move |d, text, _| d.themes.drafts[kind as usize] = text,
            window,
            cx,
        )
        .w_full();
    d.on_submit(kind.field(), move |d, text, cx| {
        d.themes.drafts[kind as usize] = text;
        d.generate_themed(kind, cx);
    });
    let writing = d.themes.writing[kind as usize].clone();
    let busy = writing.is_some();
    let describe = div().flex_1().min_w_0().child(describe);
    let generate = btn(
        SharedString::from(format!("generate-{}", kind.field())),
        format!("Generate a {noun}"),
        BtnKind::Primary,
        busy,
        &t,
    )
    .child("Generate")
    .when(!busy, |b| {
        b.on_click(cx.listener(move |d, _, _, cx| d.generate_themed(kind, cx)))
    });
    let open_folder = icon_button(
        SharedString::from(format!("folder-{}", kind.field())),
        format!("Open the {noun} folder"),
        IconName::FolderOpen,
        16.,
        cx,
    )
    .on_click(cx.listener(move |_, _, _, cx| {
        SettingsDialog::open_themed_folder(folder.clone(), cx);
    }));
    let row = div()
        .pb(px(10.))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(describe)
        .child(generate)
        .child(open_folder);
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

/// Light, dark, or as the system is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Light,
    Dark,
    System,
}

const MODES: [(Mode, &str); 3] = [
    (Mode::Light, "Light"),
    (Mode::Dark, "Dark"),
    (Mode::System, "System"),
];

/// One kind of theme, of which the page shows one at a time.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Shelf {
    #[default]
    Palette,
    Syntax,
    Orchestrator,
    Chat,
    Quilts,
}

const SHELVES: [(Shelf, &str); 5] = [
    (Shelf::Palette, "Palette"),
    (Shelf::Syntax, "Syntax"),
    (Shelf::Orchestrator, "Orchestrator"),
    (Shelf::Chat, "Chat"),
    (Shelf::Quilts, "Quilts"),
];

impl Shelf {
    /// The kind a search result names, by the name it had as a section of its own.
    fn named(name: &str) -> Option<Self> {
        let name = name.to_ascii_lowercase();
        match name.as_str() {
            "palette" | "light palette" | "dark palette" => Some(Self::Palette),
            "syntax theme" => Some(Self::Syntax),
            "orchestrator theme" => Some(Self::Orchestrator),
            "chat theme" => Some(Self::Chat),
            "quilts theme" => Some(Self::Quilts),
            _ => None,
        }
    }

    /// The name a search finds it by.
    fn found_as(self) -> &'static str {
        match self {
            Self::Palette => "Palette",
            Self::Syntax => "Syntax theme",
            Self::Orchestrator => "Orchestrator theme",
            Self::Chat => "Chat theme",
            Self::Quilts => "Quilts theme",
        }
    }
}

/// The whole Appearance page.
pub fn appearance(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let look = look(d, window, cx);
    let themes = themes(d, window, cx);
    let interface = super::interface_pane(d, window, cx);
    let t = *theme(cx);
    let generate_with = generator_row(d, &t, cx);
    look.child(themes)
        .child(interface)
        .child(Section::new("Theme generation").child(generate_with))
        .into_any_element()
}

/// The mode and accent card.
fn look(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> gpui::Stateful<gpui::Div> {
    let t = *theme(cx);
    if !d.themes.loaded {
        d.load_themes(cx);
    }
    let follows = d.settings.theme == ThemeSetting::System && d.settings.theme_id.is_none();
    let mode = match (follows, t.is_dark()) {
        (true, _) => Mode::System,
        (false, true) => Mode::Dark,
        (false, false) => Mode::Light,
    };
    let pick_mode = segmented(
        "appearance-mode",
        "Mode",
        mode,
        &MODES,
        &t,
        move |d, to, cx| {
            if to == mode {
                return;
            }
            d.update(
                |s| {
                    s.theme_id = None;
                    s.theme = match to {
                        Mode::Light => ThemeSetting::Light,
                        Mode::Dark => ThemeSetting::Dark,
                        Mode::System => ThemeSetting::System,
                    };
                },
                cx,
            );
        },
        cx,
    );
    let mode_row = token_row("Mode", &t).child(pick_mode);
    let schedule = if follows {
        schedule_rows(d, window, cx)
    } else {
        Vec::new()
    };
    let accent = accent_row(d, cx);
    pane(d, cx).child(
        Section::untitled()
            .child(mode_row)
            .children(schedule)
            .child(accent),
    )
}

/// Every kind of theme under one heading, the one picked above showing below it.
fn themes(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> Section {
    let t = *theme(cx);
    // A search for one kind opens it.
    let sought = sought().filter(|name| Shelf::named(name).is_some());
    if let Some(shelf) = sought.and_then(Shelf::named) {
        d.themes.shelf = shelf;
    }
    let shelf = d.themes.shelf;
    let pick = segmented(
        "theme-kind",
        "Themes",
        shelf,
        &SHELVES,
        &t,
        |d, to, cx| {
            d.themes.shelf = to;
            cx.notify();
        },
        cx,
    );
    let shown = match shelf {
        Shelf::Palette => palettes(d, window, cx),
        Shelf::Syntax => code_theme_pane(d, window, cx),
        Shelf::Orchestrator => super::orchestrator_background_pane(d, window, cx),
        Shelf::Chat => super::chat_background_pane(d, window, cx),
        Shelf::Quilts => super::quilts_background_pane(d, cx),
    };
    let found_as = sought.unwrap_or(shelf.found_as());
    Section::new("Themes")
        .action(pick)
        .bare()
        .child(anchored(div().id("theme-shelf"), found_as).child(shown))
}

/// The palettes: one grid for each kind of window while the app matches the system, else one
/// grid of them all, then the editor and the row that writes one.
fn palettes(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let follows = d.settings.theme == ThemeSetting::System && d.settings.theme_id.is_none();
    // It lights up while a theme's file is held over the pane, which takes the drop.
    let add_theme = add_card("import-theme", "Add theme", &t)
        .group_drag_over::<ExternalPaths>(PANE, move |style| {
            style.border_color(t.accent).text_color(t.text)
        })
        .on_click(cx.listener(|d, _, _, cx| d.pick_themes(cx)))
        .into_any_element();
    let windows = |text: &'static str| {
        div()
            .pt(px(12.))
            .text_size(px(fs_sm()))
            .text_color(t.text_muted)
            .child(text)
    };
    let grids: Vec<AnyElement> = if follows {
        let light = scheme_cards(d, Slot::Light, cx);
        let night = scheme_cards(d, Slot::Dark, cx);
        vec![
            windows("In light windows").into_any_element(),
            grid("schemes-light", "Light palette", light, None),
            windows("In dark windows").into_any_element(),
            grid("schemes-dark", "Dark palette", night, Some(add_theme)),
        ]
    } else {
        let all = scheme_cards(d, Slot::Fixed, cx);
        vec![grid("color-schemes", "Palette", all, Some(add_theme))]
    };
    let editing = editor(d, window, cx);
    let themes_dir = d.core.themes_dir();
    let new_scheme = generator(
        d,
        Kind::Scheme,
        "Warm paper, ink blue",
        themes_dir,
        window,
        cx,
    );
    div()
        .flex()
        .flex_col()
        .children(grids)
        .children(editing)
        .child(new_scheme)
        .into_any_element()
}

/// The code themes, for the kind of window that is showing, and the row that writes one.
fn code_theme_pane(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let scheme = if t.is_dark() {
        Scheme::Dark
    } else {
        Scheme::Light
    };
    let code = code_theme_cards(d, scheme, cx);
    let code_dir = d.core.code_themes_dir();
    let new_code = generator(
        d,
        Kind::Code,
        "Soft pastels, quiet comments",
        code_dir,
        window,
        cx,
    );
    div()
        .flex()
        .flex_col()
        .child(code)
        .child(new_code)
        .into_any_element()
}

/// A pane of themes, which imports a theme's file dropped anywhere on it and shows the files
/// that could not be read.
fn pane(d: &SettingsDialog, cx: &mut Context<SettingsDialog>) -> gpui::Stateful<gpui::Div> {
    let t = *theme(cx);
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
}
