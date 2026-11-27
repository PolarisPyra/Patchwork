//! Patchwork's own look.
//!
//! The fonts, icons and marks built into the binary, and the agents', providers' and chat apps'
//! logos.
//!
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
use std::sync::atomic::{AtomicU32, Ordering};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use gpui::{
    AssetSource, Result, SharedString, AnyElement, Image, ImageFormat, IntoElement, Styled, img, px,
};
use rust_embed::RustEmbed;
use base64::Engine;

use crate::icons::{icon, mono_svg};

#[derive(RustEmbed)]
#[folder = "../../../../assets"]
#[include = "fonts/*"]
#[include = "icons/*"]
struct Own;

#[derive(RustEmbed)]
#[folder = "../../../../assets"]
#[include = "*.svg"]
#[exclude = "icons/*"]
struct Logos;

/// How many times SVG files changed while a development build ran.
static GENERATION: AtomicU32 = AtomicU32::new(0);

/// The asset at `path`, as views ask for it.
///
/// In a development build, once an SVG file has changed, the path is marked with the times they
/// have, so the window reads the file again instead of drawing what it cached.
#[must_use]
pub fn asset(path: impl Into<SharedString>) -> SharedString {
    let path = path.into();
    match GENERATION.load(Ordering::Relaxed) {
        0 => path,
        n => format!("{path}?{n}").into(),
    }
}

/// Notes that SVG files changed, in a development build, which reads them from disk; the
/// windows must then be drawn again.
pub fn svgs_changed() {
    if cfg!(debug_assertions) {
        GENERATION.fetch_add(1, Ordering::Relaxed);
    }
}

/// Serves `fonts/…` and `icons/…` from this app's assets, and from the shared ones `logos/…`, the
/// provider marks, and `app-icons/…`, the app icons in `assets/logos/`.
#[derive(Debug)]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let path = path.split_once('?').map_or(path, |(path, _)| path);
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
#[must_use]
pub fn fonts() -> Vec<Cow<'static, [u8]>> {
    Own::iter()
        .filter(|name| name.starts_with("fonts/"))
        .filter_map(|name| Own::get(&name).map(|file| file.data))
        .collect()
}

/// A logo file under `logos/`; a mono one takes the text color.
#[must_use]
pub fn logo_file(file: &str, mono: bool, size: f32) -> AnyElement {
    let path = self::asset(format!("logos/{file}"));
    if mono {
        mono_svg(path, size).into_any_element()
    } else {
        img(path).size(px(size)).flex_none().into_any_element()
    }
}

/// An API provider's mark: the picture it was given, else its preset's, else a plug.
pub fn provider_logo(preset: &str, logo: Option<&str>, size: f32) -> AnyElement {
    if let Some(image) = logo.and_then(logo_image) {
        return img(image).size(px(size)).flex_none().into_any_element();
    }
    match preset_logo(preset) {
        Some((file, mono)) => logo_file(file, mono, size),
        None => icon("plugs-connected", size).into_any_element(),
    }
}

/// A `data:image/…;base64,` logo as an image, decoded once.
pub(crate) fn logo_image(url: &str) -> Option<Arc<Image>> {
    static DECODED: OnceLock<Mutex<HashMap<String, Arc<Image>>>> = OnceLock::new();
    let decoded = DECODED.get_or_init(Default::default);
    let mut decoded = decoded.lock().ok()?;
    if let Some(image) = decoded.get(url) {
        return Some(Arc::clone(image));
    }
    let (head, data) = url.strip_prefix("data:")?.split_once(";base64,")?;
    let format = ImageFormat::from_mime_type(head)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    let image = Arc::new(Image::from_bytes(format, bytes));
    decoded.insert(url.to_string(), Arc::clone(&image));
    Some(image)
}

/// The mark of an API provider preset (`PRESET_LOGOS`).
pub(crate) fn preset_logo(preset: &str) -> Option<(&'static str, bool)> {
    Some(match preset {
        "deepseek" => ("deepseek-color.svg", false),
        "openai" => ("openai.svg", true),
        "gemini" => ("gemini-color.svg", false),
        "xai" => ("grok.svg", true),
        "mistral" => ("mistral-color.svg", false),
        "zai" => ("zai.svg", true),
        "llama" => ("meta-color.svg", false),
        "moonshot" => ("kimi.svg", true),
        "dashscope" => ("qwen-color.svg", false),
        "minimax" => ("minimax-color.svg", false),
        "groq" => ("groq.svg", true),
        "together" => ("together-color.svg", false),
        "fireworks" => ("fireworks-color.svg", false),
        "cerebras" => ("cerebras.svg", true),
        "openrouter" => ("openrouter.svg", true),
        "perplexity" => ("perplexity-color.svg", false),
        "lmstudio" => ("lmstudio.svg", true),
        _ => return None,
    })
}
