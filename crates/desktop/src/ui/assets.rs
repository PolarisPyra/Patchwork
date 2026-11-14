//! The files the window draws with, built into the binary: the fonts, icons and provider marks in
//! the repository's `assets/`.
//!
//! The derived files are made from these npm packages:
//!
//! - `assets/fonts/`: `Inter-{400,500,600,700,400-italic}.ttf` and `JetBrainsMono-{400,700}.ttf`,
//!   the fonts the app and code are set in until Settings picks one the computer has. They are
//!   static instances of the `latin` variable fonts in `@fontsource-variable/inter` and
//!   `@fontsource-variable/jetbrains-mono`. Decompress each `*-latin-wght-*.woff2` with
//!   `uvx --with brotli --from fonttools fonttools ttLib.woff2 decompress <in> -o <out>.ttf`, then
//!   pin a weight with `uvx --from fonttools fonttools varLib.instancer <out>.ttf wght=<weight>
//!   --update-name-table -o <Family>-<weight>.ttf`. Each family's license is in
//!   `docs/font-licenses/`.
//! - `assets/icons/`: one SVG per Phosphor icon the app draws, as `<kebab-name>.svg`
//!   (regular), `-bold.svg` and `-fill.svg`, written from the `dist/defs/<Name>.es.js` files of
//!   `@phosphor-icons/react` with their path data in a 256×256 `viewBox`. `twistie.svg` is the
//!   Codicons chevron the task tree draws, and
//!   `twistie-closed.svg` is the same path with `rotate(-90 8 8)` put first in its `transform`.

// Rust guideline compliant 2026-10-09

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../assets"]
#[include = "fonts/*"]
#[include = "icons/*"]
struct Own;

#[derive(RustEmbed)]
#[folder = "../../assets"]
#[include = "*.svg"]
#[exclude = "icons/*"]
struct Logos;

/// Serves `fonts/…` and `icons/…` from this app's assets, and from the shared ones `logos/…`, the
/// provider marks, and `app-icons/…`, the app icons in `assets/logos/`.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(name) = path.strip_prefix("app-icons/") {
            return Ok(Logos::get(&format!("logos/{name}")).map(|file| file.data));
        }
        Ok(match path.strip_prefix("logos/") {
            Some(name) => Logos::get(name).map(|file| file.data),
            None => Own::get(path).map(|file| file.data),
        })
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(Own::iter()
            .filter(|name| name.starts_with(path))
            .map(SharedString::from)
            .collect())
    }
}

/// The fonts the UI and code are set in.
pub fn fonts() -> Vec<Cow<'static, [u8]>> {
    Own::iter()
        .filter(|name| name.starts_with("fonts/"))
        .filter_map(|name| Own::get(&name).map(|file| file.data))
        .collect()
}
