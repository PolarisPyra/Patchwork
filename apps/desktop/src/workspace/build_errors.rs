//! The build's errors over the window, in a development build a reloader such as Oxidation
//! started, from a failed build until the next good one.

// Rust guideline compliant 2026-10-09

use gpui::{Role, deferred, div, prelude::*, px, relative};
use patchwork_ui_theme::{Theme, font_mono, fs_sm, radius, radius_sm};
use patchwork_ui_components::button;
use patchwork_ui_theme::icon;

use super::window::Workspace;

impl Workspace {
    /// The panel of build errors, while the last build failed.
    pub(crate) fn build_errors_panel(
        &self,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let errors = self.build_errors.clone()?;
        let (hover, fg) = (t.bg_hover, t.text);
        Some(
            deferred(
                div()
                    .id("build-errors")
                    .absolute()
                    .top(px(48.))
                    .left(relative(0.1))
                    .right(relative(0.1))
                    .max_h(relative(0.6))
                    .flex()
                    .flex_col()
                    .rounded(px(radius()))
                    .border_1()
                    .border_color(t.danger)
                    .bg(t.bg_elevated)
                    .shadow(patchwork_ui_components::popup_shadow(t))
                    .role(Role::Alert)
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(px(8.))
                            .py(px(6.))
                            .pl(px(12.))
                            .pr(px(6.))
                            .border_b_1()
                            .border_color(t.border)
                            .text_color(t.danger)
                            .child(icon("warning", 14.))
                            .child(div().flex_1().text_color(t.text).child("Build failed"))
                            .child(
                                button("dismiss-build-errors", "Dismiss")
                                    .size(px(28.))
                                    .flex_none()
                                    .justify_center()
                                    .rounded(px(radius_sm()))
                                    .text_color(t.text_muted)
                                    .hover(move |s| s.bg(hover).text_color(fg))
                                    .child(icon("x", 14.))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.build_errors = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .id("build-errors-text")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .p(px(12.))
                            .font_family(font_mono())
                            .text_size(px(fs_sm()))
                            .text_color(t.text)
                            .whitespace_normal()
                            .children(errors.lines().map(|line| {
                                // An empty line keeps its height, so blocks stay apart.
                                div().min_h(px(fs_sm() * 1.4)).child(line.to_owned())
                            })),
                    ),
            )
            .with_priority(2),
        )
    }
}
