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
//! - The chat apps' logos in `assets/`: `telegram-color`, `discord-color`, `slack-color` and
//!   `mattermost-color` are the `telegram`, `discord-icon`, `slack-icon` and `mattermost-icon`
//!   bodies of `@iconify-json/logos`. `matrix` is `icons/matrix.svg` of `simple-icons` as it is;
//!   `signal-color` and `qq-color` are the `signal` and `qq` ones with
//!   `fill="#<hex>"` from its `data/simple-icons.json`. Each colour one has `width="24"
//!   height="24"` put first in its `<svg>`: an image is drawn at twice the size its file gives,
//!   and a 256px drawing shrunk to a 24px tile blurs.
//! - `assets/logos/<id>.png` and `assets/logo.png`: 512×512 renders of the SVG beside each, made
//!   with `rsvg-convert -w 512 -h 512 <id>.svg -o <id>.png`. `assets/logo.svg` is a copy of
//!   `assets/logos/original.svg`; `assets/logo.ico` is made from `logo.png` (see `build.rs`).

// Rust guideline compliant 2026-10-09

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../../assets"]
#[include = "fonts/*"]
struct Own;

#[derive(RustEmbed)]
#[folder = "../../../assets"]
#[include = "*.svg"]
#[exclude = "icons/*"]
struct Logos;

/// The asset at `path`, as views ask for it.
#[must_use]
pub fn asset(path: impl Into<SharedString>) -> SharedString {
    path.into()
}

/// Serves `fonts/…` and `icons/…` from this app's assets, and from the shared ones `logos/…`, the
/// provider marks, and `app-icons/…`, the app icons in `assets/logos/`.
#[derive(Debug)]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let path = path.split_once('?').map_or(path, |(path, _)| path);
        if path.starts_with("icons/") {
            // gpui-component's own icons, such as the spinner's, where Patchwork has none.
            return Ok(patchwork_design_icons::load_icon_svg(path)
                .or_else(|| gpui::assets::Assets.load(path).ok().flatten()));
        }
        if let Some(name) = path.strip_prefix("app-icons/") {
            return Ok(Logos::get(&format!("logos/{name}")).map(|file| file.data));
        }
        Ok(match path.strip_prefix("logos/") {
            Some(name) => Logos::get(name).map(|file| file.data),
            None => Own::get(path).map(|file| file.data),
        })
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut out: Vec<SharedString> = Own::iter()
            .filter(|name| name.starts_with(path))
            .map(SharedString::from)
            .collect();
        out.extend(
            patchwork_design_icons::list_icon_paths(path)
                .into_iter()
                .map(SharedString::from),
        );
        Ok(out)
    }
}

/// The fonts the UI and code are set in.
#[must_use]
pub fn fonts() -> Vec<Cow<'static, [u8]>> {
    Own::iter()
        .filter(|name| name.starts_with("fonts/"))
        .filter_map(|name| Own::get(&name).map(|file| file.data))
        .collect()
}
