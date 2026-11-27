//! The activity's window: a full-screen GPUI window drawn with wgpu onto the `ANativeWindow`.

// Rust guideline compliant 2026-10-09

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use futures::channel::oneshot;
use gpui::{
    AnyWindowHandle, Bounds, Capslock, DevicePixels, DispatchEventResult, Edges, GpuSpecs,
    Modifiers, Pixels, PlatformAtlas, PlatformDisplay, PlatformInput, PlatformInputHandler,
    PlatformWindow, Point, PromptButton, PromptLevel, RequestFrameOptions, Scene, Size,
    WindowAppearance, WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowInsets,
    WindowVisibility, size,
};
use gpui_wgpu::{GpuContext, WgpuRenderer, WgpuSurfaceConfig, wgpu};
use ndk::native_window::NativeWindow;
use raw_window_handle::{DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle};

use super::display::AndroidDisplay;
use super::{CallbackSlot, app, input};

/// A borrowed `ANativeWindow` that wgpu can make a surface from.
#[derive(Clone, Debug)]
struct Surface(NativeWindow);

impl HasWindowHandle for Surface {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        self.0.window_handle()
    }
}

impl HasDisplayHandle for Surface {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::android())
    }
}

/// State shared by the GPUI-owned [`AndroidWindow`] and the platform's event loop.
pub(crate) struct WindowState {
    renderer: RefCell<WgpuRenderer>,
    gpu_context: GpuContext,
    native: RefCell<Option<NativeWindow>>,
    atlas: Arc<dyn PlatformAtlas>,
    size: Cell<Size<Pixels>>,
    scale: Cell<f32>,
    insets: RefCell<WindowInsets>,
    active: Cell<bool>,
    visibility: Cell<WindowVisibility>,
    modifiers: Cell<Modifiers>,
    last_touch: Cell<Point<Pixels>>,
    touch_base: Cell<u64>,
    dirty: Cell<bool>,
    input_handler: CallbackSlot<PlatformInputHandler>,
    request_frame: CallbackSlot<Box<dyn FnMut(RequestFrameOptions)>>,
    input: CallbackSlot<Box<dyn FnMut(PlatformInput) -> DispatchEventResult>>,
    active_status: CallbackSlot<Box<dyn FnMut(bool)>>,
    visibility_change: CallbackSlot<Box<dyn FnMut(WindowVisibility)>>,
    resize: CallbackSlot<Box<dyn FnMut(Size<Pixels>, f32)>>,
    appearance_changed: CallbackSlot<Box<dyn FnMut()>>,
    insets_changed: CallbackSlot<Box<dyn FnMut(WindowInsets)>>,
    back: CallbackSlot<Box<dyn FnMut()>>,
    back_enabled: Cell<bool>,
    close: CallbackSlot<Box<dyn FnOnce()>>,
}

/// GPUI's handle on the activity's window.
pub(crate) struct AndroidWindow(Rc<WindowState>);

/// Returns the density scale: Android's `density / 160`.
fn scale_factor() -> f32 {
    app()
        .config()
        .density()
        .map_or(1.0, |density| density as f32 / 160.0)
}

fn present_mode() -> Option<wgpu::PresentMode> {
    // Mailbox keeps `get_current_texture` from blocking while the activity is being paused.
    Some(wgpu::PresentMode::Mailbox)
}

fn physical_size(window: &NativeWindow) -> Size<DevicePixels> {
    size(
        DevicePixels(window.width().max(1)),
        DevicePixels(window.height().max(1)),
    )
}

impl AndroidWindow {
    /// Creates the window on the activity's current `ANativeWindow`.
    pub(crate) fn new(_handle: AnyWindowHandle, gpu_context: GpuContext) -> anyhow::Result<Self> {
        let native = app()
            .native_window()
            .ok_or_else(|| anyhow::anyhow!("the activity has no window yet"))?;
        let physical = physical_size(&native);
        let renderer = WgpuRenderer::new(
            gpu_context.clone(),
            &Surface(native.clone()),
            WgpuSurfaceConfig {
                size: physical,
                transparent: false,
                preferred_present_mode: present_mode(),
            },
            None,
        )?;
        let atlas: Arc<dyn PlatformAtlas> = renderer.sprite_atlas().clone();
        let scale = scale_factor();
        let state = Rc::new(WindowState {
            renderer: RefCell::new(renderer),
            gpu_context,
            native: RefCell::new(Some(native)),
            atlas,
            size: Cell::new(logical_size(physical, scale)),
            scale: Cell::new(scale),
            insets: RefCell::new(WindowInsets::default()),
            active: Cell::new(true),
            visibility: Cell::new(WindowVisibility::Visible),
            modifiers: Cell::new(Modifiers::default()),
            last_touch: Cell::new(Point::default()),
            touch_base: Cell::new(0),
            dirty: Cell::new(true),
            input_handler: CallbackSlot::default(),
            request_frame: CallbackSlot::default(),
            input: CallbackSlot::default(),
            active_status: CallbackSlot::default(),
            visibility_change: CallbackSlot::default(),
            resize: CallbackSlot::default(),
            appearance_changed: CallbackSlot::default(),
            insets_changed: CallbackSlot::default(),
            back: CallbackSlot::default(),
            back_enabled: Cell::new(false),
            close: CallbackSlot::default(),
        });
        state.update_insets();
        Ok(Self(state))
    }

    pub(crate) fn state(&self) -> Weak<WindowState> {
        Rc::downgrade(&self.0)
    }
}

fn logical_size(physical: Size<DevicePixels>, scale: f32) -> Size<Pixels> {
    size(
        input::logical(physical.width.0, scale),
        input::logical(physical.height.0, scale),
    )
}

impl WindowState {
    /// Asks GPUI for a frame. GPUI redraws only when something changed unless `force` is set.
    pub(crate) fn request_frame(&self) {
        if self.native.borrow().is_none() {
            return;
        }
        let force_render = self.dirty.replace(false);
        self.request_frame.with(|callback| {
            callback(RequestFrameOptions {
                require_presentation: force_render,
                force_render,
                ..RequestFrameOptions::default()
            });
        });
    }

    /// Points the renderer at a new `ANativeWindow` after the activity returns.
    pub(crate) fn attach(&self, native: NativeWindow) {
        let physical = physical_size(&native);
        let instance = self
            .gpu_context
            .borrow()
            .as_ref()
            .map(|context| context.instance.clone());
        let Some(instance) = instance else {
            log::error!("the GPU context is gone; cannot attach the new window");
            return;
        };
        let replaced = self.renderer.borrow_mut().replace_surface(
            &Surface(native.clone()),
            WgpuSurfaceConfig {
                size: physical,
                transparent: false,
                preferred_present_mode: present_mode(),
            },
            &instance,
        );
        if let Err(error) = replaced {
            log::error!("failed to attach the renderer to the new window: {error:#}");
            return;
        }
        self.native.replace(Some(native));
        self.set_visibility(WindowVisibility::Visible);
        self.resized();
    }

    /// Releases the surface before Android destroys the `ANativeWindow` under it.
    pub(crate) fn detach(&self) {
        self.renderer.borrow_mut().unconfigure_surface();
        self.native.replace(None);
        self.set_visibility(WindowVisibility::Hidden);
    }

    /// Re-reads the window size and density after a resize or configuration change.
    pub(crate) fn resized(&self) {
        let Some(physical) = self.native.borrow().as_ref().map(physical_size) else {
            return;
        };
        let scale = scale_factor();
        let logical = logical_size(physical, scale);
        let changed = logical != self.size.get() || scale != self.scale.get();
        self.renderer.borrow_mut().update_drawable_size(physical);
        self.size.set(logical);
        self.scale.set(scale);
        self.dirty.set(true);
        if changed {
            self.resize.with(|callback| callback(logical, scale));
        }
        self.update_insets();
    }

    /// Recomputes the safe area from the activity's content rectangle.
    pub(crate) fn update_insets(&self) {
        let Some(physical) = self.native.borrow().as_ref().map(physical_size) else {
            return;
        };
        let scale = self.scale.get();
        let content = app().content_rect();
        let mut insets = self.insets.borrow().clone();
        // An empty rectangle means Android has not reported one yet.
        insets.safe_area = if content.right <= content.left || content.bottom <= content.top {
            Edges::default()
        } else {
            Edges {
                top: input::logical(content.top.max(0), scale),
                left: input::logical(content.left.max(0), scale),
                right: input::logical((physical.width.0 - content.right).max(0), scale),
                bottom: input::logical((physical.height.0 - content.bottom).max(0), scale),
            }
        };
        if *self.insets.borrow() != insets {
            self.insets.replace(insets.clone());
            self.dirty.set(true);
            self.insets_changed.with(|callback| callback(insets));
        }
    }

    pub(crate) fn set_active(&self, active: bool) {
        if self.active.replace(active) != active {
            self.active_status.with(|callback| callback(active));
        }
    }

    fn set_visibility(&self, visibility: WindowVisibility) {
        if self.visibility.replace(visibility) != visibility {
            self.visibility_change.with(|callback| callback(visibility));
        }
    }

    pub(crate) fn appearance_changed(&self) {
        self.dirty.set(true);
        self.appearance_changed.with(|callback| callback());
    }

    /// Sends one input event to GPUI, typing its text into the focused field if nothing took it.
    pub(crate) fn dispatch(&self, input: PlatformInput) -> bool {
        let result = self.input.with(|callback| callback(input.clone()));
        let propagate = result.is_none_or(|result| result.propagate);
        if !propagate {
            return true;
        }
        if let PlatformInput::KeyDown(event) = &input
            && event.keystroke.modifiers.is_subset_of(&Modifiers::shift())
            && let Some(text) = &event.keystroke.key_char
        {
            return self
                .input_handler
                .with(|handler| handler.replace_text_in_range(None, text))
                .is_some();
        }
        false
    }

    /// Handles one motion event, returning whether GPUI used it.
    pub(crate) fn handle_motion(&self, event: &android_activity::input::MotionEvent<'_>) -> bool {
        use android_activity::input::MotionAction;
        if matches!(event.action(), MotionAction::Down) {
            // Android reuses pointer ids from 0 in every gesture; GPUI needs fresh ids.
            self.touch_base
                .set(self.touch_base.get().wrapping_add(1 << 32));
        }
        self.modifiers.set(input::modifiers(event.meta_state()));
        let touches = input::touches(event, self.scale.get(), self.touch_base.get());
        let mut handled = false;
        for touch in touches {
            self.last_touch.set(touch.position);
            handled |= self.dispatch(PlatformInput::Touch(touch));
        }
        handled
    }

    /// Handles the system back key or gesture, returning whether the app took it.
    pub(crate) fn handle_back(&self) -> bool {
        self.back_enabled.get() && self.back.with(|callback| callback()).is_some()
    }

    pub(crate) fn set_modifiers(&self, modifiers: Modifiers) {
        self.modifiers.set(modifiers);
    }

    pub(crate) fn mark_dirty(&self) {
        self.dirty.set(true);
    }

    pub(crate) fn close(&self) {
        if let Some(callback) = self.close.take() {
            callback();
        }
    }
}

impl HasWindowHandle for AndroidWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let native = self.0.native.borrow();
        let raw = native
            .as_ref()
            .ok_or(HandleError::Unavailable)?
            .window_handle()?
            .as_raw();
        // SAFETY: the handle stays valid until Android terminates the window, which GPUI hears
        // about through `detach` before the pointer goes stale.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

impl HasDisplayHandle for AndroidWindow {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::android())
    }
}

impl PlatformWindow for AndroidWindow {
    fn bounds(&self) -> Bounds<Pixels> {
        Bounds::new(Point::default(), self.0.size.get())
    }

    fn is_maximized(&self) -> bool {
        true
    }

    fn window_bounds(&self) -> WindowBounds {
        WindowBounds::Fullscreen(self.bounds())
    }

    fn content_size(&self) -> Size<Pixels> {
        self.0.size.get()
    }

    fn resize(&mut self, _size: Size<Pixels>) {}

    fn scale_factor(&self) -> f32 {
        self.0.scale.get()
    }

    fn appearance(&self) -> WindowAppearance {
        super::platform::appearance()
    }

    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(Rc::new(AndroidDisplay {
            size: self.0.size.get(),
        }))
    }

    fn mouse_position(&self) -> Point<Pixels> {
        self.0.last_touch.get()
    }

    fn modifiers(&self) -> Modifiers {
        self.0.modifiers.get()
    }

    fn capslock(&self) -> Capslock {
        Capslock { on: false }
    }

    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        self.0.input_handler.set(input_handler);
    }

    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.0.input_handler.take()
    }

    fn prompt(
        &self,
        _level: PromptLevel,
        _msg: &str,
        _detail: Option<&str>,
        _answers: &[PromptButton],
    ) -> Option<oneshot::Receiver<usize>> {
        // GPUI draws its own prompt when the platform has none.
        None
    }

    fn activate(&self) {}

    fn is_active(&self) -> bool {
        self.0.active.get()
    }

    fn visibility(&self) -> WindowVisibility {
        self.0.visibility.get()
    }

    fn is_hovered(&self) -> bool {
        false
    }

    fn background_appearance(&self) -> WindowBackgroundAppearance {
        WindowBackgroundAppearance::Opaque
    }

    fn set_title(&mut self, _title: &str) {}

    fn set_background_appearance(&self, _background_appearance: WindowBackgroundAppearance) {}

    fn minimize(&self) {}

    fn zoom(&self) {}

    fn toggle_fullscreen(&self) {}

    fn is_fullscreen(&self) -> bool {
        true
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
        self.0.request_frame.set(callback);
    }

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
        self.0.input.set(callback);
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0.active_status.set(callback);
    }

    fn on_visibility_change(&self, callback: Box<dyn FnMut(WindowVisibility)>) {
        self.0.visibility_change.set(callback);
    }

    fn on_hover_status_change(&self, _callback: Box<dyn FnMut(bool)>) {}

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) {
        self.0.resize.set(callback);
    }

    fn on_moved(&self, _callback: Box<dyn FnMut()>) {}

    fn on_display_changed(&self, _callback: Box<dyn FnMut()>) {}

    fn on_should_close(&self, _callback: Box<dyn FnMut() -> bool>) {}

    fn on_hit_test_window_control(&self, _callback: Box<dyn FnMut() -> Option<WindowControlArea>>) {
    }

    fn on_close(&self, callback: Box<dyn FnOnce()>) {
        self.0.close.set(callback);
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut()>) {
        self.0.appearance_changed.set(callback);
    }

    fn draw(&self, scene: &Scene) {
        if self.0.native.borrow().is_some() && !self.0.renderer.borrow_mut().draw(scene) {
            // The frame was dropped (surface lost or outdated); draw the next one in full.
            self.0.dirty.set(true);
        }
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.0.atlas.clone()
    }

    fn is_subpixel_rendering_supported(&self) -> bool {
        false
    }

    fn gpu_specs(&self) -> Option<GpuSpecs> {
        self.0.renderer.borrow().gpu_specs()
    }

    fn update_ime_position(&self, _bounds: Bounds<Pixels>) {}

    fn insets(&self) -> WindowInsets {
        self.0.insets.borrow().clone()
    }

    fn on_insets_changed(&self, callback: Box<dyn FnMut(WindowInsets)>) {
        self.0.insets_changed.set(callback);
    }

    fn set_back_handler(&self, callback: Box<dyn FnMut()>) {
        self.0.back.set(callback);
    }

    fn set_back_enabled(&self, enabled: bool) {
        self.0.back_enabled.set(enabled);
    }

    fn show_soft_keyboard(&self) {
        app().show_soft_input(true);
    }

    fn hide_soft_keyboard(&self) {
        app().hide_soft_input(false);
    }
}

impl Drop for AndroidWindow {
    fn drop(&mut self) {
        self.0.renderer.borrow_mut().destroy();
    }
}
