//! Logo files under `logos/`, and the marks of the API provider presets.
//!
//! The files are served by `patchwork_design_theme::Assets`; this draws them.

// Rust guideline compliant 2026-11-27

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use base64::Engine;
use gpui::{AnyElement, Image, ImageFormat, IntoElement, Styled, img, px};
use patchwork_design_icons::{IconName, icon, mono_svg};
use patchwork_design_theme::asset;

/// A logo file under `logos/`; a mono one takes the text color.
#[must_use]
pub fn logo_file(file: &str, mono: bool, size: f32) -> AnyElement {
    let path = asset(format!("logos/{file}"));
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
        None => icon(IconName::PlugsConnected, size).into_any_element(),
    }
}

/// A `data:image/…;base64,` logo as an image, decoded once.
fn logo_image(url: &str) -> Option<Arc<Image>> {
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
fn preset_logo(preset: &str) -> Option<(&'static str, bool)> {
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
