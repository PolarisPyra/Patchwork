//! What the appearance settings come to: the palette, the backdrop, the fonts and the spacing
//! a window is drawn with, for the project that is open and the time of day.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

use super::library::{SCENES, palette, syntax};
use super::schedule::dark_now;
use super::types::{Applied, Backdrop, BackdropChoice, Color, Fit, Paint, Palette, Scheme, Shown};
use crate::Server;
use crate::settings::{Settings, Theme as ThemeSetting};

/// How much room rows and their padding take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Density {
    Compact,
    #[default]
    Comfortable,
    Roomy,
}

impl Density {
    /// What a row's height and padding are multiplied by.
    #[must_use]
    pub fn scale(self) -> f32 {
        match self {
            Self::Compact => 0.85,
            Self::Comfortable => 1.,
            Self::Roomy => 1.2,
        }
    }
}

/// What decides whether a window that matches its surroundings is light or dark.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "by", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Follow {
    /// The system's own setting.
    #[default]
    System,
    /// The clock: light from `light_at` to `dark_at`, each in minutes after midnight.
    Clock { light_at: u16, dark_at: u16 },
    /// The sun at this place: light from sunrise to sunset.
    Sun { latitude: f32, longitude: f32 },
}

/// What a project is drawn in while it is the one open, in place of the app's own.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectLook {
    /// A color scheme, by its id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent: Option<Color>,
}

impl ProjectLook {
    /// Whether it changes nothing, so it need not be kept.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.theme.is_none() && self.accent.is_none()
    }
}

impl Palette {
    /// Whether its windows are light or dark.
    #[must_use]
    pub fn scheme(&self) -> Scheme {
        match self {
            Self::Light => Scheme::Light,
            Self::Dark | Self::Oled => Scheme::Dark,
            Self::File(file) => file.scheme,
        }
    }
}

impl Server {
    /// What the appearance settings come to: the palettes, the code themes, the backdrop.
    /// Reads their files, so call it off the UI thread once the window is open.
    #[must_use]
    pub fn applied_theme(&self) -> Applied {
        let settings = self.store.settings();
        let look = settings
            .active_project_id
            .as_ref()
            .and_then(|id| settings.project_looks.get(id))
            .cloned()
            .unwrap_or_default();
        let light = || {
            let id = settings.system_light.as_deref();
            id.and_then(palette).unwrap_or(Palette::Light)
        };
        let dark = || {
            let id = settings.system_dark.as_deref();
            id.and_then(palette).unwrap_or(Palette::Dark)
        };
        let project = look.theme.as_deref().and_then(palette);
        let picked = settings.theme_id.as_deref().and_then(palette);
        let matches = picked.is_none() && settings.theme == ThemeSetting::System;
        let clock = matches.then(|| dark_now(settings.follow)).flatten();
        let own = match (picked, settings.theme, clock) {
            (Some(palette), _, _) => Paint::Fixed(palette),
            (None, ThemeSetting::Light, _) => Paint::Fixed(Palette::Light),
            (None, ThemeSetting::Dark, _) => Paint::Fixed(Palette::Dark),
            (None, ThemeSetting::Oled, _) => Paint::Fixed(Palette::Oled),
            (None, ThemeSetting::System, Some((true, _))) => Paint::Fixed(dark()),
            (None, ThemeSetting::System, Some((false, _))) => Paint::Fixed(light()),
            (None, ThemeSetting::System, None) => Paint::System {
                light: light(),
                dark: dark(),
            },
        };
        let paint = project.map_or(own, Paint::Fixed);
        let slots = [
            &settings.system_light_backdrop,
            &settings.system_dark_backdrop,
        ];
        let slot = |scheme: Scheme| {
            let own = slots[usize::from(scheme == Scheme::Dark)].as_ref();
            self.backdrop(own.unwrap_or(&settings.orchestrator_backdrop))
        };
        let (backdrop, system_backdrops) = match &paint {
            Paint::Fixed(palette) if matches => (slot(palette.scheme()), None),
            Paint::Fixed(_) => (self.backdrop(&settings.orchestrator_backdrop), None),
            Paint::System { .. } => {
                let pair = [slot(Scheme::Light), slot(Scheme::Dark)];
                (pair[1].clone(), Some(Box::new(pair)))
            }
        };
        let named = |font: &Option<String>| font.clone().filter(|font| !font.trim().is_empty());
        Applied {
            paint,
            code_light: syntax(settings.code_theme_light.as_deref(), Scheme::Light),
            code_dark: syntax(settings.code_theme_dark.as_deref(), Scheme::Dark),
            backdrop,
            system_backdrops,
            accent: look.accent.or(settings.accent),
            ui_font: named(&settings.ui_font),
            code_font: named(&settings.code_font),
            chat_font: named(&settings.chat_font),
            text_size: settings.text_size,
            density: settings.density,
            switch_in: clock.map(|(_, wait)| wait),
            reduce_motion: settings.reduce_motion,
        }
    }

    /// What a backdrop setting draws, and the text that reads over it.
    fn backdrop(&self, choice: &BackdropChoice) -> Backdrop {
        let (shown, scheme) = match choice {
            BackdropChoice::None => (Shown::None, None),
            // The butterfly is drawn on its own night sky, whatever the colors.
            BackdropChoice::Butterfly => (Shown::Butterfly, Some(Scheme::Dark)),
            BackdropChoice::Image {
                file,
                dim,
                fit,
                blur,
            } => {
                let path = self.backgrounds_dir.join(file);
                // Only a file of the folder itself, which still exists.
                let inside = !file.contains(['/', '\\']) && path.is_file();
                let shown = if inside {
                    let size = (*fit == Fit::Tile)
                        .then(|| image::image_dimensions(&path).ok())
                        .flatten();
                    let soft = (*blur > 0)
                        .then(|| self.blurred_background(file, *blur))
                        .flatten();
                    Shown::Image {
                        path: soft.unwrap_or(path),
                        dim: dim.share(),
                        fit: *fit,
                        size,
                    }
                } else {
                    Shown::None
                };
                (shown, None)
            }
            BackdropChoice::Scene { id } => SCENES.load(id).map_or((Shown::None, None), |saved| {
                (Shown::Scene(saved.file.scene), Some(saved.file.scheme))
            }),
        };
        Backdrop { shown, scheme }
    }
}

impl Settings {
    /// The Orchestrator background to change for windows of `scheme`: that side's own, which
    /// starts as the one every window shows.
    pub fn backdrop_mut(&mut self, scheme: Option<Scheme>) -> &mut BackdropChoice {
        let shared = &mut self.orchestrator_backdrop;
        match scheme {
            Some(Scheme::Light) => self
                .system_light_backdrop
                .get_or_insert_with(|| shared.clone()),
            Some(Scheme::Dark) => self
                .system_dark_backdrop
                .get_or_insert_with(|| shared.clone()),
            None => shared,
        }
    }

    /// Stops showing the animated background `id`, which was deleted.
    pub fn drop_scene(&mut self, id: &str) {
        let gone = BackdropChoice::Scene { id: id.to_string() };
        if self.orchestrator_backdrop == gone {
            self.orchestrator_backdrop = BackdropChoice::default();
        }
        for slot in [
            &mut self.system_light_backdrop,
            &mut self.system_dark_backdrop,
        ] {
            if slot.as_ref() == Some(&gone) {
                *slot = None;
            }
        }
    }

    /// Stops drawing anything in the color scheme `id`, which was deleted.
    pub fn drop_theme(&mut self, id: &str) {
        for picked in [
            &mut self.theme_id,
            &mut self.system_light,
            &mut self.system_dark,
        ] {
            if picked.as_deref() == Some(id) {
                *picked = None;
            }
        }
        for look in self.project_looks.values_mut() {
            if look.theme.as_deref() == Some(id) {
                look.theme = None;
            }
        }
        self.project_looks.retain(|_, look| !look.is_empty());
    }

    /// Stops highlighting code in the code theme `id`, which was deleted.
    pub fn drop_code_theme(&mut self, id: &str) {
        for picked in [&mut self.code_theme_light, &mut self.code_theme_dark] {
            if picked.as_deref() == Some(id) {
                *picked = None;
            }
        }
    }

    /// Sets what the project `id` is drawn in. A look that changes nothing is not kept.
    pub fn set_project_look(&mut self, id: &str, change: impl FnOnce(&mut ProjectLook)) {
        let look = self.project_looks.entry(id.to_string()).or_default();
        change(look);
        if look.is_empty() {
            self.project_looks.remove(id);
        }
    }

    /// The Orchestrator background that is set for windows of `scheme`: its own when the app
    /// matches its surroundings and that side has one, else the one every window shows.
    #[must_use]
    pub fn backdrop_of(&self, scheme: Option<Scheme>) -> &BackdropChoice {
        let own = match scheme {
            Some(Scheme::Light) => self.system_light_backdrop.as_ref(),
            Some(Scheme::Dark) => self.system_dark_backdrop.as_ref(),
            None => None,
        };
        own.unwrap_or(&self.orchestrator_backdrop)
    }
}
