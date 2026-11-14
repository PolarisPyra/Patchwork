//! Sharing a color scheme as one file, and taking in a file someone shared.

// Rust guideline compliant 2026-10-09

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::code::from_base16 as code_base16;
use super::foreign::{from_base16, from_color_theme, from_zed_family, relaxed};
use super::library::{CODE, SCENES, THEMES};
use super::types::{SceneFile, ThemeFile};
use crate::{Error, Server};

/// A color scheme with the animated background it names, as one file.
#[derive(Serialize, Deserialize)]
struct Bundle {
    theme: ThemeFile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scene: Option<SceneFile>,
}

/// Which folder an imported file went to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportedKind {
    Scheme,
    Scene,
    Code,
}

/// A file that was imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imported {
    pub kind: ImportedKind,
    /// The id it can be picked by.
    pub id: String,
    pub name: String,
}

/// The largest file that is read as a theme. A real one is a few kilobytes.
const MAX_BYTES: u64 = 2 * 1024 * 1024;

impl Server {
    /// Writes the color scheme with this id to `to`, with the animated background it names in
    /// the same file.
    ///
    /// # Errors
    ///
    /// Returns an error when there is no such color scheme or the file cannot be written.
    pub fn export_theme(&self, id: &str, to: &Path) -> Result<(), Error> {
        let theme = THEMES
            .load(id)
            .ok_or_else(|| format!("{id} is not a color scheme"))?
            .file;
        let scene = theme
            .backdrop
            .as_deref()
            .and_then(|id| SCENES.load(id))
            .map(|saved| saved.file);
        let text =
            serde_json::to_string_pretty(&Bundle { theme, scene }).map_err(|e| e.to_string())?;
        std::fs::write(to, text).map_err(|e| format!("couldn't write {}: {e}", to.display()))?;
        Ok(())
    }

    /// Adds the file at `path` to the folder it belongs in: an exported color scheme with its
    /// background, a color scheme's, an animated background's or a code theme's own file, or
    /// another editor's theme: a base16 scheme, a JSON color theme or a Zed theme family.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read, is none of those, or cannot be saved.
    pub fn import_theme(&self, path: &Path) -> Result<Imported, Error> {
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let size = std::fs::metadata(path)
            .map_err(|e| format!("couldn't read {name}: {e}"))?
            .len();
        if size > MAX_BYTES {
            return Err(format!("{name} is too large to be a theme").into());
        }
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("couldn't read {name}: {e}"))?;
        let strict = serde_json::from_str::<serde_json::Value>(&text).ok();
        let Some(json) = strict.or_else(|| relaxed(&text)) else {
            let code = code_base16(&text).ok_or_else(|| format!("{name} is not a theme"))?;
            CODE.add(&code.name, &code)?;
            let theme =
                from_base16(&text, &code).ok_or_else(|| format!("{name} is not a theme"))?;
            return add_foreign(vec![theme], &name);
        };
        let unusable = |e: String| format!("{name} cannot be used: {e}");
        if json.get("theme").is_some() {
            let Bundle { mut theme, scene } =
                serde_json::from_value(json).map_err(|e| unusable(e.to_string()))?;
            if let Some(scene) = scene {
                SCENES.check(&scene).map_err(unusable)?;
                theme.backdrop = Some(SCENES.add(&scene.name, &scene)?);
            }
            let id = THEMES.add(&theme.name, &theme)?;
            Ok(imported(ImportedKind::Scheme, id, theme.name))
        } else if json.pointer("/colors/surface").is_some() {
            let theme = THEMES.parse(&text).map_err(unusable)?;
            let id = THEMES.add(&theme.name, &theme)?;
            Ok(imported(ImportedKind::Scheme, id, theme.name))
        } else if json.pointer("/colors/keyword").is_some() {
            let code = CODE.parse(&text).map_err(unusable)?;
            let id = CODE.add(&code.name, &code)?;
            Ok(imported(ImportedKind::Code, id, code.name))
        } else if let Some(theme) = from_color_theme(&json, stem(path).as_str()) {
            add_foreign(vec![theme], &name)
        } else if json.get("themes").is_some() {
            add_foreign(from_zed_family(&json), &name)
        } else {
            let scene = SCENES.parse(&text).map_err(unusable)?;
            let id = SCENES.add(&scene.name, &scene)?;
            Ok(imported(ImportedKind::Scene, id, scene.name))
        }
    }
}

/// Adds the color schemes read from another editor's file, and says which came first.
fn add_foreign(themes: Vec<ThemeFile>, name: &str) -> Result<Imported, Error> {
    let mut first = None;
    for theme in themes {
        THEMES
            .check(&theme)
            .map_err(|e| format!("{name} cannot be used: {e}"))?;
        let id = THEMES.add(&theme.name, &theme)?;
        first.get_or_insert(imported(ImportedKind::Scheme, id, theme.name));
    }
    first.ok_or_else(|| format!("{name} has no color scheme in it").into())
}

/// The name of the file at `path` without its extension.
fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn imported(kind: ImportedKind, id: String, name: String) -> Imported {
    Imported { kind, id, name }
}
