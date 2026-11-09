//! The window during first-run setup: only the setup, in the app's font, until
//! the settings say it is done. Then the workspace comes back with the composer focused.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use gpui::{AnyElement, Focusable, Window, div, prelude::*, px, relative};

use super::Workspace;
use crate::onboarding::{Onboarding, OnboardingEvent};
use crate::ui::theme::{FONT_UI, FS_MD, theme};

impl Workspace {
    pub(super) fn onboarding_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let onboarding = if let Some(onboarding) = &self.onboarding {
            onboarding.clone()
        } else {
            let server = Arc::clone(&self.server);
            let onboarding = cx.new(|cx| Onboarding::new(server, window, cx));
            cx.subscribe_in(
                &onboarding,
                window,
                |this, _, event, window, cx| match event {
                    OnboardingEvent::Changed => {
                        this.settings = this.server.store.settings();
                        // Setup may have added projects and imported threads.
                        this.reload(cx);
                        if this.settings.onboarded {
                            this.onboarding = None;
                            let composer = this.composer.read(cx).editor.focus_handle(cx);
                            window.focus(&composer, cx);
                        }
                    }
                    OnboardingEvent::ToggleTheme => this.toggle_theme(window, cx),
                },
            )
            .detach();
            self.onboarding = Some(onboarding.clone());
            onboarding
        };
        let t = *theme(cx);
        div()
            .size_full()
            .font_family(FONT_UI)
            .text_size(px(FS_MD))
            .line_height(relative(1.5))
            .text_color(t.text)
            .bg(t.bg_app)
            .child(onboarding)
            .into_any_element()
    }
}
