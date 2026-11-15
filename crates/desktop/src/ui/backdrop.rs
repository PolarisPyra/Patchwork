//! The chat's background, behind everything it draws: its own, else the one in Settings. A preset
//! is a gradient, with dots or a grid in the window's text color; a picture covers the chat. The
//! window's color is laid over either.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    AnyElement, Bounds, Hsla, ObjectFit, canvas, div, fill, img, linear_color_stop,
    linear_gradient, point, prelude::*, px, rgb, size,
};
use patchwork_server::backgrounds::{BackgroundImage, ChatBackground};

use patchwork_server::themes::SavedScene;

use crate::orchestrator::{PreviewFlight, butterfly_backdrop, scene_backdrop};
use crate::ui::theme::{Theme, applied};
use crate::workspace::Workspace;

/// What a preset paints over its base.
enum Pattern {
    /// Dots this many pixels apart.
    Dots(f32),
    Grid,
}

/// What a preset paints under its pattern: a gradient between two colors, or the window color.
enum Base {
    Gradient(u32, u32),
    Window,
}

/// The backgrounds the app comes with, by their id in Settings.
fn preset(id: &str) -> Option<(Base, Pattern)> {
    Some(match id {
        "sky" => (Base::Gradient(0x00a1_c4fd, 0x00c2_e9fb), Pattern::Dots(18.)),
        "peach" => (Base::Gradient(0x00ff_ecd2, 0x00fc_b69f), Pattern::Dots(18.)),
        "mint" => (Base::Gradient(0x00d4_fc79, 0x0096_e6a1), Pattern::Dots(18.)),
        "lavender" => (Base::Gradient(0x00e0_c3fc, 0x008e_c5fc), Pattern::Dots(18.)),
        "dusk" => (Base::Gradient(0x002b_5876, 0x004e_4376), Pattern::Dots(22.)),
        "forest" => (Base::Gradient(0x0013_4e5e, 0x0071_b280), Pattern::Dots(22.)),
        "grid" => (Base::Window, Pattern::Grid),
        "dots" => (Base::Window, Pattern::Dots(14.)),
        _ => return None,
    })
}

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
        let server = Arc::clone(&self.server);
        let task =
            cx.background_spawn(
                async move { (server.list_background_images(), server.list_scenes()) },
            );
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

/// The pattern's marks, painted over the chat's bounds.
fn pattern(pattern: Pattern, color: Hsla) -> AnyElement {
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<gpui::Pixels>, (), window, _| {
            let (left, top) = (f32::from(bounds.left()), f32::from(bounds.top()));
            let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            match pattern {
                Pattern::Dots(gap) => {
                    let mut y = 0.;
                    while y < height {
                        let mut x = 0.;
                        while x < width {
                            let dot = Bounds::new(
                                point(px(left + x - 1.), px(top + y - 1.)),
                                size(px(2.), px(2.)),
                            );
                            window.paint_quad(fill(dot, color).corner_radii(px(1.)));
                            x += gap;
                        }
                        y += gap;
                    }
                }
                Pattern::Grid => {
                    let mut x = 0.;
                    while x < width {
                        let line = Bounds::new(
                            point(px(left + x), bounds.top()),
                            size(px(1.), bounds.size.height),
                        );
                        window.paint_quad(fill(line, color));
                        x += 24.;
                    }
                    let mut y = 0.;
                    while y < height {
                        let line = Bounds::new(
                            point(bounds.left(), px(top + y)),
                            size(bounds.size.width, px(1.)),
                        );
                        window.paint_quad(fill(line, color));
                        y += 24.;
                    }
                }
            }
        },
    )
    .absolute()
    .inset_0()
    .into_any_element()
}

/// The backgrounds the app comes with, by id and name, in the order Settings lists them.
pub const PRESETS: [(&str, &str); 8] = [
    ("sky", "Sky"),
    ("peach", "Peach"),
    ("mint", "Mint"),
    ("lavender", "Lavender"),
    ("dusk", "Dusk"),
    ("forest", "Forest"),
    ("grid", "Grid"),
    ("dots", "Dots"),
];

/// A preset drawn over the whole of its parent, which must be `relative`.
pub fn preset_fill(id: &str, t: &Theme) -> Option<gpui::Div> {
    let (base, marks) = preset(id)?;
    let base = match base {
        Base::Gradient(from, to) => div().absolute().inset_0().bg(linear_gradient(
            160.,
            linear_color_stop(rgb(from), 0.),
            linear_color_stop(rgb(to), 1.),
        )),
        Base::Window => div().absolute().inset_0().bg(t.bg_editor),
    };
    let mark = match marks {
        Pattern::Dots(_) => t.text.opacity(0.22),
        Pattern::Grid => t.text.opacity(0.10),
    };
    Some(
        div()
            .absolute()
            .inset_0()
            .overflow_hidden()
            .child(base)
            .child(pattern(marks, mark)),
    )
}

pub fn chat_backdrop(
    w: &mut Workspace,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Option<AnyElement> {
    let background = w
        .thread
        .as_ref()
        .and_then(|t| t.background.clone())
        .unwrap_or_else(|| w.settings.chat_background.clone());
    let under = match &background {
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
