//! The create flow drawn: the hardware step, the image step, and a download with its licence.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, FontWeight, Role, ScrollHandle, SharedString, Window, div, prelude::*, px};
use patchwork_server::android::SystemImage;

use super::state::{Create, Install, Step};
use crate::android::rows::radio_row;
use crate::android::state::AndroidPanel;
use crate::ui::theme::{font_mono, fs_sm, fs_xs, radius, Theme};
use crate::ui::{BtnKind, badge, btn, checkbox, input_box, spinner};

impl AndroidPanel {
    /// The flow drawn in place of the device list.
    pub(in crate::android) fn create_view(
        &self,
        t: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let create = self.create.as_ref()?;
        let body = if let Some(install) = &create.install {
            self.install_view(install, t, cx)
        } else if create.working {
            working("Creating the device", t)
        } else {
            match create.step {
                Step::Profile => self.profile_step(create, t, cx),
                Step::Image => self.image_step(create, t, window, cx),
            }
        };
        let title = match create.step {
            Step::Profile => "New device: hardware",
            Step::Image => "New device: system image",
        };
        Some(
            div()
                .id("android-create")
                .role(Role::Form)
                .aria_label("New device")
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .id("android-create-title")
                        .role(Role::Heading)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .children(
                    create
                        .error
                        .as_ref()
                        .map(|e| self.alert("create-error", e, None, t, cx)),
                )
                .child(body)
                .into_any_element(),
        )
    }

    fn profile_step(&self, create: &Create, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let list = match &create.profiles {
            None => working("Reading the hardware profiles", t),
            Some(Err(e)) => self.alert("profiles-error", e, None, t, cx),
            Some(Ok(profiles)) => list_box(
                "android-profiles",
                "Hardware profiles",
                &create.profile_scroll,
                t,
            )
            .children(profiles.iter().enumerate().map(|(i, profile)| {
                let checked = create.profile.as_ref() == Some(profile);
                let picked = profile.clone();
                radio_row(("android-profile", i), profile.name.clone(), checked, t)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(profile.name.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(fs_sm()))
                            .text_color(t.text_muted)
                            .child(profile.oem.clone()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(create) = &mut this.create {
                            create.profile = Some(picked.clone());
                        }
                        this.name_device(cx);
                        cx.notify();
                    }))
            }))
            .into_any_element(),
        };
        let ready = create.profile.is_some();
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(list)
            .child(
                buttons()
                    .child(
                        btn("create-cancel", "Cancel", BtnKind::Plain, false, t)
                            .child("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.create = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        btn("create-next", "Next", BtnKind::Primary, !ready, t)
                            .child("Next")
                            .when(ready, |b| {
                                b.on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(create) = &mut this.create {
                                        create.step = Step::Image;
                                    }
                                    cx.notify();
                                }))
                            }),
                    ),
            )
            .into_any_element()
    }

    fn image_step(
        &self,
        create: &Create,
        t: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let shown: Vec<&SystemImage> = create
            .images
            .iter()
            .filter(|image| create.every_abi || image.abi == create.abi)
            .collect();
        // The installed ones lead, whatever their API level.
        let ordered = shown
            .iter()
            .filter(|image| image.installed)
            .chain(shown.iter().filter(|image| !image.installed));
        let rows = ordered.enumerate().map(|(i, image)| {
            let image = (*image).clone();
            let checked = create
                .image
                .as_ref()
                .is_some_and(|picked| picked.same(&image));
            let version = match &image.android_version {
                Some(version) => format!("Android {version} · API {}", image.api),
                None => format!("API {}", image.api),
            };
            let label = format!(
                "{version}, {}, {}, {}",
                image.variant(),
                image.abi,
                if image.installed {
                    "installed"
                } else {
                    "to download"
                }
            );
            radio_row(("android-image", i), label, checked, t)
                .child(div().flex_1().min_w_0().truncate().child(version))
                .child(
                    div()
                        .flex_none()
                        .text_size(px(fs_sm()))
                        .text_color(t.text_muted)
                        .child(format!("{} · {}", image.variant(), image.abi)),
                )
                .child(badge(
                    if image.installed {
                        "Installed"
                    } else {
                        "Download"
                    },
                    t,
                ))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(create) = &mut this.create {
                        create.image = Some(image.clone());
                    }
                    this.name_device(cx);
                    cx.notify();
                }))
        });
        let list =
            list_box("android-images", "System images", &create.image_scroll, t).children(rows);
        let ready = create.image.is_some();
        let action = match &create.image {
            Some(image) if !image.installed => "Download and create",
            _ => "Create",
        };
        let focused = gpui::Focusable::focus_handle(create.name.read(cx), cx).is_focused(window);
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(list)
            .when(create.fetching, |d| {
                d.child(working("Fetching the images to download", t))
            })
            .children(
                create
                    .fetch_error
                    .as_ref()
                    .map(|e| self.alert("images-error", e, None, t, cx)),
            )
            .child(
                checkbox(
                    "android-every-abi",
                    "Other architectures",
                    create.every_abi,
                    t,
                )
                .text_size(px(fs_sm()))
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(create) = &mut this.create {
                        create.every_abi = !create.every_abi;
                    }
                    cx.notify();
                })),
            )
            .child(input_box(focused, t).child(div().flex_1().min_w_0().child(create.name.clone())))
            .child(
                buttons()
                    .child(
                        btn("create-back", "Back", BtnKind::Plain, false, t)
                            .child("Back")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(create) = &mut this.create {
                                    create.step = Step::Profile;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        btn("create-submit", action, BtnKind::Primary, !ready, t)
                            .child(action)
                            .when(ready, |b| {
                                b.on_click(cx.listener(|this, _, _, cx| this.submit_create(cx)))
                            }),
                    ),
            )
            .into_any_element()
    }

    fn install_view(&self, install: &Install, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        if let Some(license) = &install.license {
            return div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .id("android-license")
                        .role(Role::Region)
                        .aria_label("Licence")
                        .tab_index(0)
                        .max_h(px(320.))
                        .overflow_y_scroll()
                        .rounded(px(radius()))
                        .border_1()
                        .border_color(t.border)
                        .bg(t.bg_app)
                        .p(px(10.))
                        .font_family(font_mono())
                        .text_size(px(fs_xs()))
                        .text_color(t.text_muted)
                        .children(license.lines().map(|line| {
                            div()
                                .min_h(px(8.))
                                .child(SharedString::from(line.to_string()))
                        })),
                )
                .child(
                    buttons()
                        .child(
                            btn("license-decline", "Decline", BtnKind::Plain, false, t)
                                .child("Decline")
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.answer_license(false, cx)),
                                ),
                        )
                        .child(
                            btn(
                                "license-accept",
                                "Accept the licence",
                                BtnKind::Primary,
                                false,
                                t,
                            )
                            .child("Accept")
                            .on_click(cx.listener(|this, _, _, cx| this.answer_license(true, cx))),
                        ),
                )
                .into_any_element();
        }
        let percent = f32::from(install.percent);
        div()
            .id("android-download")
            .role(Role::Status)
            .aria_label(format!(
                "Downloading the system image: {}%, {}",
                install.percent, install.step
            ))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(spinner(14., t.text_muted))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(install.step.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(t.text_muted)
                            .child(format!("{}%", install.percent)),
                    ),
            )
            .child(
                div()
                    .id("android-download-bar")
                    .role(Role::ProgressIndicator)
                    .aria_label("Download")
                    .aria_numeric_value(f64::from(install.percent))
                    .aria_min_numeric_value(0.)
                    .aria_max_numeric_value(100.)
                    .h(px(4.))
                    .rounded_full()
                    .bg(t.bg_selected_muted)
                    .child(
                        div()
                            .h_full()
                            .rounded_full()
                            .bg(t.accent)
                            .w(gpui::relative(percent / 100.)),
                    ),
            )
            .into_any_element()
    }
}

/// A line saying what is being waited for, beside the spinner.
fn working(what: &'static str, t: &Theme) -> AnyElement {
    div()
        .id(what)
        .role(Role::Status)
        .aria_label(what)
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(px(fs_sm()))
        .text_color(t.text_muted)
        .child(spinner(14., t.text_muted))
        .child(what)
        .into_any_element()
}

/// The box a step's choices scroll in.
fn list_box(
    id: &'static str,
    label: &'static str,
    scroll: &ScrollHandle,
    t: &Theme,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .role(Role::RadioGroup)
        .aria_label(label)
        .flex()
        .flex_col()
        .max_h(px(300.))
        .overflow_y_scroll()
        .track_scroll(scroll)
        .rounded(px(radius()))
        .border_1()
        .border_color(t.border)
        .p(px(4.))
}

/// A step's buttons, at its right edge.
fn buttons() -> gpui::Div {
    div().flex().justify_end().gap(px(8.))
}
