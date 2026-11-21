//! A web page drawn by the installed browser, running headless: its pictures as they stream in,
//! and the pointer, wheel and keys sent back to it.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{
    Bounds, Entity, FocusHandle, KeyDownEvent, Keystroke, MouseButton, Pixels, Point, RenderImage,
    ScrollDelta, ScrollEvent, Task, Window, prelude::*,
};
use patchwork_core::browser::{
    BrowserCommand, BrowserEvent, BrowserFrame, BrowserMessage, BrowserSender, BrowserSession,
    Modifiers, MouseAction, PointerButton, address, find_browser,
};

use crate::ui::text_area::{Submitted, TextArea};

/// The pixels a wheel scrolls for one of its lines.
const PIXELS_PER_LINE: f32 = 40.;

/// The most errors of one page kept; a page that throws in a loop would grow them forever.
const MOST_ERRORS: usize = 20;

/// The errors the shown page met changed.
pub struct PageErrorsChanged;

#[expect(
    clippy::struct_excessive_bools,
    reason = "independent flags the page reports, not a state machine"
)]
pub struct BrowserView {
    /// What it is about, for the screen reader: the artifact's title, or "Web page".
    pub(super) label: String,
    /// Whether it has an address bar, for a page the person browses rather than one Patchwork
    /// opened.
    pub(super) toolbar: bool,
    pub(super) url: String,
    pub(super) title: String,
    pub(super) loading: bool,
    pub(super) back: bool,
    pub(super) forward: bool,
    /// No browser is installed: the view asks for one instead.
    pub(super) missing: bool,
    pub(super) error: Option<String>,
    pub(super) image: Option<Arc<RenderImage>>,
    pub(super) address: Entity<TextArea>,
    pub(super) sender: Option<BrowserSender>,
    /// Where the page is drawn, which the pointer is placed in.
    pub(super) bounds: Option<Bounds<Pixels>>,
    /// The size last sent, in logical pixels and device pixels per one.
    sent: Option<(u32, u32, f32)>,
    /// The address to start the browser on once the view is first laid out, so the page is
    /// drawn at its real size from its first picture.
    waiting_start: Option<String>,
    /// The button held down over the page.
    held: Option<PointerButton>,
    pub(super) focus: FocusHandle,
    session: Option<Task<()>>,
    /// The errors the shown page met, the first first.
    errors: Vec<String>,
    /// Whether the page was last told the app's theme is dark.
    dark: Option<bool>,
}

impl gpui::EventEmitter<PageErrorsChanged> for BrowserView {}

impl BrowserView {
    /// A view that starts the browser on `url`. `toolbar` gives it an address bar.
    pub fn new(url: String, label: String, toolbar: bool, cx: &mut Context<Self>) -> Self {
        let address = cx.new(|cx| {
            let mut field = TextArea::new("Address", "Address", cx);
            field.single_line = true;
            field.placeholder_color = Some(crate::ui::theme::input_placeholder());
            field.set_text(&url, cx);
            field
        });
        cx.subscribe(&address, |this, field, _: &Submitted, cx| {
            let typed = field.read(cx).content.trim().to_owned();
            if !typed.is_empty() {
                this.open(&typed, cx);
            }
        })
        .detach();
        let mut view = Self {
            label,
            toolbar,
            url: url.clone(),
            title: String::new(),
            loading: true,
            back: false,
            forward: false,
            missing: false,
            error: None,
            image: None,
            address,
            sender: None,
            bounds: None,
            sent: None,
            waiting_start: Some(url),
            held: None,
            focus: cx.focus_handle(),
            session: None,
            errors: Vec::new(),
            dark: None,
        };
        view.missing = find_browser().is_none();
        if view.missing {
            view.waiting_start = None;
            view.loading = false;
        }
        view
    }

    /// Starts the browser on `url`, or notes that there is none to start.
    pub(super) fn start(&mut self, url: String, cx: &mut Context<Self>) {
        self.missing = find_browser().is_none();
        self.error = None;
        if self.missing {
            self.loading = false;
            cx.notify();
            return;
        }
        let Some(size) = self.sent.filter(|_| self.bounds.is_some()) else {
            self.waiting_start = Some(url);
            self.loading = true;
            cx.notify();
            return;
        };
        self.loading = true;
        self.waiting_start = None;
        let (width, height, scale) = size;
        let starting =
            cx.background_spawn(async move { BrowserSession::start(&url, width, height, scale) });
        self.session = Some(cx.spawn(async move |this, cx| {
            let mut session = match starting.await {
                Ok(session) => session,
                Err(e) => {
                    let _ = this.update(cx, |this, cx| {
                        this.error = Some(e.to_string());
                        this.loading = false;
                        cx.notify();
                    });
                    return;
                }
            };
            let sender = session.sender();
            if this
                .update(cx, |this, cx| {
                    this.sender = Some(sender);
                    // A new browser takes the theme again from the next render.
                    this.dark = None;
                    cx.notify();
                })
                .is_err()
            {
                return;
            }
            while let Some(message) = session.next().await {
                if this.update(cx, |this, cx| this.take(message, cx)).is_err() {
                    return;
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.sender = None;
                this.loading = false;
                this.error = Some("The browser closed.".into());
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Loads what the person typed, or another page Patchwork opens.
    pub fn open(&mut self, typed: &str, cx: &mut Context<Self>) {
        let url = address(typed);
        if self.sender.is_none() {
            self.start(url, cx);
            return;
        }
        self.send(BrowserCommand::Open { url });
    }

    /// Names the page for the screen reader, when the view goes on to show another.
    pub fn relabel(&mut self, label: String) {
        self.label = label;
    }

    /// The errors the shown page met, the first first.
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Tells the page whether the app's theme is dark, when that changed.
    pub(super) fn theme(&mut self, dark: bool) {
        if self.sender.is_some() && self.dark != Some(dark) {
            self.dark = Some(dark);
            self.send(BrowserCommand::Theme { dark });
        }
    }

    pub(super) fn send(&self, command: BrowserCommand) {
        if let Some(sender) = &self.sender {
            sender.send(command);
        }
    }

    fn take(&mut self, message: BrowserMessage, cx: &mut Context<Self>) {
        match message {
            BrowserMessage::Frame(frame) => self.show(frame, cx),
            BrowserMessage::Event(BrowserEvent::Address { url }) => {
                self.address
                    .update(cx, |field, cx| field.set_text(&url, cx));
                self.url = url;
            }
            BrowserMessage::Event(BrowserEvent::Title { title }) => self.title = title,
            BrowserMessage::Event(BrowserEvent::Loading {
                loading,
                back,
                forward,
            }) => {
                // A page that starts loading starts without errors.
                if loading && !self.loading && !self.errors.is_empty() {
                    self.errors.clear();
                    cx.emit(PageErrorsChanged);
                }
                self.loading = loading;
                self.back = back;
                self.forward = forward;
            }
            BrowserMessage::Event(BrowserEvent::PageError { message }) => {
                if self.errors.len() < MOST_ERRORS && !self.errors.contains(&message) {
                    self.errors.push(message);
                    cx.emit(PageErrorsChanged);
                }
                return;
            }
        }
        cx.notify();
    }

    /// Takes the newest picture in place of the one shown.
    fn show(&mut self, frame: BrowserFrame, cx: &mut Context<Self>) {
        // The browser can send one picture at its own default size before it takes the view's;
        // shown first, it would jump to the right size a frame later.
        if self.image.is_none() && !self.sent.is_some_and(|size| fits(&frame, size)) {
            return;
        }
        let Some(buffer) = image::RgbaImage::from_raw(frame.width, frame.height, frame.bgra) else {
            return;
        };
        let image = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
        if let Some(old) = self.image.replace(image) {
            cx.drop_image(old, None);
        }
        self.error = None;
    }

    /// Tells the page the size it is drawn at, when that changed, and starts the browser once
    /// the size is first known. Returns whether the bounds moved.
    pub(super) fn fit(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let moved = self.bounds != Some(bounds);
        self.bounds = Some(bounds);
        let size = (
            logical(bounds.size.width),
            logical(bounds.size.height),
            window.scale_factor(),
        );
        if self.sent == Some(size) || size.0 == 0 || size.1 == 0 {
            return moved;
        }
        self.sent = Some(size);
        if let Some(url) = self.waiting_start.take() {
            self.start(url, cx);
        } else {
            self.send(BrowserCommand::Resize {
                width: size.0,
                height: size.1,
                scale: size.2,
            });
        }
        moved
    }

    /// `position` in the page's own pixels.
    fn place(&self, position: Point<Pixels>) -> Option<(f32, f32)> {
        let bounds = self.bounds?;
        Some((
            f32::from(position.x - bounds.origin.x),
            f32::from(position.y - bounds.origin.y),
        ))
    }

    pub(super) fn mouse(
        &mut self,
        position: Point<Pixels>,
        action: MouseAction,
        button: Option<MouseButton>,
        clicks: usize,
        modifiers: gpui::Modifiers,
    ) {
        let Some((x, y)) = self.place(position) else {
            return;
        };
        let button = match button {
            Some(MouseButton::Right) => PointerButton::Right,
            Some(MouseButton::Middle) => PointerButton::Middle,
            _ => PointerButton::Left,
        };
        match action {
            MouseAction::Down => self.held = Some(button),
            MouseAction::Up if self.held.take().is_none() => return,
            _ => {}
        }
        self.send(BrowserCommand::Mouse {
            x,
            y,
            action,
            button,
            clicks: u32::try_from(clicks).unwrap_or(1),
            modifiers: held(modifiers),
        });
    }

    pub(super) fn wheel(&self, event: &ScrollEvent) {
        let Some((x, y)) = self.place(event.position) else {
            return;
        };
        let (dx, dy) = match event.delta {
            ScrollDelta::Lines(lines) => (lines.x * PIXELS_PER_LINE, lines.y * PIXELS_PER_LINE),
            ScrollDelta::Pixels(pixels) => (f32::from(pixels.x), f32::from(pixels.y)),
        };
        // The app's deltas are positive towards the top; the page's towards the bottom.
        self.send(BrowserCommand::Wheel {
            x,
            y,
            dx: -dx,
            dy: -dy,
            modifiers: held(event.modifiers),
        });
    }

    /// Sends a key to the page. Paste sends the app's clipboard as typed text, since the headless
    /// browser has a clipboard of its own. Returns whether the page took it.
    pub(super) fn key(&self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let Keystroke {
            key,
            key_char,
            modifiers,
            ..
        } = &event.keystroke;
        if self.sender.is_none() {
            return false;
        }
        if (modifiers.control || modifiers.platform) && key == "v" && !modifiers.shift {
            if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                self.send(BrowserCommand::Text { text });
            }
            return true;
        }
        self.send(BrowserCommand::Key {
            key: key.clone(),
            text: key_char.clone(),
            modifiers: held(*modifiers),
        });
        true
    }

    pub(super) fn reload_or_stop(&mut self, cx: &mut Context<Self>) {
        if self.sender.is_none() {
            let url = self.url.clone();
            self.start(url, cx);
        } else if self.loading {
            self.send(BrowserCommand::Stop);
        } else {
            self.send(BrowserCommand::Reload);
        }
    }
}

/// The app's modifier keys as the page's.
fn held(m: gpui::Modifiers) -> Modifiers {
    Modifiers {
        shift: m.shift,
        control: m.control,
        alt: m.alt,
        meta: m.platform,
    }
}

/// Whether `frame` was drawn at `size`, in logical pixels and device pixels per one, give or
/// take the device pixel a scale rounds off.
fn fits(frame: &BrowserFrame, (width, height, scale): (u32, u32, f32)) -> bool {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a page a few thousand pixels across"
    )]
    let near = |pixels: u32, logical: u32| (pixels as f32 - logical as f32 * scale).abs() <= 1.;
    near(frame.width, width) && near(frame.height, height)
}

/// A length in whole logical pixels.
fn logical(length: Pixels) -> u32 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a page a few thousand pixels across, never negative"
    )]
    let whole = f32::from(length).max(0.).floor() as u32;
    whole
}

impl gpui::Focusable for BrowserView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}
