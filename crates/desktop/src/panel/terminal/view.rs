//! The active shell drawn in the panel, with the handlers its screen takes.

// Rust guideline compliant 2026-10-09

use std::time::Duration;

use gpui::{
    AnyElement, DispatchPhase, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Role,
    ScrollEvent, Window, canvas, div, prelude::*,
};

use crate::panel::Panel;
use crate::panel::cells::draw_screen;
use crate::panel::input::Mouse;
use crate::ui::selection::from_primary;
use crate::ui::theme::theme;

use super::select::spans;
use super::shell::Metrics;

/// How long the cursor shows, then hides, while it blinks.
const BLINK: Duration = Duration::from_millis(600);

impl Panel {
    /// The active shell's screen, sized to the panel, or nothing while none is open.
    pub(in crate::panel) fn terminal_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = *theme(cx);
        let metrics = Metrics::new(&self.settings, window);
        let focused = self.term_focus.is_focused(window);
        let measure = {
            let this = cx.entity().downgrade();
            let metrics = metrics.clone();
            let selecting = self.selecting;
            canvas(
                {
                    let this = this.clone();
                    let metrics = metrics.clone();
                    move |bounds, _, cx| {
                        let _ = this.update(cx, |this, cx| this.fit(bounds, &metrics, cx));
                    }
                },
                // A drag that began on the screen follows the pointer over the whole window.
                move |_, (), window, _| {
                    if !selecting {
                        return;
                    }
                    let (moved, released) = ((this.clone(), metrics.clone()), (this, metrics));
                    window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble {
                            let (this, metrics) = &moved;
                            let _ = this.update(cx, |this, cx| {
                                if this.selecting {
                                    this.drag_to(e.position, metrics, cx);
                                }
                            });
                        }
                    });
                    window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble && e.button == MouseButton::Left {
                            let (this, metrics) = &released;
                            let _ = this.update(cx, |this, cx| this.release(e, metrics, cx));
                        }
                    });
                },
            )
            .absolute()
            .size_full()
        };
        // The cursor blinks only while the terminal has the keys, as xterm's does.
        let blinking = self.settings.cursor_blinking && focused;
        let elapsed = self.blink_from.elapsed();
        let shown = !blinking || (elapsed.as_millis() / BLINK.as_millis()).is_multiple_of(2);
        if blinking && self.blink_timer.is_none() {
            let wait = BLINK.saturating_sub(Duration::from_millis(
                (elapsed.as_millis() % BLINK.as_millis()) as u64,
            ));
            self.blink_timer = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(wait).await;
                let _ = this.update(cx, |this, cx| {
                    this.blink_timer = None;
                    cx.notify();
                });
            }));
        }
        let selection = self
            .active_shell_mut()
            .and_then(|shell| {
                let span = shell.selection?;
                Some(spans(shell.parser.screen_mut(), span, shell.block))
            })
            .unwrap_or_default();
        let screen = self.active_shell().map(|shell| {
            let screen = shell.parser.screen();
            draw_screen(
                screen,
                &selection,
                &metrics,
                &self.settings,
                focused,
                shown,
                &t,
            )
        });
        div()
            .id("terminal")
            .role(Role::Group)
            .aria_label("Terminal")
            .key_context("Terminal")
            .track_focus(&self.term_focus)
            .relative()
            .size_full()
            .min_w_0()
            .overflow_hidden()
            .cursor_text()
            .bg(t.bg_editor)
            .on_key_down(cx.listener(Self::on_terminal_key))
            .on_scroll(cx.listener({
                let metrics = metrics.clone();
                move |this, e: &ScrollEvent, _, cx| this.on_terminal_scroll(e, &metrics, cx)
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener({
                    let metrics = metrics.clone();
                    move |this, e: &MouseDownEvent, window, cx| {
                        window.focus(&this.term_focus, cx);
                        if this.report_mouse(Mouse::Press(0), e.position, e.modifiers, &metrics) {
                            this.mouse_held = Some(0);
                            return;
                        }
                        this.press(e, &metrics, cx);
                    }
                }),
            )
            .on_mouse_move(cx.listener({
                let metrics = metrics.clone();
                move |this, e: &MouseMoveEvent, _, _| {
                    let event = this.mouse_held.map_or(Mouse::Move, Mouse::Drag);
                    if !this.selecting {
                        this.report_mouse(event, e.position, e.modifiers, &metrics);
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener({
                    let metrics = metrics.clone();
                    move |this, e: &MouseUpEvent, _, _| {
                        if this.mouse_held.take().is_some() {
                            this.report_mouse(Mouse::Release(0), e.position, e.modifiers, &metrics);
                        }
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener({
                    let metrics = metrics.clone();
                    move |this, e: &MouseDownEvent, window, cx| {
                        window.focus(&this.term_focus, cx);
                        if this.report_mouse(Mouse::Press(1), e.position, e.modifiers, &metrics) {
                            this.mouse_held = Some(1);
                        } else if let Some(text) = from_primary(cx) {
                            // A middle click pastes the primary selection, as in xterm.
                            this.paste_text(&text, window, cx);
                        }
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener({
                    let metrics = metrics.clone();
                    move |this, e: &MouseUpEvent, _, _| {
                        if this.mouse_held.take().is_some() {
                            this.report_mouse(Mouse::Release(1), e.position, e.modifiers, &metrics);
                        }
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener({
                    let metrics = metrics.clone();
                    move |this, e: &MouseUpEvent, _, _| {
                        if this.mouse_held.take().is_some() {
                            this.report_mouse(Mouse::Release(2), e.position, e.modifiers, &metrics);
                        }
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener({
                    move |this, e: &MouseDownEvent, window, cx| {
                        use patchwork_core::settings::RightClickBehavior as Behavior;
                        window.focus(&this.term_focus, cx);
                        if this.report_mouse(Mouse::Press(2), e.position, e.modifiers, &metrics) {
                            this.mouse_held = Some(2);
                            return;
                        }
                        let behavior = this.settings.right_click_behavior;
                        match behavior {
                            Behavior::CopyPaste if this.copy_selection(cx) => {
                                if let Some(shell) = this.active_shell_mut() {
                                    shell.selection = None;
                                }
                            }
                            Behavior::CopyPaste | Behavior::Paste => this.paste(window, cx),
                            _ => {}
                        }
                        cx.notify();
                    }
                }),
            )
            .child(measure)
            .children(screen)
            .into_any_element()
    }
}
