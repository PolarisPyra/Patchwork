//! `DeleteDialog`: the app's own confirm dialog, drawn for every `window.prompt` in place of GPUI's
//! plain fallback. The cancel button is the default, so Enter keeps; Escape and a press outside the
//! dialog cancel too.

use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent, MouseButton,
    PromptButton, PromptHandle, PromptLevel, PromptResponse, RenderablePromptHandle, Role,
    SharedString, Window, div, prelude::*, px,
};

use crate::theme::{FS_LG, FS_MD, RADIUS, RADIUS_SM, theme};
use crate::ui::popup_shadow;

pub fn build(
    level: PromptLevel,
    message: &str,
    detail: Option<&str>,
    actions: &[PromptButton],
    handle: PromptHandle,
    window: &mut Window,
    cx: &mut App,
) -> RenderablePromptHandle {
    // The cancel button sits first, then the rest in the order asked.
    let mut order: Vec<usize> = (0..actions.len())
        .filter(|&i| actions[i].is_cancel())
        .collect();
    order.extend((0..actions.len()).filter(|&i| !actions[i].is_cancel()));
    let cancel = order.first().copied().unwrap_or(0);
    let view = cx.new(|cx| Confirm {
        danger: level != PromptLevel::Info,
        title: message.into(),
        detail: detail.map(|d| d.to_string().into()),
        actions: actions.to_vec(),
        order,
        cancel,
        active: cancel,
        keyboard: false,
        focus: cx.focus_handle(),
    });
    handle.with_view(view, window, cx)
}

struct Confirm {
    /// The confirming buttons are red, as when they delete or remove.
    danger: bool,
    title: SharedString,
    detail: Option<SharedString>,
    actions: Vec<PromptButton>,
    /// The buttons left to right, as indexes into `actions`.
    order: Vec<usize>,
    cancel: usize,
    /// The button Enter presses.
    active: usize,
    /// Whether the keyboard moved, so the active button shows its ring.
    keyboard: bool,
    focus: FocusHandle,
}

impl EventEmitter<PromptResponse> for Confirm {}

impl Focusable for Confirm {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Confirm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let mut buttons = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_end()
            .gap(px(8.))
            .border_t_1()
            .border_color(t.border)
            .px(px(16.))
            .py(px(8.));
        for &ix in &self.order {
            let action = &self.actions[ix];
            let confirm = !action.is_cancel();
            let (bg, border, fg, hover) = if confirm && self.danger {
                let mut hover = t.danger;
                hover.l *= 0.92;
                (t.danger, t.danger, t.text_on_accent, hover)
            } else if confirm {
                (t.accent, t.accent, t.text_on_accent, t.accent_hover)
            } else {
                (t.bg_editor, t.border_strong, t.text, t.bg_hover)
            };
            buttons = buttons.child(
                div()
                    .id(ix)
                    .role(Role::Button)
                    .aria_label(action.label().clone())
                    .h(px(28.))
                    .px(px(14.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(RADIUS_SM))
                    .border_1()
                    .border_color(border)
                    .bg(bg)
                    .text_color(fg)
                    .text_size(px(FS_MD))
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .when(self.keyboard && self.active == ix, |s| {
                        s.border_2().border_color(t.accent)
                    })
                    .child(action.label().clone())
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.stop_propagation();
                        cx.emit(PromptResponse(ix));
                    })),
            );
        }
        let cancel = self.cancel;
        div()
            .id("prompt")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |_, _, _, cx| cx.emit(PromptResponse(cancel))),
            )
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                let at = this
                    .order
                    .iter()
                    .position(|&i| i == this.active)
                    .unwrap_or(0);
                match e.keystroke.key.as_str() {
                    "escape" => cx.emit(PromptResponse(this.cancel)),
                    "enter" | "space" => cx.emit(PromptResponse(this.active)),
                    "tab" | "right" | "left" => {
                        let back = e.keystroke.key == "left" || e.keystroke.modifiers.shift;
                        let n = this.order.len();
                        let next = (if back { at + n - 1 } else { at + 1 }) % n.max(1);
                        this.active = this.order.get(next).copied().unwrap_or(this.cancel);
                        this.keyboard = true;
                        cx.notify();
                    }
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .child(
                div()
                    .id("prompt-dialog")
                    .role(Role::Dialog)
                    .aria_label(self.title.clone())
                    .w(px(360.))
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .rounded(px(RADIUS))
                    .border_1()
                    .border_color(t.border_strong)
                    .bg(t.bg_elevated)
                    .shadow(popup_shadow(&t))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .px(px(20.))
                            .pt(px(12.))
                            .pb(px(10.))
                            .line_height(gpui::relative(1.25))
                            .child(
                                div()
                                    .text_size(px(FS_LG))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(t.text)
                                    .child(self.title.clone()),
                            )
                            .children(self.detail.clone().map(|detail| {
                                div()
                                    .mt(px(4.))
                                    .text_size(px(FS_MD))
                                    .text_color(t.text_muted)
                                    .child(detail)
                            })),
                    )
                    .child(buttons),
            )
    }
}
