//! The active shell drawn in the panel, with the handlers its screen takes.

// Rust guideline compliant 2026-10-09

use std::time::Duration;

use gpui::{
    AnyElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Role, ScrollEvent,
    Window, canvas, div, prelude::*,
};

use crate::panel::Panel;
use crate::panel::cells::{draw_screen, line_at, word_at};
use crate::panel::input::Mouse;
use crate::ui::theme::theme;

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
            canvas(
                move |bounds, _, cx| {
                    let _ = this.update(cx, |this, cx| this.fit(bounds, &metrics, cx));
                },
                |_, (), _, _| {},
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
        let screen = self.active_shell().map(|shell| {
            let screen = shell.parser.screen();
            draw_screen(
                screen,
                shell.selection,
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
                        if e.modifiers.alt
                            && this.settings.alt_click_moves_cursor
                            && this.alt_click(e.position, &metrics)
                        {
                            cx.notify();
                            return;
                        }
                        let cell = this.cell_at(e.position, &metrics);
                        let under = this.cell_under(e.position, &metrics);
                        if let Some(shell) = this.active_shell_mut() {
                            // A double click takes the word, a triple click the line, as in xterm.
                            let screen = shell.parser.screen();
                            shell.selection = Some(match e.click_count {
                                0 | 1 => (cell, cell),
                                2 => word_at(screen, under),
                                _ => line_at(screen, under.0),
                            });
                        }
                        this.selecting = e.click_count < 2;
                        cx.notify();
                    }
                }),
            )
            .on_mouse_move(cx.listener({
                let metrics = metrics.clone();
                move |this, e: &MouseMoveEvent, _, cx| {
                    let event = this.mouse_held.map_or(Mouse::Move, Mouse::Drag);
                    if !this.selecting
                        && this.report_mouse(event, e.position, e.modifiers, &metrics)
                    {
                        return;
                    }
                    if !this.selecting || !e.dragging() {
                        return;
                    }
                    let cell = this.cell_at(e.position, &metrics);
                    if let Some(shell) = this.active_shell_mut()
                        && let Some((_, end)) = &mut shell.selection
                    {
                        *end = cell;
                        cx.notify();
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener({
                    let metrics = metrics.clone();
                    move |this, e: &MouseUpEvent, _, cx| {
                        if this.mouse_held.take().is_some() {
                            this.report_mouse(Mouse::Release(0), e.position, e.modifiers, &metrics);
                            return;
                        }
                        this.selecting = false;
                        let copy = this.settings.copy_on_selection;
                        if let Some(shell) = this.active_shell_mut()
                            && shell.selection.is_some_and(|(a, b)| a == b)
                        {
                            shell.selection = None;
                        }
                        if copy {
                            this.copy_selection(cx);
                        }
                        cx.notify();
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
                    let metrics = metrics.clone();
                    move |this, e: &MouseDownEvent, window, cx| {
                        use patchwork_server::settings::RightClickBehavior as Behavior;
                        window.focus(&this.term_focus, cx);
                        if this.report_mouse(Mouse::Press(2), e.position, e.modifiers, &metrics) {
                            this.mouse_held = Some(2);
                            return;
                        }
                        match this.settings.right_click_behavior {
                            Behavior::CopyPaste if this.copy_selection(cx) => {
                                if let Some(shell) = this.active_shell_mut() {
                                    shell.selection = None;
                                }
                            }
                            Behavior::CopyPaste | Behavior::Paste => this.paste(cx),
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
