//! A running emulator's screen inside the panel, in a phone's frame: the picture the emulator
//! streams, taps and drags, the wheel and the keyboard sent back to it, its power and volume
//! keys on the frame's side, and Back, Home and Overview under it.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{
    AnyElement, Bounds, FocusHandle, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, RenderImage, Role, ScrollDelta, ScrollEvent, SharedString, Size,
    Task, Window, canvas, div, img, prelude::*, px, rgb,
};
use patchwork_server::android::{ScreenFrame, ScreenInput, ScreenSender};
use patchwork_server::{Error, Server};

use crate::ui::theme::{FS_SM, theme};
use crate::ui::{button, icon_button, spinner, tooltip};

/// The frame around the screen, as wide on every side.
const BEZEL: f32 = 10.;

/// The room under the frame for Back, Home and Overview.
const NAV: f32 = 44.;

/// The room kept clear around the phone, so it sits apart from the list above and the panel's
/// edges.
const MARGIN: f32 = 20.;

/// How far the side keys stand out of the frame.
const KEY_OUT: f32 = 3.;

/// The pixels a touchpad scrolls for one line of a wheel.
const PIXELS_PER_LINE: f32 = 40.;

/// Fit sizes are rounded to this many pixels, so a panel being resized does not restart the
/// emulator's stream at every step.
const FIT_STEP: u32 = 16;

/// The frame's colours: a phone is dark whatever the theme.
const BODY: u32 = 0x1d_1d_20;
const EDGE: u32 = 0x3a_3a_40;
const KEY: u32 = 0x2c_2c_31;
const KEY_HOVER: u32 = 0x4a_4a_52;
const GLASS: u32 = 0x00_00_00;

pub struct DeviceScreen {
    /// The emulator's serial, which the panel compares to the device picked.
    pub serial: String,
    name: String,
    sender: Option<ScreenSender>,
    /// The screen's size in pixels as the emulator was made, before the first frame arrives.
    display: (u32, u32),
    image: Option<Arc<RenderImage>>,
    /// The size of the last frame, in pixels.
    frame_size: Option<(u32, u32)>,
    error: Option<Error>,
    /// The room the frame is drawn in.
    area: Option<Size<Pixels>>,
    /// Where the screen itself is drawn, which taps are placed in.
    glass: Option<Bounds<Pixels>>,
    fit: Option<(u32, u32)>,
    pressed: bool,
    pub(super) focus: FocusHandle,
    _stream: Option<Task<()>>,
}

impl DeviceScreen {
    pub fn new(
        server: Arc<Server>,
        root: std::path::PathBuf,
        serial: String,
        name: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let opening = cx.background_spawn({
            let serial = serial.clone();
            async move { server.android_screen(&root, &serial) }
        });
        let stream = cx.spawn(async move |this, cx| {
            let mut screen = match opening.await {
                Ok(screen) => screen,
                Err(e) => {
                    let _ = this.update(cx, |this, cx| {
                        this.error = Some(e);
                        cx.notify();
                    });
                    return;
                }
            };
            let opened = this.update(cx, |this, cx| {
                this.sender = Some(screen.sender());
                this.display = screen.display();
                this.fit = None;
                cx.notify();
            });
            if opened.is_err() {
                return;
            }
            while let Some(next) = screen.next().await {
                if this.update(cx, |this, cx| this.show(next, cx)).is_err() {
                    break;
                }
            }
        });
        Self {
            serial,
            name,
            sender: None,
            display: (1080, 2400),
            image: None,
            frame_size: None,
            error: None,
            area: None,
            glass: None,
            fit: None,
            pressed: false,
            focus: cx.focus_handle(),
            _stream: Some(stream),
        }
    }

    /// Takes the newest frame in place of the one shown, or the reason there are no more.
    fn show(&mut self, next: Result<Arc<ScreenFrame>, Error>, cx: &mut Context<Self>) {
        match next {
            Ok(frame) => {
                let Some(buffer) =
                    image::RgbaImage::from_raw(frame.width, frame.height, frame.bgra.clone())
                else {
                    return;
                };
                let image = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
                if let Some(old) = self.image.replace(image) {
                    cx.drop_image(old, None);
                }
                self.frame_size = Some((frame.width, frame.height));
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    fn send(&self, input: ScreenInput) {
        if let Some(sender) = &self.sender {
            sender.send(input);
        }
    }

    /// The point `position` as fractions of the screen, when the screen has been drawn.
    fn place(&self, position: Point<Pixels>) -> Option<(f32, f32)> {
        let glass = self.glass?;
        let x = f32::from(position.x - glass.origin.x) / f32::from(glass.size.width);
        let y = f32::from(position.y - glass.origin.y) / f32::from(glass.size.height);
        Some((x, y))
    }

    fn touch(&self, position: Point<Pixels>, down: bool) {
        if let Some((x, y)) = self.place(position) {
            self.send(ScreenInput::Touch { x, y, down });
        }
    }

    fn key(&self, key: &str) {
        self.send(ScreenInput::Key(key.to_string()));
    }

    /// The screen's size on the panel: as large as fits the room under the frame and the keys
    /// below it, in the screen's own shape.
    fn glass_size(&self) -> Option<Size<Pixels>> {
        let area = self.area?;
        let (w, h) = self.frame_size.unwrap_or(self.display);
        #[allow(clippy::cast_precision_loss, reason = "a screen's pixel count")]
        let (w, h) = (w as f32, h as f32);
        let room_w = f32::from(area.width) - 2. * (MARGIN + BEZEL + KEY_OUT);
        let room_h = f32::from(area.height) - 2. * (MARGIN + BEZEL) - NAV;
        let scale = (room_w / w).min(room_h / h);
        (scale > 0.).then(|| Size::new(px(w * scale), px(h * scale)))
    }

    /// Asks the emulator for frames the size the screen is drawn at, on this display.
    fn fit(&mut self, glass: Size<Pixels>, window: &Window) {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a panel a few thousand pixels across"
        )]
        let pixels = |length: Pixels| {
            let exact = (f32::from(length) * window.scale_factor()).ceil() as u32;
            exact.div_ceil(FIT_STEP) * FIT_STEP
        };
        let fit = (pixels(glass.width), pixels(glass.height));
        if self.sender.is_some() && self.fit != Some(fit) {
            self.fit = Some(fit);
            self.send(ScreenInput::Fit {
                width: fit.0,
                height: fit.1,
            });
        }
    }

    /// A key on the frame's side, standing out of its right edge.
    fn side_key(
        &self,
        id: &'static str,
        label: &'static str,
        key: &'static str,
        top: f32,
        length: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        button(id, label)
            .absolute()
            .right(px(-KEY_OUT))
            .top(px(top))
            .w(px(KEY_OUT + 2.))
            .h(px(length))
            .rounded(px(2.))
            .bg(rgb(KEY))
            .hover(|s| s.bg(rgb(KEY_HOVER)))
            .tooltip(tooltip(label))
            .on_click(cx.listener(move |this, _, _, _| this.key(key)))
            .into_any_element()
    }

    /// The text shown on the dark glass while there is no picture.
    fn waiting(&self, t: &crate::ui::theme::Theme) -> AnyElement {
        match &self.error {
            Some(e) => div()
                .id("screen-error")
                .role(Role::Alert)
                .p(px(16.))
                .text_size(px(FS_SM))
                .text_color(rgb(0xd0_d0_d6))
                .child(e.to_string())
                .into_any_element(),
            None => div()
                .id("screen-waiting")
                .role(Role::Status)
                .aria_label("Connecting to the screen")
                .child(spinner(16., t.text_muted))
                .into_any_element(),
        }
    }
}

/// The wheel's turn in lines, positive towards the top of the page.
fn clicks(event: &ScrollEvent) -> f32 {
    match event.delta {
        ScrollDelta::Lines(lines) => lines.y,
        ScrollDelta::Pixels(pixels) => f32::from(pixels.y) / PIXELS_PER_LINE,
    }
}

/// The emulator's name for a key the keyboard pressed, for the keys that type no text.
fn key_name(key: &str) -> Option<&'static str> {
    Some(match key {
        "enter" => "Enter",
        "backspace" => "Backspace",
        "delete" => "Delete",
        "tab" => "Tab",
        "escape" => "Escape",
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        "up" => "ArrowUp",
        "down" => "ArrowDown",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        _ => return None,
    })
}

impl Render for DeviceScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let glass = self.glass_size();
        if let Some(glass) = glass {
            self.fit(glass, window);
        }
        let this = cx.entity().downgrade();
        let measure = canvas(
            move |bounds, _, cx| {
                let _ = this.update(cx, |this, cx| {
                    if this.area != Some(bounds.size) {
                        this.area = Some(bounds.size);
                        cx.notify();
                    }
                });
            },
            |_, (), _, _| {},
        )
        .absolute()
        .size_full();
        let Some(glass) = glass else {
            return div().relative().flex_1().min_h_0().child(measure);
        };
        let (gw, gh) = (f32::from(glass.width), f32::from(glass.height));
        // The screen's corners, as round as a phone's.
        let radius = gw * 0.08;
        let label: SharedString = format!("Screen of {}", self.name).into();
        let this = cx.entity().downgrade();
        let screen = div()
            .id("device-glass")
            .role(Role::Application)
            .aria_label(label)
            .tab_index(0)
            .track_focus(&self.focus)
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .w(glass.width)
            .h(glass.height)
            .rounded(px(radius))
            .overflow_hidden()
            .bg(rgb(GLASS))
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let _ = this.update(cx, |this, _| this.glass = Some(bounds));
                    },
                    |_, (), _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .map(|screen| match &self.image {
                // Clipping is square, so the picture rounds its own corners to stay inside the frame.
                Some(image) => screen.child(img(Arc::clone(image)).size_full().rounded(px(radius))),
                None => screen.child(self.waiting(&t)),
            })
            // The camera, in the screen's top centre.
            .child(
                div()
                    .absolute()
                    .top(px(gh * 0.012))
                    .left(px(gw / 2. - gw * 0.018))
                    .size(px(gw * 0.036))
                    .rounded_full()
                    .bg(rgb(GLASS)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    this.pressed = true;
                    this.touch(e.position, true);
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, _| {
                if this.pressed && e.pressed_button == Some(MouseButton::Left) {
                    this.touch(e.position, true);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, e: &MouseUpEvent, _, _| {
                    this.pressed = false;
                    this.touch(e.position, false);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, e: &MouseUpEvent, _, _| {
                    if std::mem::take(&mut this.pressed) {
                        this.touch(e.position, false);
                    }
                }),
            )
            .on_scroll(cx.listener(|this, e: &ScrollEvent, _, cx| {
                cx.stop_propagation();
                if let Some((x, y)) = this.place(e.position) {
                    this.send(ScreenInput::Wheel {
                        x,
                        y,
                        dy: clicks(e),
                    });
                }
            }))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                let stroke = &e.keystroke;
                let m = stroke.modifiers;
                if m.control || m.platform || m.alt {
                    return;
                }
                if let Some(name) = key_name(&stroke.key) {
                    this.key(name);
                } else if let Some(text) = &stroke.key_char {
                    this.send(ScreenInput::Text(text.clone()));
                } else {
                    return;
                }
                cx.stop_propagation();
            }));
        let corner = radius + BEZEL;
        let device = div()
            .id("device-frame")
            .relative()
            .flex_none()
            .p(px(BEZEL))
            .rounded(px(corner))
            .bg(rgb(BODY))
            .border_1()
            .border_color(rgb(EDGE))
            .child(screen)
            .child(self.side_key("device-power", "Power", "Power", gh * 0.2, gh * 0.08, cx))
            .child(self.side_key(
                "device-volume-up",
                "Volume up",
                "AudioVolumeUp",
                gh * 0.32,
                gh * 0.07,
                cx,
            ))
            .child(self.side_key(
                "device-volume-down",
                "Volume down",
                "AudioVolumeDown",
                gh * 0.395,
                gh * 0.07,
                cx,
            ));
        let nav = div()
            .id("device-nav")
            .role(Role::Toolbar)
            .aria_label("Navigation keys")
            .flex()
            .items_center()
            .justify_center()
            .gap(px(24.))
            .h(px(NAV))
            .children(
                [
                    ("device-back", "Back", "caret-left", "GoBack"),
                    ("device-home", "Home", "circle", "GoHome"),
                    ("device-overview", "Overview", "square", "AppSwitch"),
                ]
                .map(|(id, label, glyph, key)| {
                    icon_button(id, label, glyph, 16., cx)
                        .tooltip(tooltip(label))
                        .on_click(cx.listener(move |this, _, _, _| this.key(key)))
                }),
            );
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .p(px(MARGIN))
            .child(measure)
            .child(device)
            .child(nav)
    }
}
