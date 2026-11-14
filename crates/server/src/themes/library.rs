//! The color schemes and animated backgrounds that can be picked, and what the settings come to.

// Rust guideline compliant 2026-10-09

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

use super::code::{CodeTheme, CodeThemeFile, Syntax};
use super::contrast;
use super::types::{Entry, Palette, SavedScene, SceneFile, Scheme, Theme, ThemeFile, Unreadable};
use crate::{Error, Server};

/// One kind of file that can be picked: its folder, and the files Patchwork comes with.
pub(super) struct Shelf<T> {
    /// What one of its files is called in a message.
    noun: &'static str,
    /// The folder's name in Patchwork's configuration folder.
    folder: &'static str,
    /// The id of each file Patchwork comes with, and its text.
    presets: &'static [(&'static str, &'static str)],
    check: fn(&T) -> Result<(), String>,
    /// What to tell a person about a file that can be used but has something wrong with it.
    notes: fn(&T) -> Vec<String>,
}

/// The color schemes.
pub(super) const THEMES: Shelf<ThemeFile> = Shelf {
    noun: "color scheme",
    folder: "themes",
    presets: &[
        (
            "butterfly-night",
            include_str!("../../../../assets/themes/butterfly-night.json"),
        ),
        (
            "night-reef",
            include_str!("../../../../assets/themes/night-reef.json"),
        ),
        (
            "lagoon",
            include_str!("../../../../assets/themes/lagoon.json"),
        ),
        (
            "terminal",
            include_str!("../../../../assets/themes/terminal.json"),
        ),
        (
            "blossom",
            include_str!("../../../../assets/themes/blossom.json"),
        ),
    ],
    check: |_| Ok(()),
    notes: contrast::notes,
};

/// The animated backgrounds.
pub(super) const SCENES: Shelf<SceneFile> = Shelf {
    noun: "background",
    folder: "scenes",
    presets: &[
        (
            "night-reef",
            include_str!("../../../../assets/scenes/night-reef.json"),
        ),
        (
            "lagoon",
            include_str!("../../../../assets/scenes/lagoon.json"),
        ),
        (
            "terminal",
            include_str!("../../../../assets/scenes/terminal.json"),
        ),
        (
            "blossom",
            include_str!("../../../../assets/scenes/blossom.json"),
        ),
    ],
    check: SceneFile::check,
    notes: |_| Vec::new(),
};

/// The code themes.
pub(super) const CODE: Shelf<CodeThemeFile> = Shelf {
    noun: "code theme",
    folder: "code-themes",
    presets: &[
        (
            "github-light",
            include_str!("../../../../assets/code-themes/github-light.json"),
        ),
        (
            "catppuccin-latte",
            include_str!("../../../../assets/code-themes/catppuccin-latte.json"),
        ),
        (
            "rose-pine-dawn",
            include_str!("../../../../assets/code-themes/rose-pine-dawn.json"),
        ),
        (
            "light-high-contrast",
            include_str!("../../../../assets/code-themes/light-high-contrast.json"),
        ),
        (
            "light-colorblind",
            include_str!("../../../../assets/code-themes/light-colorblind.json"),
        ),
        (
            "github-dark",
            include_str!("../../../../assets/code-themes/github-dark.json"),
        ),
        (
            "catppuccin-mocha",
            include_str!("../../../../assets/code-themes/catppuccin-mocha.json"),
        ),
        (
            "rose-pine",
            include_str!("../../../../assets/code-themes/rose-pine.json"),
        ),
        (
            "dark-high-contrast",
            include_str!("../../../../assets/code-themes/dark-high-contrast.json"),
        ),
        (
            "dark-colorblind",
            include_str!("../../../../assets/code-themes/dark-colorblind.json"),
        ),
    ],
    check: |_| Ok(()),
    notes: |_| Vec::new(),
};

/// The text of each file Patchwork wrote into a folder, as a hash under `folder/id`.
///
/// A file that still has the hash was not edited, so a newer Patchwork may write over it.
type Seeded = BTreeMap<String, String>;

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut hash, b| {
            let _ = write!(hash, "{b:02x}");
            hash
        })
}

/// Whether `id` can be a file's name: letters, digits, `-` and `_`.
fn is_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

impl<T: DeserializeOwned + Serialize + PartialEq> Shelf<T> {
    /// The folder its files are in: the ones Patchwork comes with, and the ones a person or
    /// an agent wrote.
    pub(super) fn dir(&self) -> PathBuf {
        crate::system::dirs().config.join(self.folder)
    }

    /// Where the file with this id is, or would be.
    pub(super) fn path(&self, id: &str) -> PathBuf {
        self.dir().join(format!("{id}.json"))
    }

    pub(super) fn parse(&self, text: &str) -> Result<T, String> {
        let file: T = serde_json::from_str(text).map_err(|e| e.to_string())?;
        (self.check)(&file)?;
        Ok(file)
    }

    /// Checks a file that was read as part of another.
    pub(super) fn check(&self, file: &T) -> Result<(), String> {
        (self.check)(file)
    }

    /// Whether Patchwork comes with a file of this id.
    fn comes_with(&self, id: &str) -> bool {
        self.presets.iter().any(|(preset, _)| *preset == id)
    }

    /// Writes the files Patchwork comes with into the folder, so they can be read and edited
    /// there.
    ///
    /// A missing one is written, and so is one a newer Patchwork changed, as long as the file
    /// is still what Patchwork wrote. An edited one is left as it is.
    fn seed(&self, seeded: &mut Seeded) {
        let (dir, noun) = (self.dir(), self.noun);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!(noun, error:% = e; "could not make the {{noun}} folder: {{error}}");
            return;
        }
        for (id, text) in self.presets {
            let path = self.path(id);
            let key = format!("{}/{id}", self.folder);
            let ours = hash(text.as_bytes());
            let theirs = std::fs::read(&path).ok().map(|bytes| hash(&bytes));
            if theirs.as_ref() == Some(&ours) {
                seeded.insert(key, ours);
                continue;
            }
            if theirs.is_some() && theirs.as_ref() != seeded.get(&key) {
                continue;
            }
            match std::fs::write(&path, text) {
                Ok(()) => {
                    seeded.insert(key, ours);
                }
                Err(e) => {
                    log::warn!(id, noun, error:% = e; "could not write {{noun}} {{id}}: {{error}}");
                }
            }
        }
    }

    fn entry(&self, id: &str, custom: bool, edited: bool, file: T) -> Entry<T> {
        Entry {
            id: id.to_string(),
            custom,
            edited,
            notes: (self.notes)(&file),
            file,
        }
    }

    /// The file with this id: the one in the folder, else the one Patchwork comes with.
    pub(super) fn load(&self, id: &str) -> Option<Entry<T>> {
        if !is_id(id) {
            return None;
        }
        let noun = self.noun;
        let preset = self.presets.iter().find(|(preset, _)| *preset == id);
        if let Ok(text) = std::fs::read_to_string(self.path(id)) {
            match self.parse(&text) {
                Ok(file) => {
                    let edited = preset.is_some_and(|(_, ours)| *ours != text);
                    return Some(self.entry(id, preset.is_none(), edited, file));
                }
                Err(e) => {
                    log::warn!(id, noun, error:% = e; "{{noun}} {{id}} cannot be read: {{error}}");
                }
            }
        }
        let (_, text) = preset?;
        self.parse(text)
            .ok()
            .map(|file| self.entry(id, false, false, file))
    }

    /// The ids of the files in the folder, by name.
    fn ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = std::fs::read_dir(self.dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let json = path.extension().is_some_and(|e| e == "json");
                let stem = path.file_stem()?.to_str()?.to_string();
                (json && is_id(&stem)).then_some(stem)
            })
            .collect();
        ids.sort();
        ids
    }

    /// The files in the folder that cannot be used, and why.
    fn unreadable(&self) -> Vec<Unreadable> {
        self.ids()
            .into_iter()
            .filter_map(|id| {
                let error = match std::fs::read_to_string(self.path(&id)) {
                    Ok(text) => self.parse(&text).err()?,
                    Err(e) => e.to_string(),
                };
                Some(Unreadable {
                    file: format!("{}/{id}.json", self.folder),
                    error,
                })
            })
            .collect()
    }

    /// Writes `file` into the folder under an id made from `name`, and returns the id. A file
    /// that is already there with the same contents is kept, and its id returned.
    pub(super) fn add(&self, name: &str, file: &T) -> Result<String, Error> {
        if let Some(same) = self.list().into_iter().find(|entry| &entry.file == file) {
            return Ok(same.id);
        }
        let noun = self.noun;
        let id = self.free_id(name);
        let text = serde_json::to_string_pretty(file).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(self.dir())
            .and_then(|()| std::fs::write(self.path(&id), text))
            .map_err(|e| format!("couldn't save the {noun}: {e}"))?;
        Ok(id)
    }

    /// The ones Patchwork comes with, then the files in the folder by name. A file with the id
    /// of one Patchwork comes with stands in for it.
    fn list(&self) -> Vec<Entry<T>> {
        let ids = self.ids();
        let presets = self.presets.iter().map(|(id, _)| (*id).to_string());
        let custom = ids.iter().filter(|id| !self.comes_with(id));
        presets
            .chain(custom.cloned())
            .filter_map(|id| self.load(&id))
            .collect()
    }

    fn written(&self, id: &str) -> bool {
        is_id(id) && self.path(id).is_file()
    }

    fn remove(&self, id: &str) -> Result<(), Error> {
        let noun = self.noun;
        if !is_id(id) {
            return Err(format!("{id} is not a {noun}").into());
        }
        std::fs::remove_file(self.path(id)).map_err(|e| format!("couldn't delete {id}: {e}"))?;
        Ok(())
    }

    /// Writes `file` over the one with this id.
    pub(super) fn write(&self, id: &str, file: &T) -> Result<(), Error> {
        let noun = self.noun;
        if !is_id(id) {
            return Err(format!("{id} is not a {noun}").into());
        }
        let text = serde_json::to_string_pretty(file).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(self.dir())
            .and_then(|()| std::fs::write(self.path(id), text))
            .map_err(|e| format!("couldn't save the {noun}: {e}"))?;
        Ok(())
    }

    /// Writes the file Patchwork comes with under this id over the one in the folder.
    fn reset(&self, id: &str) -> Result<(), Error> {
        let noun = self.noun;
        let (_, text) = self
            .presets
            .iter()
            .find(|(preset, _)| *preset == id)
            .ok_or_else(|| format!("Patchwork has no {noun} {id} of its own"))?;
        std::fs::create_dir_all(self.dir())
            .and_then(|()| std::fs::write(self.path(id), text))
            .map_err(|e| format!("couldn't reset {id}: {e}"))?;
        Ok(())
    }

    /// An id made from `description` that no file has yet.
    pub(super) fn free_id(&self, description: &str) -> String {
        let mut slug = String::new();
        for c in description.trim().chars().take(40) {
            if c.is_ascii_alphanumeric() {
                slug.push(c.to_ascii_lowercase());
            } else if !slug.ends_with('-') && !slug.is_empty() {
                slug.push('-');
            }
        }
        let slug = slug.trim_end_matches('-');
        let slug = if slug.is_empty() { self.folder } else { slug };
        let taken: Vec<String> = self.list().into_iter().map(|entry| entry.id).collect();
        std::iter::once(slug.to_string())
            .chain((2..=taken.len() + 2).map(|n| format!("{slug}-{n}")))
            .find(|id| !taken.contains(id))
            .unwrap_or_else(|| slug.to_string())
    }
}

/// Puts the color schemes, animated backgrounds and code themes Patchwork comes with in their
/// folders. Call it once the config folder is known.
pub fn init() {
    let path = crate::system::dirs().data.join("themes-seeded.json");
    let before: Seeded = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let mut seeded = before.clone();
    THEMES.seed(&mut seeded);
    SCENES.seed(&mut seeded);
    CODE.seed(&mut seeded);
    if seeded == before {
        return;
    }
    let saved = serde_json::to_string_pretty(&seeded)
        .map_err(|e| e.to_string())
        .and_then(|text| std::fs::write(&path, text).map_err(|e| e.to_string()));
    if let Err(e) = saved {
        log::warn!(error:% = e; "could not save which themes were written: {{error}}");
    }
}

impl Server {
    /// The color schemes that can be picked: the ones Patchwork comes with, then the files in
    /// the themes folder by name. Reads files, so call it off the UI thread.
    #[must_use]
    pub fn list_themes(&self) -> Vec<Theme> {
        THEMES.list()
    }

    /// The animated backgrounds that can be picked: the ones Patchwork comes with, then the
    /// files in the scenes folder by name. Reads files, so call it off the UI thread.
    #[must_use]
    pub fn list_scenes(&self) -> Vec<SavedScene> {
        SCENES.list()
    }

    /// The code themes that can be picked: the ones Patchwork comes with, then the files in the
    /// code themes folder by name. Reads files, so call it off the UI thread.
    #[must_use]
    pub fn list_code_themes(&self) -> Vec<CodeTheme> {
        CODE.list()
    }

    /// The files in the themes, scenes and code themes folders that cannot be used, and why.
    /// Reads files, so call it off the UI thread.
    #[must_use]
    pub fn unreadable_themes(&self) -> Vec<Unreadable> {
        let mut out = THEMES.unreadable();
        out.extend(SCENES.unreadable());
        out.extend(CODE.unreadable());
        out
    }

    /// The folder the code themes are in.
    #[must_use]
    pub fn code_themes_dir(&self) -> PathBuf {
        CODE.dir()
    }

    /// The folder the color schemes are in.
    #[must_use]
    pub fn themes_dir(&self) -> PathBuf {
        THEMES.dir()
    }

    /// The folder the animated backgrounds are in.
    #[must_use]
    pub fn scenes_dir(&self) -> PathBuf {
        SCENES.dir()
    }

    /// Whether the themes folder has a file for the color scheme with this id.
    #[must_use]
    pub fn theme_written(&self, id: &str) -> bool {
        THEMES.written(id)
    }

    /// Whether the scenes folder has a file for the animated background with this id.
    #[must_use]
    pub fn scene_written(&self, id: &str) -> bool {
        SCENES.written(id)
    }

    /// Deletes a color scheme's file. A setting that picked it falls back to Light, Dark,
    /// OLED or System.
    ///
    /// # Errors
    ///
    /// Returns an error when `id` is not a color scheme's id or its file cannot be deleted.
    pub fn remove_theme(&self, id: &str) -> Result<(), Error> {
        THEMES.remove(id)
    }

    /// Deletes an animated background's file. A setting that picked it shows no backdrop.
    ///
    /// # Errors
    ///
    /// Returns an error when `id` is not a background's id or its file cannot be deleted.
    pub fn remove_scene(&self, id: &str) -> Result<(), Error> {
        SCENES.remove(id)
    }

    /// Deletes a code theme's file. A setting that picked it uses the color scheme's colors.
    ///
    /// # Errors
    ///
    /// Returns an error when `id` is not a code theme's id or its file cannot be deleted.
    pub fn remove_code_theme(&self, id: &str) -> Result<(), Error> {
        CODE.remove(id)
    }

    /// Puts a color scheme Patchwork comes with back as it came.
    ///
    /// # Errors
    ///
    /// Returns an error when Patchwork has no such color scheme or its file cannot be written.
    pub fn reset_theme(&self, id: &str) -> Result<(), Error> {
        THEMES.reset(id)
    }

    /// Puts an animated background Patchwork comes with back as it came.
    ///
    /// # Errors
    ///
    /// Returns an error when Patchwork has no such background or its file cannot be written.
    pub fn reset_scene(&self, id: &str) -> Result<(), Error> {
        SCENES.reset(id)
    }

    /// Puts a code theme Patchwork comes with back as it came.
    ///
    /// # Errors
    ///
    /// Returns an error when Patchwork has no such code theme or its file cannot be written.
    pub fn reset_code_theme(&self, id: &str) -> Result<(), Error> {
        CODE.reset(id)
    }

    /// Writes a color scheme over the file with this id, as the editor in Settings changes it.
    ///
    /// # Errors
    ///
    /// Returns an error when `id` cannot be a file's name or the file cannot be written.
    pub fn write_theme(&self, id: &str, file: &ThemeFile) -> Result<(), Error> {
        THEMES.write(id, file)
    }
}

/// The palette with this id: a color scheme's file, else one Patchwork comes with.
pub(super) fn palette(id: &str) -> Option<Palette> {
    THEMES
        .load(id)
        .map(|theme| Palette::File(Box::new(theme.file)))
        .or_else(|| Palette::built_in(id))
}

/// The colors of the code theme with this id, when it is one for `scheme` windows.
pub(super) fn syntax(id: Option<&str>, scheme: Scheme) -> Option<Syntax> {
    let theme = CODE.load(id?)?;
    (theme.file.scheme == scheme).then_some(theme.file.colors)
}
