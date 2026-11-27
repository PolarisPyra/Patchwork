//! The window during first-run setup: only the setup, in the app's font, until
//! the settings say it is done. Then the workspace comes back with the composer focused.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use gpui::{AnyElement, Focusable, Window, div, prelude::*, px, relative};

use crate::workspace::Workspace;
use patchwork_desktop_onboarding::{Onboarding, OnboardingEvent};
use patchwork_ui_theme::{font_ui, fs_md, theme};

impl Workspace {
    pub(crate) fn onboarding_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let onboarding = if let Some(onboarding) = &self.onboarding {
            onboarding.clone()
        } else {
            let core = Arc::clone(&self.core);
            let onboarding = cx.new(|cx| Onboarding::new(core, window, cx));
            cx.subscribe_in(
                &onboarding,
                window,
                |this, onboarding, event, window, cx| match event {
                    OnboardingEvent::Changed => {
                        this.settings = this.core.settings();
                        // Setup may have added projects and imported chats.
                        this.reload(cx);
                        // A CLI installed during setup is missing from what was read at launch,
                        // such as each agent's models.
                        if this.settings.onboarded && onboarding.read(cx).restarts() {
                            crate::workspace::updates::restart(cx);
                        } else if this.settings.onboarded {
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
            .font_family(font_ui())
            .text_size(px(fs_md()))
            .line_height(relative(1.5))
            .text_color(t.text)
            .bg(t.bg_app)
            .child(onboarding)
            .into_any_element()
    }
}
