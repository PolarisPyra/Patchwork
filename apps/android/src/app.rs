//! The activity entry point.

// Rust guideline compliant 2026-11-27

use std::rc::Rc;

use gpui::{App, AppContext as _, Application, WindowOptions};
use gpui_android::android_activity::AndroidApp;
use patchwork_client::Session;
use patchwork_themes::{Applied, Paint, Palette};

use crate::nav::Nav;

/// Runs the app. `android-activity` calls this on its own thread once the activity starts.
#[unsafe(no_mangle)]
pub extern "Rust" fn android_main(app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_tag("Patchwork")
            .with_max_level(log::LevelFilter::Info),
    );
    gpui_android::init(app);
    Application::with_platform(Rc::new(gpui_android::AndroidPlatform::new())).run(
        |cx: &mut App| {
            if let Err(error) = cx.text_system().add_fonts(patchwork_design_theme::fonts()) {
                log::error!("failed to load the fonts: {error:#}");
            }
            Session::init(cx);
            let opened = cx.open_window(WindowOptions::default(), |window, cx| {
                // The desktop's palettes, following the phone's light or dark mode.
                let applied = Applied {
                    paint: Paint::System {
                        light: Palette::Light,
                        dark: Palette::Dark,
                    },
                    ..Applied::default()
                };
                patchwork_design_theme::apply(applied, window.appearance(), cx);
                cx.new(Nav::new)
            });
            if let Err(error) = opened {
                log::error!("failed to open the window: {error:#}");
            }
        },
    );
}
