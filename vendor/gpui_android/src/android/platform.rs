//! The GPUI platform for an Android activity: its event loop, lifecycle and system services.

// Rust guideline compliant 2026-10-09

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::{Duration, Instant};

use android_activity::input::{InputEvent, KeyAction, Keycode};
use android_activity::{InputStatus, MainEvent, PollEvent};
use futures::channel::oneshot;
use gpui::{
    Action, AnyWindowHandle, AppLifecyclePhase, BackgroundExecutor, ClipboardItem, CursorStyle,
    DummyKeyboardMapper, ForegroundExecutor, Keymap, Menu, MenuItem, PathPromptOptions, Platform,
    PlatformDisplay, PlatformInfo, PlatformKeyboardLayout, PlatformKeyboardMapper,
    PlatformTextSystem, PlatformWindow, PriorityQueueReceiver, Result, RunnableVariant, Task,
    ThermalState, WindowAppearance, WindowParams,
};
use gpui_wgpu::{CosmicTextSystem, GpuContext};
use ndk::configuration::UiModeNight;

use super::dispatcher::{self, AndroidDispatcher};
use super::display::AndroidDisplay;
use super::window::{AndroidWindow, WindowState};
use super::{CallbackSlot, app, input};

/// How long the loop waits between frames when nothing else wakes it: one 60 Hz frame.
const FRAME_INTERVAL: Duration = Duration::from_micros(16_667);

/// The most main-thread tasks run between two looper polls, so input and frames keep flowing.
const MAX_TASKS_PER_TURN: usize = 256;

/// Where Android keeps its system fonts.
const SYSTEM_FONTS: &str = "/system/fonts";

/// GPUI's platform for an Android activity.
pub struct AndroidPlatform(Rc<State>);

struct State {
    background_executor: BackgroundExecutor,
    foreground_executor: ForegroundExecutor,
    main_receiver: RefCell<PriorityQueueReceiver<RunnableVariant>>,
    text_system: Arc<CosmicTextSystem>,
    gpu_context: GpuContext,
    window: RefCell<Weak<WindowState>>,
    window_handle: Cell<Option<AnyWindowHandle>>,
    quit_requested: Cell<bool>,
    clipboard: RefCell<Option<ClipboardItem>>,
    lifecycle: CallbackSlot<Box<dyn FnMut(AppLifecyclePhase)>>,
    memory_warning: CallbackSlot<Box<dyn FnMut()>>,
    quit: CallbackSlot<Box<dyn FnMut() -> bool>>,
}

impl AndroidPlatform {
    /// Creates the platform. Call it on the thread that runs `android_main`.
    ///
    /// # Panics
    ///
    /// Panics if `gpui_android::init` has not been called.
    #[must_use]
    pub fn new() -> Self {
        let (main_sender, main_receiver) = PriorityQueueReceiver::new();
        let dispatcher = Arc::new(AndroidDispatcher::new(main_sender, app().create_waker()));
        let text_system = Arc::new(CosmicTextSystem::new_without_system_fonts("Roboto"));
        if let Err(error) = text_system.add_fonts(system_fonts()) {
            log::error!("failed to load the system fonts: {error:#}");
        }
        Self(Rc::new(State {
            background_executor: BackgroundExecutor::new(dispatcher.clone()),
            foreground_executor: ForegroundExecutor::new(dispatcher),
            main_receiver: RefCell::new(main_receiver),
            text_system,
            gpu_context: Rc::new(RefCell::new(None)),
            window: RefCell::new(Weak::new()),
            window_handle: Cell::new(None),
            quit_requested: Cell::new(false),
            clipboard: RefCell::new(None),
            lifecycle: CallbackSlot::default(),
            memory_warning: CallbackSlot::default(),
            quit: CallbackSlot::default(),
        }))
    }
}

impl Default for AndroidPlatform {
    fn default() -> Self {
        Self::new()
    }
}

/// Reads Roboto and the emoji font from the system font folder.
fn system_fonts() -> Vec<Cow<'static, [u8]>> {
    let Ok(entries) = std::fs::read_dir(SYSTEM_FONTS) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    (name.starts_with("Roboto") || name.starts_with("NotoColorEmoji"))
                        && name.ends_with(".ttf")
                })
        })
        .filter_map(|path| std::fs::read(path).ok().map(Cow::Owned))
        .collect()
}

/// Reads whether the system is in dark mode.
pub(crate) fn appearance() -> WindowAppearance {
    match app().config().ui_mode_night() {
        UiModeNight::Yes => WindowAppearance::Dark,
        _ => WindowAppearance::Light,
    }
}

impl State {
    fn window(&self) -> Option<Rc<WindowState>> {
        self.window.borrow().upgrade()
    }

    fn set_phase(&self, phase: AppLifecyclePhase) {
        self.lifecycle.with(|callback| callback(phase));
    }

    /// Runs the main-thread tasks that are ready, up to [`MAX_TASKS_PER_TURN`].
    fn run_main_tasks(&self) {
        for _ in 0..MAX_TASKS_PER_TURN {
            let next = self.main_receiver.borrow_mut().try_pop();
            match next {
                Ok(Some(runnable)) => dispatcher::run(runnable),
                Ok(None) | Err(_) => return,
            }
        }
    }

    /// Handles one activity event. Returns `true` once the activity is destroyed.
    fn handle_main(&self, event: MainEvent<'_>, launch: &mut Option<Box<dyn FnOnce()>>) -> bool {
        match event {
            MainEvent::InitWindow { .. } => {
                if let Some(launch) = launch.take() {
                    launch();
                } else if let (Some(window), Some(native)) = (self.window(), app().native_window())
                {
                    window.attach(native);
                }
            }
            MainEvent::TerminateWindow { .. } => {
                if let Some(window) = self.window() {
                    window.detach();
                }
            }
            MainEvent::WindowResized { .. } | MainEvent::ConfigChanged { .. } => {
                if let Some(window) = self.window() {
                    window.resized();
                    window.appearance_changed();
                }
            }
            MainEvent::ContentRectChanged { .. } | MainEvent::InsetsChanged { .. } => {
                if let Some(window) = self.window() {
                    window.update_insets();
                }
            }
            MainEvent::RedrawNeeded { .. } => {
                if let Some(window) = self.window() {
                    window.mark_dirty();
                }
            }
            MainEvent::GainedFocus => {
                if let Some(window) = self.window() {
                    window.set_active(true);
                }
            }
            MainEvent::LostFocus => {
                if let Some(window) = self.window() {
                    window.set_active(false);
                }
            }
            MainEvent::Start => self.set_phase(AppLifecyclePhase::Foreground),
            MainEvent::Resume { .. } => self.set_phase(AppLifecyclePhase::Active),
            MainEvent::Pause => self.set_phase(AppLifecyclePhase::Inactive),
            MainEvent::Stop => self.set_phase(AppLifecyclePhase::Background),
            MainEvent::LowMemory => {
                self.memory_warning.with(|callback| callback());
            }
            MainEvent::Destroy => {
                self.set_phase(AppLifecyclePhase::Disconnected);
                self.quit.with(|callback| callback());
                if let Some(window) = self.window() {
                    window.close();
                }
                return true;
            }
            _ => {}
        }
        false
    }

    /// Sends the activity's pending touches and key presses to the window.
    fn handle_input(&self) {
        let app = app();
        let Ok(mut events) = app.input_events_iter() else {
            return;
        };
        while events.next(|event| self.handle_input_event(event)) {}
    }

    fn handle_input_event(&self, event: &InputEvent<'_>) -> InputStatus {
        let Some(window) = self.window() else {
            return InputStatus::Unhandled;
        };
        let handled = match event {
            InputEvent::MotionEvent(motion) => window.handle_motion(motion),
            InputEvent::KeyEvent(key) if key.key_code() == Keycode::Back => {
                // Answer the press and act on the release, as Android's own views do.
                match key.action() {
                    KeyAction::Up => window.handle_back(),
                    _ => true,
                }
            }
            InputEvent::KeyEvent(key) => {
                window.set_modifiers(input::modifiers(key.meta_state()));
                input::key(&app(), key).is_some_and(|input| window.dispatch(input))
            }
            _ => false,
        };
        if handled {
            InputStatus::Handled
        } else {
            InputStatus::Unhandled
        }
    }
}

struct AndroidKeyboardLayout;

impl PlatformKeyboardLayout for AndroidKeyboardLayout {
    fn id(&self) -> &str {
        "android"
    }

    fn name(&self) -> &str {
        "Android"
    }
}

impl Platform for AndroidPlatform {
    fn platform_info(&self) -> PlatformInfo {
        PlatformInfo {
            os: gpui::OperatingSystem::Android,
            target: gpui::PlatformTarget::Native,
        }
    }

    fn background_executor(&self) -> BackgroundExecutor {
        self.0.background_executor.clone()
    }

    fn foreground_executor(&self) -> ForegroundExecutor {
        self.0.foreground_executor.clone()
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.0.text_system.clone()
    }

    fn run(&self, on_finish_launching: Box<dyn 'static + FnOnce()>) {
        let state = &self.0;
        let mut launch = Some(on_finish_launching);
        let mut last_frame = Instant::now();
        loop {
            let timeout = if state.main_receiver.borrow().is_empty() {
                FRAME_INTERVAL.saturating_sub(last_frame.elapsed())
            } else {
                Duration::ZERO
            };
            let mut destroyed = false;
            app().poll_events(Some(timeout), |event| {
                if let PollEvent::Main(event) = event {
                    destroyed |= state.handle_main(event, &mut launch);
                }
            });
            if destroyed || state.quit_requested.get() {
                break;
            }
            state.handle_input();
            state.run_main_tasks();
            if last_frame.elapsed() >= FRAME_INTERVAL {
                last_frame = Instant::now();
                if let Some(window) = state.window() {
                    window.request_frame();
                }
            }
        }
    }

    fn quit(&self) {
        self.0.quit_requested.set(true);
    }

    fn restart(&self, _binary_path: Option<PathBuf>, _arguments: Vec<std::ffi::OsString>) {
        log::warn!("an Android app cannot restart itself");
    }

    fn activate(&self, _ignoring_other_apps: bool) {}

    fn hide(&self) {}

    fn hide_other_apps(&self) {}

    fn unhide_other_apps(&self) {}

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        self.primary_display().into_iter().collect()
    }

    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        let native = app().native_window()?;
        let scale = app()
            .config()
            .density()
            .map_or(1.0, |density| density as f32 / 160.0);
        Some(Rc::new(AndroidDisplay {
            size: gpui::size(
                input::logical(native.width(), scale),
                input::logical(native.height(), scale),
            ),
        }))
    }

    fn on_displays_changed(&self, _callback: Box<dyn FnMut()>) {}

    fn active_window(&self) -> Option<AnyWindowHandle> {
        self.0.window_handle.get()
    }

    fn open_window(
        &self,
        handle: AnyWindowHandle,
        _options: WindowParams,
    ) -> anyhow::Result<Box<dyn PlatformWindow>> {
        if self.0.window().is_some() {
            anyhow::bail!("an Android activity has a single window");
        }
        let window = AndroidWindow::new(handle, self.0.gpu_context.clone())?;
        self.0.window.replace(window.state());
        self.0.window_handle.set(Some(handle));
        Ok(Box::new(window))
    }

    fn window_appearance(&self) -> WindowAppearance {
        appearance()
    }

    fn open_url(&self, url: &str) {
        log::warn!("opening links is not supported on Android yet: {url}");
    }

    fn on_open_urls(&self, _callback: Box<dyn FnMut(Vec<String>)>) {}

    fn register_url_scheme(&self, _url: &str) -> Task<Result<()>> {
        // URL schemes are declared in AndroidManifest.xml.
        Task::ready(Ok(()))
    }

    fn prompt_for_paths(
        &self,
        _options: PathPromptOptions,
    ) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>> {
        let (sender, receiver) = oneshot::channel();
        sender
            .send(Err(anyhow::anyhow!(
                "file pickers are not supported on Android"
            )))
            .ok();
        receiver
    }

    fn prompt_for_new_path(
        &self,
        _directory: &Path,
        _suggested_name: Option<&str>,
    ) -> oneshot::Receiver<Result<Option<PathBuf>>> {
        let (sender, receiver) = oneshot::channel();
        sender
            .send(Err(anyhow::anyhow!(
                "save dialogs are not supported on Android"
            )))
            .ok();
        receiver
    }

    fn can_select_mixed_files_and_dirs(&self) -> bool {
        false
    }

    fn reveal_path(&self, _path: &Path) {}

    fn open_with_system(&self, _path: &Path) {}

    fn on_quit(&self, callback: Box<dyn FnMut() -> bool>) {
        self.0.quit.set(callback);
    }

    fn on_reopen(&self, _callback: Box<dyn FnMut()>) {}

    fn on_system_sleep(&self, _callback: Box<dyn FnMut()>) {}

    fn on_system_wake(&self, _callback: Box<dyn FnMut()>) {}

    fn on_app_lifecycle(&self, callback: Box<dyn FnMut(AppLifecyclePhase)>) {
        self.0.lifecycle.set(callback);
    }

    fn on_memory_warning(&self, callback: Box<dyn FnMut()>) {
        self.0.memory_warning.set(callback);
    }

    fn set_menus(&self, _menus: Vec<Menu>, _keymap: &Keymap) {}

    fn set_dock_menu(&self, _menu: Vec<MenuItem>, _keymap: &Keymap) {}

    fn on_app_menu_action(&self, _callback: Box<dyn FnMut(&dyn Action)>) {}

    fn on_will_open_app_menu(&self, _callback: Box<dyn FnMut()>) {}

    fn on_validate_app_menu_command(&self, _callback: Box<dyn FnMut(&dyn Action) -> bool>) {}

    fn thermal_state(&self) -> ThermalState {
        ThermalState::Nominal
    }

    fn on_thermal_state_change(&self, _callback: Box<dyn FnMut()>) {}

    fn prevent_idle_sleep(&self, _reason: &str) -> Task<Result<gpui::ActivityGuard>> {
        Task::ready(Err(anyhow::anyhow!(
            "keeping the screen on is not supported on Android yet"
        )))
    }

    fn app_path(&self) -> Result<PathBuf> {
        app()
            .internal_data_path()
            .ok_or_else(|| anyhow::anyhow!("the activity has no data folder"))
    }

    fn path_for_auxiliary_executable(&self, name: &str) -> Result<PathBuf> {
        anyhow::bail!("an Android app has no auxiliary executables: {name}")
    }

    fn set_cursor_style(&self, _style: CursorStyle) {}

    fn hide_cursor_until_mouse_moves(&self) {}

    fn is_cursor_visible(&self) -> bool {
        false
    }

    fn should_auto_hide_scrollbars(&self) -> bool {
        true
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        self.0.clipboard.borrow().clone()
    }

    fn write_to_clipboard(&self, item: ClipboardItem) {
        self.0.clipboard.replace(Some(item));
    }

    fn write_credentials(&self, _url: &str, _username: &str, _password: &[u8]) -> Task<Result<()>> {
        Task::ready(Err(anyhow::anyhow!(
            "credentials are not supported on Android yet"
        )))
    }

    fn read_credentials(&self, _url: &str) -> Task<Result<Option<(String, Vec<u8>)>>> {
        Task::ready(Ok(None))
    }

    fn delete_credentials(&self, _url: &str) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        Box::new(AndroidKeyboardLayout)
    }

    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        Rc::new(DummyKeyboardMapper)
    }

    fn on_keyboard_layout_change(&self, _callback: Box<dyn FnMut()>) {}
}
