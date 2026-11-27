//! The browser view drawn: back, forward, reload and the address over the page, the page's
//! picture taking the pointer and keys, and the prompt to install a browser when none is.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{
    AnyElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Role,
    ScrollEvent, SharedString, Window, canvas, div, img, prelude::*, px,
};
use patchwork_backend::browser::{INSTALL_URL, MouseAction};

use super::headless_page::BrowserView;
use patchwork_ui_theme::{Theme, fs_sm, theme};
use patchwork_ui_components::{BtnKind, btn, icon_button, input_box, spinner, tooltip};

impl BrowserView {
    fn toolbar(&self, t: &Theme, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let focused = gpui::Focusable::focus_handle(self.address.read(cx), cx).is_focused(window);
        let nav = |id: &'static str, label: &'static str, glyph: &'static str, on: bool| {
            icon_button(id, label, glyph, 16., cx)
                .tooltip(tooltip(label))
                .when(!on, |b| b.opacity(0.4).cursor_default())
        };
        let (reload_label, reload_icon) = if self.loading && self.sender.is_some() {
            ("Stop", "x")
        } else {
            ("Reload", "arrow-clockwise")
        };
        div()
            .id("browser-toolbar")
            .role(Role::Toolbar)
            .aria_label("Page")
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .px(px(8.))
            .py(px(6.))
            .border_b_1()
            .border_color(t.border)
            .child(
                nav("browser-back", "Back", "arrow-left", self.back).on_click(cx.listener(
                    |this, _, _, _| {
                        if this.back {
                            this.send(patchwork_backend::browser::BrowserCommand::Back);
                        }
                    },
                )),
            )
            .child(
                nav("browser-forward", "Forward", "arrow-right", self.forward).on_click(
                    cx.listener(|this, _, _, _| {
                        if this.forward {
                            this.send(patchwork_backend::browser::BrowserCommand::Forward);
                        }
                    }),
                ),
            )
            .child(
                nav("browser-reload", reload_label, reload_icon, true)
                    .on_click(cx.listener(|this, _, _, cx| this.reload_or_stop(cx))),
            )
            .child(
                input_box(focused, t)
                    .flex_1()
                    .child(div().flex_1().min_w_0().child(self.address.clone())),
            )
            .child(
                icon_button(
                    "browser-external",
                    "Open in browser",
                    "arrow-square-out",
                    16.,
                    cx,
                )
                .tooltip(tooltip("Open in browser"))
                .on_click(cx.listener(|this, _, _, cx| cx.open_url(&this.url))),
            )
            .into_any_element()
    }

    /// Asks for a browser, since none is installed.
    fn install(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .p(px(24.))
            .child(
                div()
                    .id("browser-missing")
                    .role(Role::Alert)
                    .text_size(px(fs_sm()))
                    .text_color(t.text_muted)
                    .child("Showing pages needs Google Chrome or another Chromium browser."),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        btn(
                            "browser-install",
                            "Get Google Chrome",
                            BtnKind::Primary,
                            false,
                            t,
                        )
                        .on_click(|_, _, cx| cx.open_url(INSTALL_URL)),
                    )
                    .child(
                        btn("browser-retry", "Try again", BtnKind::Plain, false, t).on_click(
                            cx.listener(|this, _, _, cx| {
                                let url = this.url.clone();
                                this.start(url, cx);
                            }),
                        ),
                    ),
            )
            .into_any_element()
    }

    /// What is shown while there is no picture: the error, or that it is loading.
    fn waiting(&self, t: &Theme) -> AnyElement {
        match &self.error {
            Some(e) => div()
                .id("browser-error")
                .role(Role::Alert)
                .p(px(16.))
                .text_size(px(fs_sm()))
                .text_color(t.text_muted)
                .child(e.clone())
                .into_any_element(),
            None => div()
                .id("browser-waiting")
                .role(Role::Status)
                .aria_label("Loading the page")
                .child(spinner(16., t.text_muted))
                .into_any_element(),
        }
    }

    fn page(&self, t: &Theme, scale: f32, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity().downgrade();
        let name: SharedString = if self.title.is_empty() {
            self.label.clone().into()
        } else {
            format!("{}: {}", self.label, self.title).into()
        };
        div()
            .id("browser-page")
            .role(Role::Application)
            .aria_label(name)
            .tab_index(0)
            .track_focus(&self.focus)
            .focus_visible(patchwork_ui_components::focus_ring)
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::white())
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let _ = this.update(cx, |this, cx| {
                            if this.fit(bounds, window, cx) {
                                cx.notify();
                            }
                        });
                    },
                    |_, (), _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .map(|page| match &self.image {
                // Drawn at its own size, so a frame from before a resize is never stretched.
                Some(image) => {
                    let size = image.size(0);
                    page.child(
                        img(Arc::clone(image))
                            .absolute()
                            .top_0()
                            .left_0()
                            .w(px(size.width.0 as f32 / scale))
                            .h(px(size.height.0 as f32 / scale)),
                    )
                }
                None => page.bg(t.bg_editor).child(self.waiting(t)),
            })
            .when(self.picking, gpui::Styled::cursor_crosshair)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    if this.picking {
                        cx.stop_propagation();
                        this.pick(e.position, cx);
                        return;
                    }
                    this.mouse(
                        e.position,
                        MouseAction::Down,
                        Some(e.button),
                        e.click_count,
                        e.modifiers,
                    );
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    this.mouse(
                        e.position,
                        MouseAction::Down,
                        Some(e.button),
                        e.click_count,
                        e.modifiers,
                    );
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, _| {
                if this.picking {
                    this.highlight(e.position);
                    return;
                }
                this.mouse(
                    e.position,
                    MouseAction::Move,
                    e.pressed_button,
                    0,
                    e.modifiers,
                );
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, e: &MouseUpEvent, _, _| {
                    this.mouse(
                        e.position,
                        MouseAction::Up,
                        Some(e.button),
                        e.click_count,
                        e.modifiers,
                    );
                }),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|this, e: &MouseUpEvent, _, _| {
                    this.mouse(
                        e.position,
                        MouseAction::Up,
                        Some(e.button),
                        e.click_count,
                        e.modifiers,
                    );
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, e: &MouseUpEvent, _, _| {
                    this.mouse(
                        e.position,
                        MouseAction::Up,
                        Some(e.button),
                        e.click_count,
                        e.modifiers,
                    );
                }),
            )
            .on_scroll(cx.listener(|this, e: &ScrollEvent, _, cx| {
                cx.stop_propagation();
                this.wheel(e);
            }))
            // Escape goes to the page and also leaves it, so the keys are never trapped.
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if this.picking && e.keystroke.key == "escape" {
                    this.set_picking(false, cx);
                    cx.stop_propagation();
                    return;
                }
                if this.key(e, cx) && e.keystroke.key != "escape" {
                    cx.stop_propagation();
                }
            }))
            .into_any_element()
    }
}

impl Render for BrowserView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        self.theme(t.is_dark());
        div()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .when(self.toolbar, |v| v.child(self.toolbar(&t, window, cx)))
            .map(|v| {
                if self.missing {
                    v.child(self.install(&t, cx))
                } else {
                    v.child(self.page(&t, window.scale_factor(), cx))
                }
            })
    }
}
