//! Around every setup step: the header with the logo, the steps and the theme,
//! the step's title, and the footer that moves between steps.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, FontWeight, Hsla, Role, Window, div, img, linear_color_stop, linear_gradient,
    prelude::*, px, relative, rgb,
};

use super::{
    IMPORTS_STEP, LAST_STEP, LOGO_BLUE, LOGO_PINK, LOGO_VIOLET, Onboarding, OnboardingEvent,
    PROJECTS_STEP, STEPS, agent_step, imports_step, projects_step, ready_step,
};
use crate::ui::theme::{FS_LG, FS_SM, FS_XL, FS_XXL, RADIUS_LG, RADIUS_SM, Theme, theme};
use crate::ui::{BtnKind, btn, icon, icon_button, spinner, tooltip};

/// The logo's colours as a fill, from `a` to `b`.
fn logo_fill(angle: f32, a: u32, b: u32) -> gpui::Background {
    let (a, b): (Hsla, Hsla) = (rgb(a).into(), rgb(b).into());
    linear_gradient(angle, linear_color_stop(a, 0.), linear_color_stop(b, 1.))
}

/// The steps across the header; a step not reached yet can't be picked.
fn stepper(this: &Onboarding, t: &Theme, cx: &mut Context<Onboarding>) -> impl IntoElement {
    let items = STEPS.iter().enumerate().map(|(i, step)| {
        let (current, filled, done) = (i == this.step, i <= this.step, i < this.step);
        let line = (i > 0).then(|| {
            div()
                .mx(px(12.))
                .h(px(4.))
                .w(px(48.))
                .overflow_hidden()
                .rounded_full()
                .bg(t.border_strong)
                .when(filled, |l| {
                    l.child(div().size_full().bg(logo_fill(90., LOGO_BLUE, LOGO_VIOLET)))
                })
        });
        let dot = div()
            .relative()
            .size(px(24.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .text_size(px(FS_SM))
            .font_weight(FontWeight::SEMIBOLD)
            .line_height(relative(1.))
            .map(|d| {
                if filled {
                    d.bg(logo_fill(135., LOGO_BLUE, LOGO_PINK))
                        .text_color(gpui::white())
                } else {
                    d.bg(t.bg_selected_muted).text_color(t.text_subtle)
                }
            })
            .map(|d| {
                if done {
                    d.child(icon("check-bold", 14.))
                } else {
                    d.child((i + 1).to_string())
                }
            })
            // A ring in the logo's colours around the step shown.
            .when(current, |d| {
                d.child(
                    div()
                        .absolute()
                        .top(px(-5.))
                        .left(px(-5.))
                        .size(px(34.))
                        .rounded_full()
                        .border(px(1.5))
                        .border_color(Hsla::from(rgb(LOGO_VIOLET)).opacity(0.8)),
                )
            });
        let label = div()
            .when(filled, |l| l.text_color(t.text))
            .when(current, |l| l.font_weight(FontWeight::SEMIBOLD))
            .child(step.label);
        let name = if done {
            format!("{}, done", step.label)
        } else {
            step.label.to_string()
        };
        let pickable = i <= this.reached;
        div()
            .id(("setup-step-item", i))
            .role(Role::ListItem)
            .flex()
            .items_center()
            .children(line)
            .child(
                div()
                    .id(("setup-step", i))
                    .role(Role::Button)
                    .aria_label(name)
                    .aria_selected(current)
                    .flex()
                    .items_center()
                    .gap(px(20.))
                    .rounded(px(RADIUS_SM))
                    .px(px(8.))
                    .py(px(4.))
                    .text_color(t.text_muted)
                    .child(dot)
                    .child(label)
                    .when(pickable, |b| {
                        b.tab_index(0).cursor_pointer().on_click(
                            cx.listener(move |this, _, window, cx| this.go_to(i, window, cx)),
                        )
                    }),
            )
    });
    div()
        .id("setup-steps")
        .role(Role::Navigation)
        .aria_label("Setup steps")
        .flex_none()
        .child(
            div()
                .id("setup-step-list")
                .role(Role::List)
                .flex()
                .items_center()
                .children(items),
        )
}

impl Render for Onboarding {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let light = t.bg_app == Theme::light().bg_app;
        let current = &STEPS[self.step];
        let can_continue = self.can_continue();
        let is_last = self.step == LAST_STEP;
        let body: AnyElement = match self.step {
            0 => agent_step::agent_step(self, &t, cx),
            IMPORTS_STEP => imports_step::imports_step(self, &t, cx),
            PROJECTS_STEP => projects_step::projects_step(self, &t, window, cx),
            _ => ready_step::ready_step(self, &t, cx),
        };

        let header = div()
            .flex_none()
            .flex()
            .items_center()
            .h(px(88.))
            .px(px(24.))
            .border_b_1()
            .border_color(t.border)
            .bg(t.bg_app)
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(img("logos/logo.svg").size(px(36.)).flex_none())
                    .child(
                        div()
                            .text_size(px(FS_XL))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Patchwork"),
                    ),
            )
            .child(stepper(self, &t, cx))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        icon_button(
                            "setup-theme",
                            "Dark theme",
                            if light { "sun" } else { "moon" },
                            16.,
                            cx,
                        )
                        .aria_toggled(if light {
                            gpui::Toggled::False
                        } else {
                            gpui::Toggled::True
                        })
                        .tooltip(tooltip(if light {
                            "Light theme (switch to dark)"
                        } else {
                            "Dark theme (switch to light)"
                        }))
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(OnboardingEvent::ToggleTheme))),
                    )
                    .child(
                        btn("setup-skip", "Skip setup", BtnKind::Plain, false, &t)
                            .bg(gpui::transparent_black())
                            .border_color(gpui::transparent_black())
                            .text_color(t.text_muted)
                            .child("Skip setup")
                            .on_click(cx.listener(|this, _, _, cx| this.complete(cx))),
                    ),
            );

        // The projects step is two buttons and a list, which need no box around them.
        let section = div()
            .mt(px(24.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .when(self.step != PROJECTS_STEP, |s| {
                s.min_h(px(520.))
                    .rounded(px(RADIUS_LG))
                    .bg(t.bg_editor)
                    .p(px(24.))
            })
            .child(body);

        let main = div()
            .id("setup-main")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .py(px(40.))
            .child(
                div()
                    .w_full()
                    .max_w(px(820.))
                    .mx_auto()
                    .px(px(24.))
                    .child(
                        div()
                            .id("setup-heading")
                            .track_focus(&self.heading)
                            .role(Role::Heading)
                            .aria_level(1)
                            .text_size(px(FS_XXL))
                            .font_weight(FontWeight::SEMIBOLD)
                            .line_height(relative(1.25))
                            .child(current.title),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .text_size(px(FS_LG))
                            .text_color(t.text_muted)
                            .child(current.subtitle),
                    )
                    .child(section),
            );

        let step = self.step;
        let continue_label = if is_last {
            "Start your first task"
        } else {
            "Continue"
        };
        let footer = div()
            .flex_none()
            .h(px(84.))
            .border_t_1()
            .border_color(t.border)
            .bg(t.bg_app)
            .child(
                div()
                    .w_full()
                    .max_w(px(820.))
                    .mx_auto()
                    .px(px(24.))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .when(step > 0, |row| {
                                row.child(
                                    large(btn("setup-back", "Back", BtnKind::Plain, false, &t))
                                        .child("Back")
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.go_to(step - 1, window, cx);
                                        })),
                                )
                            })
                            // Moves on without importing, also while the scans still run, or
                            // without adding a project.
                            .when(step == IMPORTS_STEP || step == PROJECTS_STEP, |row| {
                                let skip = large(btn(
                                    "setup-skip-step",
                                    "Skip",
                                    BtnKind::Plain,
                                    self.saving,
                                    &t,
                                ))
                                .child("Skip");
                                row.child(if self.saving {
                                    skip
                                } else {
                                    skip.on_click(cx.listener(move |this, _, window, cx| {
                                        this.go_to(step + 1, window, cx);
                                    }))
                                })
                            })
                            .child({
                                let go = large(btn(
                                    "setup-continue",
                                    continue_label,
                                    BtnKind::Primary,
                                    !can_continue,
                                    &t,
                                ))
                                .min_w(px(190.))
                                .child(continue_label)
                                .child(if self.saving {
                                    spinner(16., t.text_on_accent).into_any_element()
                                } else {
                                    icon("arrow-right", 16.).into_any_element()
                                });
                                if can_continue {
                                    go.on_click(
                                        cx.listener(|this, _, window, cx| this.next(window, cx)),
                                    )
                                } else {
                                    go
                                }
                            }),
                    ),
            );

        div()
            .id("onboarding")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                if e.keystroke.key == "enter" && !e.keystroke.modifiers.modified() {
                    this.on_enter(window, cx);
                }
            }))
            .size_full()
            .flex()
            .flex_col()
            .bg(t.bg_app)
            .text_color(t.text)
            .child(header)
            .child(main)
            .child(footer)
    }
}

/// `.btn-lg`: a taller, wider button.
fn large(b: gpui::Stateful<gpui::Div>) -> gpui::Stateful<gpui::Div> {
    b.h(px(36.)).px(px(20.))
}
