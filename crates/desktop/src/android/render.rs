//! The Android panel drawn: its header, what is wrong, the devices, and the
//! SDK it found.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, FontWeight, KeyDownEvent, Role, SharedString, Window, div, prelude::*, px, relative,
};
use patchwork_server::Error;
use patchwork_server::android::{self, Sdk};

use super::state::AndroidPanel;
use crate::ui::menu::Menu;
use crate::ui::theme::{FONT_MONO, FS_LG, FS_SM, FS_XS, RADIUS, Theme, theme};
use crate::ui::{BtnKind, btn, button, icon, icon_button, input_box, scrollbar, spinner, tooltip};

/// What dismissing an alert does.
pub(super) type Dismiss = fn(&mut AndroidPanel);

impl Render for AndroidPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let header = self.header(&t, cx);
        let body = self.body(&t, window, cx);
        let footer = self.footer(&t, window, cx);
        let screen = self.screen.clone().filter(|_| self.create.is_none());
        div()
            .id("android-panel")
            .role(Role::Complementary)
            .aria_label("Android devices")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key != "escape" {
                    return;
                }
                // Escape backs out of what is open, innermost first.
                if this.confirm.take().is_some() {
                    cx.notify();
                } else if this.sdk_editing {
                    this.sdk_editing = false;
                    cx.notify();
                } else {
                    return;
                }
                cx.stop_propagation();
            }))
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .bg(t.bg_editor)
            .border_l_1()
            .border_color(t.border)
            .child(header)
            .child(
                div()
                    .relative()
                    .min_h_0()
                    // While a screen shows under them, the devices take the room they need, up to half
                    // the panel, and scroll past that.
                    .map(|list| {
                        if screen.is_some() {
                            list.max_h(relative(0.5))
                                .border_b_1()
                                .border_color(t.border)
                        } else {
                            list.flex_1()
                        }
                    })
                    .child(
                        div()
                            .id("android-body")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .py(px(14.))
                            .px(px(20.))
                            .children(body),
                    )
                    .children(scrollbar(&self.scroll, 10., 2., t.border_strong)),
            )
            .children(screen)
            .children(footer)
            .children(
                self.menu
                    .as_ref()
                    .map(|(_, menu, at)| Menu::view(menu, *at)),
            )
    }
}

impl AndroidPanel {
    fn header(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .flex_none()
            .py(px(14.))
            .px(px(20.))
            .border_b_1()
            .border_color(t.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_none()
                    .size(px(36.))
                    .rounded(px(8.))
                    .bg(t.bg_accent_soft)
                    .text_color(t.accent_text)
                    .child(icon("device-mobile", 18.)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .id("android-title")
                            .role(Role::Heading)
                            .text_size(px(FS_LG))
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child("Android devices"),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(FS_SM))
                            .text_color(t.text_muted)
                            .child(format!(
                                "{} · {}",
                                self.project.framework.label(),
                                self.folder()
                            )),
                    ),
            )
            .child(
                icon_button("close-android", "Close Android devices", "x", 16., cx)
                    .tooltip(tooltip("Close"))
                    .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
            )
    }

    /// The panel's parts under the header, top to bottom.
    fn body(&self, t: &Theme, window: &Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut parts: Vec<AnyElement> = Vec::new();
        if let Some(failure) = &self.failure {
            let dismiss: Dismiss = |this| this.failure = None;
            parts.push(self.alert("failure", failure, Some(dismiss), t, cx));
        }
        let Some(snapshot) = &self.snapshot else {
            parts.push(
                div()
                    .id("android-loading")
                    .role(Role::Status)
                    .aria_label("Looking for the Android SDK")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_color(t.text_muted)
                    .child(spinner(14., t.text_muted))
                    .child("Looking for the Android SDK")
                    .into_any_element(),
            );
            return parts;
        };
        let sdk = match &snapshot.sdk {
            Ok(sdk) => sdk,
            Err(e) => {
                parts.push(self.alert("no-sdk", e, None, t, cx));
                parts.push(self.sdk_form(t, window, cx));
                return parts;
            }
        };
        for (i, missing) in android::missing_tools(sdk).iter().enumerate() {
            parts.push(self.alert(&format!("missing-{i}"), missing, None, t, cx));
        }
        if let Some(problem) = &self.acceleration {
            parts.push(self.alert("acceleration", problem, None, t, cx));
        }
        if let Err(e) = &snapshot.devices {
            parts.push(self.alert("devices-error", e, None, t, cx));
        }
        if let Some(create) = self.create_view(t, window, cx) {
            parts.push(create);
            return parts;
        }
        let can_create = sdk.avdmanager.is_some();
        parts.push(
            section("Virtual devices", t)
                .when(can_create, |row| {
                    row.child(
                        btn("android-new-device", "New device", BtnKind::Plain, false, t)
                            .child(icon("plus", 14.))
                            .child("New device")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_create(window, cx)),
                            ),
                    )
                })
                .into_any_element(),
        );
        if snapshot.avds.is_empty() {
            parts.push(
                div()
                    .text_color(t.text_muted)
                    .child(if can_create {
                        "No virtual devices yet."
                    } else {
                        "No virtual devices."
                    })
                    .into_any_element(),
            );
        } else {
            parts.push(
                list("android-avds", "Virtual devices")
                    .children(
                        snapshot
                            .avds
                            .iter()
                            .enumerate()
                            .map(|(i, avd)| self.avd_row(i, avd, t, cx)),
                    )
                    .into_any_element(),
            );
        }
        // Emulators running one of the virtual devices above show on its row.
        let connected: Vec<_> = self
            .devices()
            .iter()
            .filter(|device| {
                !device
                    .avd
                    .as_ref()
                    .is_some_and(|avd| snapshot.avds.iter().any(|known| &known.name == avd))
            })
            .collect();
        if !connected.is_empty() {
            parts.push(section("Connected devices", t).into_any_element());
            parts.push(
                list("android-connected", "Connected devices")
                    .children(
                        connected
                            .into_iter()
                            .enumerate()
                            .map(|(i, device)| self.device_row(i, device, t, cx)),
                    )
                    .into_any_element(),
            );
        }
        parts
    }

    /// The SDK in use and what named it, with the way to point at another.
    fn footer(&self, t: &Theme, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let sdk: &Sdk = self.sdk()?;
        let body = if self.sdk_editing {
            self.sdk_form(t, window, cx)
        } else {
            let path = sdk.root.to_string_lossy().into_owned();
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .id("android-sdk-path")
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .tooltip(tooltip(format!(
                            "{path}\nFrom {}",
                            android::source_label(sdk.source)
                        )))
                        .child(format!("SDK: {path}")),
                )
                .child(
                    button("android-sdk-change", "Change the Android SDK folder")
                        .flex_none()
                        .text_color(t.accent_text)
                        .child("Change")
                        .on_click(cx.listener(|this, _, window, cx| this.edit_sdk(window, cx))),
                )
                .into_any_element()
        };
        Some(
            div()
                .flex_none()
                .border_t_1()
                .border_color(t.border)
                .py(px(8.))
                .px(px(20.))
                .text_size(px(FS_SM))
                .text_color(t.text_muted)
                .child(body)
                .into_any_element(),
        )
    }

    /// The field that sets the SDK's folder by hand.
    fn sdk_form(&self, t: &Theme, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let focused = gpui::Focusable::focus_handle(self.sdk_field.read(cx), cx).is_focused(window);
        div()
            .id("android-sdk-form")
            .role(Role::Form)
            .aria_label("Android SDK folder")
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                input_box(focused, t)
                    .flex_1()
                    .font_family(FONT_MONO)
                    .child(div().flex_1().min_w_0().child(self.sdk_field.clone())),
            )
            .child(
                btn(
                    "android-sdk-save",
                    "Use this SDK folder",
                    BtnKind::Primary,
                    false,
                    t,
                )
                .child("Use")
                .on_click(cx.listener(|this, _, _, cx| this.save_sdk(cx))),
            )
            .into_any_element()
    }

    /// An error: what went wrong and what to do, with what the tool printed behind a disclosure.
    pub(super) fn alert(
        &self,
        id: &str,
        error: &Error,
        dismiss: Option<Dismiss>,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let detail = match error {
            Error::Tool { detail, .. } if !detail.trim().is_empty() => Some(detail.clone()),
            _ => None,
        };
        let open = self.open_details.contains(id);
        let key = id.to_string();
        div()
            .id(SharedString::from(format!("android-alert-{id}")))
            .role(Role::Alert)
            .flex()
            .flex_col()
            .gap(px(6.))
            .rounded(px(RADIUS))
            .border_1()
            .border_color(t.danger)
            .bg(t.danger_soft)
            .py(px(8.))
            .px(px(10.))
            .text_size(px(FS_SM))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .child(icon("warning", 14.).mt(px(2.)).text_color(t.danger))
                    .child(div().flex_1().min_w_0().child(error.to_string()))
                    .when_some(dismiss, |row, dismiss| {
                        row.child(
                            icon_button(
                                SharedString::from(format!("android-dismiss-{id}")),
                                "Dismiss",
                                "x",
                                12.,
                                cx,
                            )
                            .size(px(20.))
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    dismiss(this);
                                    cx.notify();
                                },
                            )),
                        )
                    }),
            )
            .when_some(detail, |alert, detail| {
                alert
                    .child(
                        button(
                            SharedString::from(format!("android-detail-{id}")),
                            "Tool output",
                        )
                        .aria_expanded(open)
                        .gap(px(4.))
                        .text_color(t.text_muted)
                        .child(icon(if open { "caret-down" } else { "caret-right" }, 12.))
                        .child("Tool output")
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_detail(&key, cx))),
                    )
                    .when(open, |alert| {
                        alert.child(
                            div()
                                .id(SharedString::from(format!("android-output-{id}")))
                                .role(Role::Region)
                                .aria_label("Tool output")
                                .tab_index(0)
                                .max_h(px(220.))
                                .overflow_y_scroll()
                                .rounded(px(RADIUS))
                                .bg(t.bg_app)
                                .p(px(8.))
                                .font_family(FONT_MONO)
                                .text_size(px(FS_XS))
                                .text_color(t.text_muted)
                                .children(detail.lines().map(|line| {
                                    div()
                                        .min_h(px(8.))
                                        .child(SharedString::from(line.to_string()))
                                })),
                        )
                    })
            })
            .into_any_element()
    }
}

/// A section's name, with room for its action at the right.
fn section(name: &'static str, t: &Theme) -> gpui::Div {
    div().flex().items_center().min_h(px(28.)).child(
        div()
            .id(name)
            .role(Role::Heading)
            .flex_1()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(t.text)
            .child(name),
    )
}

/// The box a section's devices are listed in, one of which is picked.
fn list(id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .role(Role::RadioGroup)
        .aria_label(label)
        .flex()
        .flex_col()
        .gap(px(2.))
}
