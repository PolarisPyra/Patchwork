//! The activity entry point and the app's root view.

// Rust guideline compliant 2026-10-09

use std::rc::Rc;

use gpui::{
    App, AppContext as _, Application, Context, IntoElement, ParentElement as _, Render,
    Styled as _, Window, WindowOptions, div, px, rgb,
};
use gpui_android::android_activity::AndroidApp;

/// The screen shown until the app is paired with a Patchwork.
struct Home;

impl Render for Home {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<'_, Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(12.0))
            .bg(rgb(0x0011_1113))
            .text_color(rgb(0x00ed_edef))
            .child(div().text_size(px(28.0)).child("Patchwork"))
            .child(
                div()
                    .text_size(px(15.0))
                    .text_color(rgb(0x00a1_a1aa))
                    .child("Not connected"),
            )
    }
}

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
            if let Err(error) =
                cx.open_window(WindowOptions::default(), |_window, cx| cx.new(|_| Home))
            {
                log::error!("failed to open the window: {error:#}");
            }
        },
    );
}
