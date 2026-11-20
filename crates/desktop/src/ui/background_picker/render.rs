//! The picker drawn: its cards in a wrapping grid, the card that adds a picture, and the line
//! that takes a removed picture back.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    AnyElement, FocusHandle, FontWeight, KeyDownEvent, MouseButton, MouseDownEvent, ObjectFit,
    Role, SharedString, Window, canvas, div, img, prelude::*, px, relative,
};
use patchwork_core::backgrounds::ChatBackground;

use super::state::{BackgroundPicker, CARD, GAP, PICTURE, SHARED_VEIL};
use crate::ui::backdrop::{PRESETS, butterfly_preview, preset_fill, scene_preview};
use crate::ui::menu::Menu;
use crate::ui::theme::{Theme, applied, fs_sm, radius, radius_sm, theme};
use crate::ui::{button, focus_ring, icon, spinner, tooltip};

impl BackgroundPicker {
    /// The first choice: "No background", or the shared background under a veil.
    fn clear_choice(&self, focus: &FocusHandle, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let checked = self.clear_checked();
        let label = if self.cleared.is_some() {
            "No background"
        } else {
            "Same as all chats"
        };
        let shared = match (&self.cleared, &self.shared) {
            (None, Some(ChatBackground::Preset { id })) => {
                preset_fill(id, t).map(IntoElement::into_any_element)
            }
            (None, Some(ChatBackground::Image { file })) => self
                .images
                .iter()
                .flatten()
                .find(|i| &i.file == file)
                .map(|i| {
                    img(PathBuf::from(&i.path))
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .into_any_element()
                }),
            _ => None,
        };
        let cleared = self.cleared.clone();
        let fill = div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(t.bg_hover)
            .when_some(shared, |d, shared| {
                d.child(shared).child(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(t.bg_editor.opacity(SHARED_VEIL)),
                )
            })
            .child(div().relative().child(icon("prohibit", 26.)));
        swatch(
            "background-none".into(),
            label.into(),
            checked,
            focus,
            fill,
            t,
        )
        .on_click(cx.listener(move |p, _, _, cx| p.pick(cleared.clone(), cx)))
        .into_any_element()
    }
}

/// A choice of the picker: the card Settings draws for a background, with `fill` as its picture
/// and its name beneath.
fn swatch(
    id: SharedString,
    name: SharedString,
    checked: bool,
    focus: &FocusHandle,
    fill: impl IntoElement,
    t: &Theme,
) -> gpui::Stateful<gpui::Div> {
    let hover = t.bg_hover;
    div()
        .id(id)
        .role(Role::RadioButton)
        .aria_label(name.clone())
        .aria_toggled(checked.into())
        .track_focus(focus)
        .focus_visible(focus_ring)
        .cursor_pointer()
        .w(px(CARD))
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(GAP))
        .p(px(6.))
        .rounded(px(radius()))
        .border_1()
        .map(|card| {
            if checked {
                card.border_color(t.accent).bg(t.bg_accent_soft)
            } else {
                card.border_color(gpui::transparent_black())
                    .hover(move |s| s.bg(hover))
            }
        })
        .child(
            div()
                .relative()
                .h(px(PICTURE))
                .rounded(px(radius_sm()))
                .border_1()
                .border_color(t.border)
                .bg(t.bg_editor)
                .overflow_hidden()
                .child(fill),
        )
        .child(
            div()
                .h(px(22.))
                .flex()
                .items_center()
                .text_size(px(fs_sm()))
                .child(
                    div()
                        .id("name")
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .tooltip(tooltip(name.clone()))
                        .child(name),
                ),
        )
}

impl Render for BackgroundPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let value = self.value.clone();
        let (hover, muted, fg) = (t.bg_hover, t.text_muted, t.text);
        let choices = self.choices();
        while self.radios.len() < choices.len() {
            self.radios.push(cx.focus_handle());
        }
        // The picked choice is the group's one Tab stop, or the first when none shows as picked.
        let stop = if self.clear_checked() {
            0
        } else {
            choices
                .iter()
                .skip(1)
                .position(|c| *c == value)
                .map_or(0, |at| at + 1)
        };
        let radios: Vec<FocusHandle> = self
            .radios
            .iter()
            .enumerate()
            .map(|(i, f)| f.clone().tab_index(0).tab_stop(i == stop))
            .collect();
        let clear = self.clear_choice(&radios[0], &t, cx);
        let first_scene = 2;
        let first_preset = first_scene + self.scenes.len();
        let presets = PRESETS.iter().enumerate().map(|(n, &(id, name))| {
            let checked = matches!(&value, Some(ChatBackground::Preset { id: p }) if p == id);
            swatch(
                SharedString::from(format!("background-{id}")),
                name.into(),
                checked,
                &radios[first_preset + n],
                div().absolute().inset_0().children(preset_fill(id, &t)),
                &t,
            )
            .on_click(cx.listener(move |p, _, _, cx| {
                p.pick(Some(ChatBackground::Preset { id: id.to_string() }), cx);
            }))
        });
        let still = applied(cx).reduce_motion;
        let butterfly = swatch(
            "background-butterfly".into(),
            "Butterfly".into(),
            value == Some(ChatBackground::Butterfly),
            &radios[1],
            div()
                .absolute()
                .inset_0()
                .child(butterfly_preview(&self.flight, still)),
            &t,
        )
        .on_click(cx.listener(|p, _, _, cx| p.pick(Some(ChatBackground::Butterfly), cx)));
        let scenes = self.scenes.iter().enumerate().map(|(n, saved)| {
            let id = saved.id.clone();
            let checked = matches!(&value, Some(ChatBackground::Scene { id: s }) if *s == id);
            swatch(
                SharedString::from(format!("background-scene-{id}")),
                saved.file.name.clone().into(),
                checked,
                &radios[first_scene + n],
                div().absolute().inset_0().child(scene_preview(
                    Arc::clone(saved),
                    t.bg_editor,
                    t.text_muted,
                    still,
                )),
                &t,
            )
            .on_click(cx.listener(move |p, _, _, cx| {
                p.pick(Some(ChatBackground::Scene { id: id.clone() }), cx);
            }))
        });
        let first_picture = first_preset + PRESETS.len();
        let images = self.images.iter().flatten().enumerate().map(|(i, image)| {
            let name = image
                .name
                .clone()
                .unwrap_or_else(|| format!("Picture {}", i + 1));
            let checked =
                matches!(&value, Some(ChatBackground::Image { file }) if *file == image.file);
            let (pick, remove) = (image.file.clone(), image.file.clone());
            let group = SharedString::from(format!("picture-{i}"));
            let removed_name = name.clone();
            let at = first_picture + i;
            let clicked = (at, image.file.clone(), name.clone());
            let keyed = (image.file.clone(), name.clone());
            div()
                .group(group.clone())
                .relative()
                .child(
                    swatch(
                        group.clone(),
                        name.clone().into(),
                        checked,
                        &radios[at],
                        img(PathBuf::from(&image.path))
                            .absolute()
                            .inset_0()
                            .size_full()
                            .object_fit(ObjectFit::Cover),
                        &t,
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |p, e: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            p.open_menu(clicked.clone(), e.position, window, cx);
                        }),
                    )
                    .on_key_down(cx.listener(move |p, e: &KeyDownEvent, _, cx| {
                        if e.keystroke.key == "delete" {
                            cx.stop_propagation();
                            p.remove_picture(&keyed.0, keyed.1.clone(), cx);
                        }
                    }))
                    .on_click(cx.listener(move |p, _, _, cx| {
                        p.pick(Some(ChatBackground::Image { file: pick.clone() }), cx);
                    })),
                )
                .child(
                    button(
                        SharedString::from(format!("remove-picture-{i}")),
                        format!("Remove {name}"),
                    )
                    .absolute()
                    // On the corner of the card's picture.
                    .top(px(2.))
                    .right(px(2.))
                    .size(px(16.))
                    .justify_center()
                    .rounded_full()
                    .border_1()
                    .border_color(t.border_strong)
                    .bg(t.bg_elevated)
                    .text_color(muted)
                    .opacity(0.)
                    .group_hover(group, |s| s.opacity(1.))
                    .focus(|s| s.opacity(1.))
                    .hover(move |s| s.text_color(fg))
                    .tooltip(tooltip(format!("Remove {name}")))
                    .child(icon("x", 9.))
                    .on_click(cx.listener(move |p, _, _, cx| {
                        p.remove_picture(&remove, removed_name.clone(), cx);
                    })),
                )
        });
        let importing = self.importing;
        let width = Rc::clone(&self.grid_width);
        let measure = canvas(
            move |bounds, _, _| width.set(bounds.size.width.into()),
            |_, (), _, _| {},
        )
        .absolute()
        .inset_0();
        div()
            .on_key_down(cx.listener(Self::on_key))
            .child(
                div()
                    .id("chat-background")
                    .role(Role::RadioGroup)
                    .aria_label(self.label)
                    .relative()
                    .flex()
                    .flex_wrap()
                    .gap(px(GAP))
                    // Up and Down move a row of the grid, so its width is read as it is drawn.
                    .child(measure)
                    .child(clear)
                    .child(butterfly)
                    .children(scenes)
                    .children(presets)
                    .children(images)
                    .when(importing, |g| {
                        g.child(
                            div()
                                .id("adding-picture")
                                .role(Role::Status)
                                .aria_label("Adding the picture")
                                // The size and place of a card's picture.
                                .m(px(7.))
                                .w(px(CARD - 14.))
                                .h(px(PICTURE))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(radius_sm()))
                                .border_1()
                                .border_color(t.border)
                                .child(spinner(18., muted)),
                        )
                    })
                    .child(
                        // The same card as the one that adds a background in Settings.
                        button("choose-picture", "Add background")
                            .m(px(7.))
                            .w(px(CARD - 14.))
                            .h(px(PICTURE))
                            .flex_none()
                            .flex_col()
                            .justify_center()
                            .gap(px(6.))
                            .rounded(px(radius_sm()))
                            .border_1()
                            .border_dashed()
                            .border_color(t.border_strong)
                            .text_size(px(crate::ui::theme::fs_sm()))
                            .text_color(muted)
                            .child(icon("plus", 16.))
                            .child("Add background")
                            .map(|b| {
                                if importing {
                                    b.cursor_default()
                                } else {
                                    b.hover(move |s| s.bg(hover).text_color(t.text))
                                        .on_click(cx.listener(|p, _, _, cx| p.choose_picture(cx)))
                                }
                            }),
                    ),
            )
            .children(self.undo.as_ref().map(|(name, _)| {
                div()
                    .id("picture-removed")
                    .role(Role::Status)
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pt(px(8.))
                    .text_size(px(fs_sm()))
                    .text_color(muted)
                    .child(div().min_w_0().truncate().child(format!("Removed {name}")))
                    .child(
                        button("undo-remove-picture", "Undo")
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.accent_text)
                            .hover(gpui::Styled::underline)
                            .child("Undo")
                            .on_click(cx.listener(|p, _, _, cx| p.take_back(cx))),
                    )
            }))
            .children(self.error.clone().map(|e| {
                div()
                    .id("picker-error")
                    .role(Role::Alert)
                    .pt(px(8.))
                    .text_size(px(fs_sm()))
                    .text_color(t.danger)
                    .child(e)
            }))
            .children(
                self.menu
                    .as_ref()
                    .map(|(menu, at, _)| Menu::view(menu, *at)),
            )
            .line_height(relative(1.5))
    }
}
