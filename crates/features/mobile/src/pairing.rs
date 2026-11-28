//! Pairing the phone with a Patchwork: its address and the code its Settings › Devices page shows.

// Rust guideline compliant 2026-11-27

use gpui::{
    Context, EventEmitter, FocusHandle, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use patchwork_client::{Pairing, Session};
use patchwork_design_components::{BtnKind, btn, input_box};
use patchwork_design_theme::{Theme, fs_xxl, theme};

/// What the pairing screen tells the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingEvent {
    /// Connected to the Patchwork at this address.
    Paired(String),
}

/// The field a key goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Address,
    Code,
}

/// The pairing form, which shows how [`Session`]'s pairing stands.
#[derive(Debug)]
pub struct PairingScreen {
    address: String,
    code: String,
    field: Field,
    focus: FocusHandle,
}

impl PairingScreen {
    /// Makes the form, which tells the app once the session connects.
    #[must_use]
    pub fn new(cx: &mut Context<'_, Self>) -> Self {
        cx.observe_global::<Session>(|_, cx| {
            if let Pairing::Connected(address) = &Session::get(cx).connection.pairing {
                cx.emit(PairingEvent::Paired(address.clone()));
            }
            cx.notify();
        })
        .detach();
        Self {
            address: String::new(),
            code: String::new(),
            field: Field::Address,
            focus: cx.focus_handle(),
        }
    }

    /// Takes a key typed into the field in focus.
    fn key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<'_, Self>) {
        let keystroke = &event.keystroke;
        let text = match self.field {
            Field::Address => &mut self.address,
            Field::Code => &mut self.code,
        };
        match keystroke.key.as_str() {
            "backspace" => {
                text.pop();
            }
            "enter" | "tab" => {
                if self.field == Field::Address {
                    self.field = Field::Code;
                } else {
                    self.pair(cx);
                }
            }
            _ => match &keystroke.key_char {
                Some(typed) if !keystroke.modifiers.control => text.push_str(typed),
                _ => return,
            },
        }
        cx.notify();
    }

    /// Claims the code at the address.
    fn pair(&self, cx: &mut Context<'_, Self>) {
        let address = self.address.trim().to_string();
        let code = self.code.trim().to_string();
        if !address.is_empty() && !code.is_empty() {
            Session::pair(address, code, "Android phone".into(), cx);
        }
    }

    /// One field, which takes the keys once tapped.
    fn field(
        &self,
        field: Field,
        focused: bool,
        t: &Theme,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let (id, label, text) = match field {
            Field::Address => ("pairing.address", "Address", &self.address),
            Field::Code => ("pairing.code", "Code", &self.code),
        };
        let shown = if text.is_empty() {
            div().text_color(t.text_muted).child(label)
        } else {
            div().text_color(t.text).child(text.clone())
        };
        input_box(focused && self.field == field, t).w_full().child(
            div()
                .id(id)
                .size_full()
                .flex()
                .items_center()
                .child(shown)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.field = field;
                    window.focus(&this.focus, cx);
                    window.request_virtual_keyboard();
                    cx.notify();
                })),
        )
    }
}

impl EventEmitter<PairingEvent> for PairingScreen {}

impl Render for PairingScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let t = *theme(cx);
        let focused = self.focus.is_focused(window);
        let status = match &Session::get(cx).connection.pairing {
            Pairing::Connecting => Some(div().text_color(t.text_muted).child("Connecting")),
            Pairing::Failed(reason) => Some(div().text_color(t.danger).child(reason.clone())),
            Pairing::Unpaired | Pairing::Connected(_) => None,
        };
        div()
            .id("pairing")
            .size_full()
            .flex()
            .flex_col()
            .justify_center()
            .gap(px(14.0))
            .px(px(24.0))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .child(
                div()
                    .text_size(px(fs_xxl()))
                    .text_color(t.text)
                    .child("Pair with Patchwork"),
            )
            .child(self.field(Field::Address, focused, &t, cx))
            .child(self.field(Field::Code, focused, &t, cx))
            .child(
                btn("pairing.pair", "Pair", BtnKind::Primary, false, &t)
                    .on_click(cx.listener(|this, _, _, cx| this.pair(cx))),
            )
            .children(status)
    }
}
