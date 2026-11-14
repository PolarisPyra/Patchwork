//! The background picker, shared by Settings → General and a
//! chat's own background dialog: the default, the presets, and the pictures picked from disk,
//! each removable with a short while to take it back.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AnyElement, ClipboardEntry, EventEmitter, FocusHandle, FontWeight, KeyDownEvent, ObjectFit,
    Role, SharedString, Toggled, Window, div, img, prelude::*, px, relative,
};
use patchwork_server::Server;
use patchwork_server::backgrounds::{BackgroundImage, ChatBackground, RemovedBackground};

use crate::ui::backdrop::{PRESETS, preset_fill};
use crate::ui::theme::{fs_sm, radius_sm, Theme, theme};
use crate::ui::{button, focus_ring, icon, spinner, tooltip};

/// How long a deleted picture can be brought back from the picker.
const UNDO: Duration = Duration::from_secs(6);

const PICTURE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "gif"];

/// The grid's columns, which Up and Down move across.
const COLUMNS: usize = 5;

/// How much of the window color covers the shared background on "Same as all chats".
const SHARED_VEIL: f32 = 0.55;

pub enum PickerEvent {
    /// A choice was picked; None is "Same as all chats".
    Picked(Option<ChatBackground>),
    /// A picture was removed or brought back, which moves the backgrounds that showed it.
    PicturesChanged,
}

pub struct BackgroundPicker {
    server: Arc<Server>,
    label: &'static str,
    value: Option<ChatBackground>,
    /// What the first choice picks: "No background" in Settings, and for one chat None, the
    /// background all chats share.
    cleared: Option<ChatBackground>,
    /// The background all chats share, shown on the first choice when it picks None.
    shared: Option<ChatBackground>,
    images: Option<Vec<BackgroundImage>>,
    importing: bool,
    /// The picture just removed, by name, with what takes it back.
    undo: Option<(String, RemovedBackground)>,
    error: Option<String>,
    /// One per choice, in order, for the arrow keys to move the focus along.
    radios: Vec<FocusHandle>,
}

impl EventEmitter<PickerEvent> for BackgroundPicker {}

fn is_picture(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, ext)| PICTURE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
}

impl BackgroundPicker {
    pub fn new(
        server: Arc<Server>,
        label: &'static str,
        value: Option<ChatBackground>,
        cleared: Option<ChatBackground>,
        shared: Option<ChatBackground>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            server,
            label,
            value,
            cleared,
            shared,
            images: None,
            importing: false,
            undo: None,
            error: None,
            radios: Vec::new(),
        };
        this.load_images(cx);
        this
    }

    /// Follows the value and the shared background as they are saved elsewhere.
    pub fn set_value(&mut self, value: Option<ChatBackground>, shared: Option<ChatBackground>) {
        self.value = value;
        self.shared = shared;
    }

    /// Shows why the last pick was not saved, with the value as it stands.
    pub fn failed(&mut self, value: Option<ChatBackground>, error: String, cx: &mut Context<Self>) {
        self.value = value;
        self.error = Some(error);
        cx.notify();
    }

    fn load_images(&mut self, cx: &mut Context<Self>) {
        let server = Arc::clone(&self.server);
        let task = cx.background_spawn(async move { server.list_background_images() });
        cx.spawn(async move |this, cx| {
            let images = task.await;
            let _ = this.update(cx, |p, cx| {
                match images {
                    Ok(images) => p.images = Some(images),
                    Err(e) => p.error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Lists the pictures again and picks `file`, a picture saved from outside the picker.
    pub fn added(&mut self, file: String, cx: &mut Context<Self>) {
        self.load_images(cx);
        cx.emit(PickerEvent::PicturesChanged);
        self.pick(Some(ChatBackground::Image { file }), cx);
    }

    fn pick(&mut self, background: Option<ChatBackground>, cx: &mut Context<Self>) {
        self.error = None;
        self.value.clone_from(&background);
        cx.emit(PickerEvent::Picked(background));
        cx.notify();
    }

    /// Asks for a picture, copies it into Patchwork's folder, and picks it.
    fn choose_picture(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths
                .first()
                .map(|p| p.to_string_lossy().into_owned())
                .filter(|p| is_picture(p))
            else {
                return;
            };
            let _ = this.update(cx, |p, cx| {
                p.add(
                    move |s| s.import_background_image(&path).map(|f| vec![f]),
                    cx,
                );
            });
        })
        .detach();
    }

    /// Ctrl+V: a picture copied in a file manager comes as its path, a copied image as its pixels.
    fn paste(&mut self, cx: &mut Context<Self>) {
        let (mut paths, mut pixels) = (Vec::new(), None);
        for entry in cx
            .read_from_clipboard()
            .iter()
            .flat_map(gpui::ClipboardItem::entries)
        {
            match entry {
                ClipboardEntry::Image(image) => pixels = Some(image.bytes.clone()),
                ClipboardEntry::String(s) => paths.extend(
                    s.text()
                        .lines()
                        .map(|l| l.trim().trim_start_matches("file://").to_string())
                        .filter(|p| is_picture(p) && std::path::Path::new(p).is_file()),
                ),
                ClipboardEntry::ExternalPaths(_) => {}
            }
        }
        self.add(
            move |s| match pixels {
                _ if !paths.is_empty() => {
                    paths.iter().map(|p| s.import_background_image(p)).collect()
                }
                Some(bytes) => s.import_background_bytes(bytes, None).map(|f| vec![f]),
                None => Err("The clipboard holds no picture".into()),
            },
            cx,
        );
    }

    /// Saves the pictures `load` brings in, off the main thread, and picks the last.
    fn add(
        &mut self,
        load: impl FnOnce(&Server) -> Result<Vec<String>, patchwork_server::Error> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.importing = true;
        self.error = None;
        cx.notify();
        let server = Arc::clone(&self.server);
        let task = cx.background_spawn(async move { load(&server) });
        cx.spawn(async move |this, cx| {
            let added = task.await;
            let _ = this.update(cx, |p, cx| {
                p.importing = false;
                match added {
                    Ok(files) => {
                        p.load_images(cx);
                        cx.emit(PickerEvent::PicturesChanged);
                        if let Some(file) = files.last() {
                            p.pick(Some(ChatBackground::Image { file: file.clone() }), cx);
                        }
                    }
                    Err(e) => p.error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// What each choice picks, in order: the first choice, the presets, then the pictures.
    fn choices(&self) -> Vec<Option<ChatBackground>> {
        std::iter::once(self.cleared.clone())
            .chain(
                PRESETS
                    .iter()
                    .map(|&(id, _)| Some(ChatBackground::Preset { id: id.to_string() })),
            )
            .chain(self.images.iter().flatten().map(|i| {
                Some(ChatBackground::Image {
                    file: i.file.clone(),
                })
            }))
            .collect()
    }

    /// The choices are one stop for Tab; the arrow keys move between them and pick, as in any
    /// group of radio buttons, with Up and Down moving a row of the grid.
    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &e.keystroke;
        if ks.key == "v" && (ks.modifiers.control || ks.modifiers.platform) {
            cx.stop_propagation();
            return self.paste(cx);
        }
        let Some(at) = self.radios.iter().position(|f| f.is_focused(window)) else {
            return;
        };
        let choices = self.choices();
        let last = choices.len().saturating_sub(1);
        let next = match ks.key.as_str() {
            "right" => at + 1,
            "left" => at.saturating_sub(1),
            "down" => at + COLUMNS,
            "up" => at.saturating_sub(COLUMNS),
            "home" => 0,
            "end" => last,
            _ => return,
        }
        .min(last);
        cx.stop_propagation();
        if let (Some(focus), Some(choice)) = (self.radios.get(next), choices.get(next)) {
            window.focus(focus, cx);
            self.pick(choice.clone(), cx);
        }
    }

    /// Whether the first choice is the one picked.
    fn clear_checked(&self) -> bool {
        match (&self.cleared, &self.value) {
            (None, value) => value.is_none(),
            (Some(cleared), Some(value)) => {
                std::mem::discriminant(cleared) == std::mem::discriminant(value)
            }
            (Some(_), None) => false,
        }
    }

    /// Deletes a picture, and moves whatever showed it back to its default, for a while
    /// offering to take that back.
    fn remove_picture(&mut self, file: &str, name: String, cx: &mut Context<Self>) {
        self.error = None;
        match self.server.remove_background_image(file) {
            Ok(removed) => {
                self.undo = Some((name.clone(), removed));
                self.load_images(cx);
                cx.emit(PickerEvent::PicturesChanged);
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(UNDO).await;
                    let _ = this.update(cx, |p, cx| {
                        if p.undo.as_ref().is_some_and(|(n, _)| *n == name) {
                            p.undo = None;
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    fn take_back(&mut self, cx: &mut Context<Self>) {
        let Some((_, removed)) = self.undo.take() else {
            return;
        };
        match self.server.restore_background_image(&removed) {
            Ok(()) => {
                self.load_images(cx);
                cx.emit(PickerEvent::PicturesChanged);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

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
        let (selected_muted, fg) = (t.bg_selected_muted, t.text);
        let cleared = self.cleared.clone();
        div()
            .id("background-none")
            .role(Role::RadioButton)
            .aria_label(label)
            .aria_toggled(if checked {
                Toggled::True
            } else {
                Toggled::False
            })
            .track_focus(focus)
            .focus_visible(focus_ring)
            .cursor_pointer()
            .relative()
            .overflow_hidden()
            .aspect_ratio(4. / 3.)
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(radius_sm()))
            .border_2()
            .border_color(if checked {
                t.accent
            } else {
                gpui::transparent_black()
            })
            .bg(t.bg_hover)
            .text_color(fg)
            .hover(move |s| s.bg(selected_muted))
            .tooltip(tooltip(label))
            .when_some(shared, |d, shared| {
                d.child(shared).child(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(t.bg_editor.opacity(SHARED_VEIL)),
                )
            })
            .child(div().relative().child(icon("prohibit", 26.)))
            .on_click(cx.listener(move |p, _, _, cx| p.pick(cleared.clone(), cx)))
            .into_any_element()
    }
}

/// A choice of the picker: a 4:3 tile, outlined when it is the one picked.
fn swatch(
    id: SharedString,
    name: SharedString,
    checked: bool,
    focus: &FocusHandle,
    t: &Theme,
) -> gpui::Stateful<gpui::Div> {
    let strong = t.border_strong;
    div()
        .id(id)
        .role(Role::RadioButton)
        .aria_label(name.clone())
        .aria_toggled(if checked {
            Toggled::True
        } else {
            Toggled::False
        })
        .track_focus(focus)
        .focus_visible(focus_ring)
        .cursor_pointer()
        .relative()
        .aspect_ratio(4. / 3.)
        .overflow_hidden()
        .rounded(px(radius_sm()))
        .border_2()
        .bg(t.bg_editor)
        .border_color(if checked { t.accent } else { t.border })
        .when(!checked, |s| s.hover(move |s| s.border_color(strong)))
        .tooltip(tooltip(name))
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
        let presets = PRESETS.iter().enumerate().map(|(n, &(id, name))| {
            let checked = matches!(&value, Some(ChatBackground::Preset { id: p }) if p == id);
            swatch(
                SharedString::from(format!("background-{id}")),
                name.into(),
                checked,
                &radios[1 + n],
                &t,
            )
            .children(preset_fill(id, &t))
            .on_click(cx.listener(move |p, _, _, cx| {
                p.pick(Some(ChatBackground::Preset { id: id.to_string() }), cx);
            }))
        });
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
            div()
                .group(group.clone())
                .relative()
                .child(
                    swatch(
                        group.clone(),
                        name.clone().into(),
                        checked,
                        &radios[1 + PRESETS.len() + i],
                        &t,
                    )
                    .child(
                        img(PathBuf::from(&image.path))
                            .absolute()
                            .inset_0()
                            .size_full()
                            .object_fit(ObjectFit::Cover),
                    )
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
                    .top(px(-6.))
                    .right(px(-6.))
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
        div()
            .on_key_down(cx.listener(Self::on_key))
            .child(
                div()
                    .id("chat-background")
                    .role(Role::RadioGroup)
                    .aria_label(self.label)
                    .grid()
                    .grid_cols(5)
                    .gap(px(8.))
                    .child(clear)
                    .children(presets)
                    .children(images)
                    .when(importing, |g| {
                        g.child(
                            div()
                                .id("adding-picture")
                                .role(Role::Status)
                                .aria_label("Adding the picture")
                                .aspect_ratio(4. / 3.)
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(radius_sm()))
                                .border_2()
                                .border_color(t.border)
                                .child(spinner(18., muted)),
                        )
                    })
                    .child(
                        button("choose-picture", "Choose a picture")
                            .aspect_ratio(4. / 3.)
                            .justify_center()
                            .rounded(px(radius_sm()))
                            .border_1()
                            .border_dashed()
                            .border_color(t.border_strong)
                            .text_color(muted)
                            .tooltip(tooltip("Choose a picture"))
                            .child(icon("image", 18.))
                            .map(|b| {
                                if importing {
                                    b.cursor_default()
                                } else {
                                    b.hover(move |s| s.bg(hover))
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
            .line_height(relative(1.5))
    }
}
