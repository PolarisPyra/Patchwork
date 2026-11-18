//! The chat's background, behind everything it draws: its own, else its bot's, else the one in
//! Settings. A bot's page draws the bot's the same way.
//!
//! A preset or an animated backdrop is drawn as Settings previews it; a picture covers the chat,
//! with the window's color laid over it.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{AnyElement, ObjectFit, div, img, prelude::*};
use patchwork_core::backgrounds::{BackgroundImage, ChatBackground};
use patchwork_core::themes::{SavedScene, Scheme};

use crate::ui::backdrop::{PreviewFlight, butterfly_backdrop, preset_fill, scene_backdrop};
use crate::ui::theme::{Theme, applied};
use crate::workspace::Workspace;

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

impl Workspace {
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

/// The chat's own background, else its bot's, else the one in Settings.
fn chat_background(w: &Workspace) -> ChatBackground {
    let thread = w.thread.as_ref();
    thread
        .and_then(|t| t.background.clone())
        .or_else(|| {
            let bot = thread?.bot_id.as_ref()?;
            w.bots.iter().find(|b| &b.id == bot)?.background.clone()
        })
        .unwrap_or_else(|| w.settings.chat_background.clone())
}

/// The scheme the chat's background is drawn for, when it is drawn for one: the chat is then
/// drawn over it as the Orchestrator page is over its backdrop, so its text reads over it.
pub fn chat_scheme(w: &Workspace) -> Option<Scheme> {
    scheme_for(w, &chat_background(w))
}

/// The scheme `background` is drawn for, when it is drawn for one.
pub fn scheme_for(w: &Workspace, background: &ChatBackground) -> Option<Scheme> {
    let scene = match background {
        ChatBackground::Scene { id } => w.backgrounds.scenes.iter().find(|s| &s.id == id),
        _ => None,
    };
    background.scheme(scene.map(|saved| &saved.file))
}

pub fn chat_backdrop(
    w: &mut Workspace,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Option<AnyElement> {
    let background = chat_background(w);
    backdrop_for(w, &background, t, cx)
}

/// `background` drawn to cover the element it is put in, which must be relative.
pub fn backdrop_for(
    w: &mut Workspace,
    background: &ChatBackground,
    t: &Theme,
    cx: &mut Context<Workspace>,
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
