//! The chat's background, behind everything it draws: its own, else its agent's, else the one in
//! Settings. An agent's page draws the agent's the same way.
//!
//! A preset or an animated backdrop is drawn as Settings previews it; a picture covers the chat,
//! with the window's color laid over it.

// Rust guideline compliant 2026-11-27

use crate::prelude::*;
use patchwork_client::AppState;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{AnyElement, ObjectFit, div, img, prelude::*};
use patchwork_client::local::core::backgrounds::{BackgroundImage, ChatBackground, ChatBackgroundExt};
use patchwork_themes::{SavedScene, Scheme};

use patchwork_design_backgrounds::{PreviewFlight, butterfly_backdrop, preset_fill, scene_backdrop};
use patchwork_design_theme::{InScheme, Theme, applied, theme};

use patchwork_design_theme::in_scheme;

/// How much of the window color is laid over a chat's picture, as on the Orchestrator page.
const PICTURE_DIM: f32 = 0.45;

/// The pictures in Patchwork's backgrounds folder, read once.
#[derive(Default)]
pub struct Backgrounds {
    images: Option<Vec<BackgroundImage>>,
    /// The pictures changed, so they are listed again the next time a chat shows one.
    stale: bool,
    /// The animated backgrounds, read with the pictures.
    scenes: Vec<Arc<SavedScene>>,
    /// Where the butterfly behind a chat is, kept between frames.
    flight: PreviewFlight,
}

impl Backgrounds {
    /// Lists the pictures again the next time a chat shows one. The ones listed stay until
    /// then, so the chat's picture does not blink.
    pub fn reload(&mut self) {
        self.stale = true;
    }
}

/// The window's backdrop methods, for any [`ChatWindow`].
pub trait BackdropExt: ChatWindow {
    fn load_background_images(&mut self, cx: &mut Context<Self>) {
        if self.backgrounds.images.is_some() && !self.backgrounds.stale {
            return;
        }
        self.backgrounds.stale = false;
        self.backgrounds.images.get_or_insert_default();
        let core = Arc::clone(&self.core);
        let task =
            cx.background_spawn(async move { (core.list_background_images(), core.list_scenes()) });
        cx.spawn(async move |this, cx| {
            let (images, scenes) = task.await;
            let _ = this.update(cx, |this, cx| {
                match images {
                    Ok(images) => this.backgrounds.images = Some(images),
                    Err(e) => {
                        log::warn!(error:% = e; "could not list the background pictures: {{error}}");
                    }
                }
                this.backgrounds.scenes = scenes.into_iter().map(Arc::new).collect();
                cx.notify();
            });
        })
        .detach();
    }
}

impl<W: ChatWindow> BackdropExt for W {}

/// The chat's own background, else its agent's, else the one in Settings.
fn chat_background<W: ChatWindow>(w: &W, cx: &gpui::App) -> ChatBackground {
    let chat = w.chat.as_ref();
    chat.and_then(|t| t.background.clone())
        .or_else(|| {
            let agent = chat?.agent_id.as_ref()?;
            AppState::get(cx)
                .agents
                .iter()
                .find(|b| &b.id == agent)?
                .background
                .clone()
        })
        .unwrap_or_else(|| AppState::get(cx).settings.chat_background.clone())
}

/// The scheme the chat's background is drawn for, when it is drawn for one: the chat is then
/// drawn over it as the Orchestrator page is over its backdrop, so its text reads over it.
pub fn chat_scheme<W: ChatWindow>(w: &W, cx: &gpui::App) -> Option<Scheme> {
    scheme_for(w, &chat_background(w, cx))
}

/// The scheme `background` is drawn for, when it is drawn for one.
pub fn scheme_for<W: ChatWindow>(w: &W, background: &ChatBackground) -> Option<Scheme> {
    let scene = match background {
        ChatBackground::Scene { id } => w.backgrounds.scenes.iter().find(|s| &s.id == id),
        _ => None,
    };
    background.scheme(scene.map(|saved| &saved.file))
}

/// The chat's background on its own: the window's color and the backdrop over it.
///
/// The workspace
/// draws it as a view under the chat's, so a backdrop that moves redraws only itself, not the
/// transcript over it.
pub fn backdrop_layer<W: ChatWindow>(w: &mut W, cx: &mut Context<W>) -> AnyElement {
    let background = chat_background(w, cx);
    layer_for(w, &background, cx)
}

/// The Quilts page's background: its own from Settings, else the one every chat shares.
fn quilts_background(cx: &gpui::App) -> ChatBackground {
    AppState::get(cx)
        .settings
        .quilts_background
        .clone()
        .unwrap_or_else(|| AppState::get(cx).settings.chat_background.clone())
}

/// The scheme the Quilts page's background is drawn for, when it is drawn for one.
pub fn quilts_scheme<W: ChatWindow>(w: &W, cx: &gpui::App) -> Option<Scheme> {
    scheme_for(w, &quilts_background(cx))
}

/// The Quilts page's background on its own, as `backdrop_layer` draws the chat's.
pub fn quilts_backdrop_layer<W: ChatWindow>(w: &mut W, cx: &mut Context<W>) -> AnyElement {
    let background = quilts_background(cx);
    layer_for(w, &background, cx)
}

/// The window's color with `background` over it, in the scheme it is drawn for.
fn layer_for<W: ChatWindow>(
    w: &mut W,
    background: &ChatBackground,
    cx: &mut Context<W>,
) -> AnyElement {
    let scheme = scheme_for(w, background);
    let _scope = scheme.map(InScheme::enter);
    let t = *theme(cx);
    let layer = div()
        .relative()
        .size_full()
        .bg(t.bg_editor)
        .children(backdrop_for(w, background, &t, cx));
    match scheme {
        Some(scheme) => in_scheme(scheme, layer).into_any_element(),
        None => layer.into_any_element(),
    }
}

pub fn chat_backdrop<W: ChatWindow>(
    w: &mut W,
    t: &Theme,
    cx: &mut Context<W>,
) -> Option<AnyElement> {
    let background = chat_background(w, cx);
    backdrop_for(w, &background, t, cx)
}

/// `background` drawn to cover the element it is put in, which must be relative.
pub fn backdrop_for<W: ChatWindow>(
    w: &mut W,
    background: &ChatBackground,
    t: &Theme,
    cx: &mut Context<W>,
) -> Option<AnyElement> {
    let under = match background {
        ChatBackground::None => return None,
        ChatBackground::Preset { id } => preset_fill(id, t)?,
        ChatBackground::Butterfly => {
            let still = applied(cx).reduce_motion;
            div()
                .absolute()
                .inset_0()
                .child(butterfly_backdrop(&w.backgrounds.flight, still))
        }
        ChatBackground::Scene { id } => {
            w.load_background_images(cx);
            let saved = w.backgrounds.scenes.iter().find(|s| &s.id == id)?;
            let still = applied(cx).reduce_motion;
            div().absolute().inset_0().child(scene_backdrop(
                Arc::clone(saved),
                t.bg_editor,
                t.text_muted,
                still,
            ))
        }
        ChatBackground::Image { file } => {
            w.load_background_images(cx);
            let path = w
                .backgrounds
                .images
                .as_ref()?
                .iter()
                .find(|i| &i.file == file)?
                .path
                .clone();
            div()
                .absolute()
                .inset_0()
                .child(
                    img(PathBuf::from(path))
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Cover),
                )
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(t.bg_editor.opacity(PICTURE_DIM)),
                )
        }
    };
    Some(
        div()
            .absolute()
            .inset_0()
            .overflow_hidden()
            .child(under)
            .into_any_element(),
    )
}
