//! The picker's state: what it lists, what is picked, and the keys and menus that change them.

// Rust guideline compliant 2026-10-09

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    ClipboardEntry, Entity, EventEmitter, FocusHandle, KeyDownEvent, Pixels, Point, Window,
    prelude::*,
};
use patchwork_core::Core;
use patchwork_core::backgrounds::{BackgroundImage, ChatBackground, RemovedBackground};
use patchwork_core::themes::SavedScene;

use crate::backdrop::{PRESETS, PreviewFlight};
use crate::controls::menu::{Menu, MenuItem};

/// How long a deleted picture can be brought back from the picker.
const UNDO: Duration = Duration::from_secs(6);

const PICTURE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "gif"];

/// A card's width and the height of its picture, the same as a background's card in Settings.
pub(super) const CARD: f32 = 150.;
pub(super) const PICTURE: f32 = 64.;
/// The space between cards.
pub(super) const GAP: f32 = 4.;

/// How much of the window color covers the shared background on "Same as all chats".
pub(super) const SHARED_VEIL: f32 = 0.55;

#[derive(Debug)]
pub enum PickerEvent {
    /// A choice was picked; None is "Same as all chats".
    Picked(Option<ChatBackground>),
    /// A picture was removed or brought back, which moves the backgrounds that showed it.
    PicturesChanged,
}

#[derive(Debug)]
pub struct BackgroundPicker {
    pub(super) core: Arc<Core>,
    pub(super) label: &'static str,
    pub(super) value: Option<ChatBackground>,
    /// What the first choice picks: "No background" in Settings, and for one chat None, the
    /// background all chats share.
    pub(super) cleared: Option<ChatBackground>,
    /// The background all chats share, shown on the first choice when it picks None.
    pub(super) shared: Option<ChatBackground>,
    pub(super) images: Option<Vec<BackgroundImage>>,
    /// The animated backgrounds, read with the pictures.
    pub(super) scenes: Vec<Arc<SavedScene>>,
    /// Where the butterfly on its choice is, kept between frames.
    pub(super) flight: PreviewFlight,
    pub(super) importing: bool,
    /// The picture just removed, by name, with what takes it back.
    pub(super) undo: Option<(String, RemovedBackground)>,
    pub(super) error: Option<String>,
    /// One per choice, in order, for the arrow keys to move the focus along.
    pub(super) radios: Vec<FocusHandle>,
    /// The menu a right click or the menu key opened on a picture, where it opened, and the
    /// picture's place among the choices, which takes the focus when the menu closes.
    pub(super) menu: Option<(Entity<Menu>, Point<Pixels>, usize)>,
    /// How wide the grid of choices was last drawn, so Up and Down can move a row.
    pub(super) grid_width: Rc<Cell<f32>>,
}

impl EventEmitter<PickerEvent> for BackgroundPicker {}

fn is_picture(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, ext)| PICTURE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
}

impl BackgroundPicker {
    pub fn new(
        core: Arc<Core>,
        label: &'static str,
        value: Option<ChatBackground>,
        cleared: Option<ChatBackground>,
        shared: Option<ChatBackground>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            core,
            label,
            value,
            cleared,
            shared,
            images: None,
            scenes: Vec::new(),
            flight: PreviewFlight::default(),
            importing: false,
            undo: None,
            error: None,
            radios: Vec::new(),
            menu: None,
            grid_width: Rc::default(),
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
        let core = Arc::clone(&self.core);
        let task =
            cx.background_spawn(async move { (core.list_background_images(), core.list_scenes()) });
        cx.spawn(async move |this, cx| {
            let (images, scenes) = task.await;
            let _ = this.update(cx, |p, cx| {
                p.scenes = scenes.into_iter().map(Arc::new).collect();
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

    pub(super) fn pick(&mut self, background: Option<ChatBackground>, cx: &mut Context<Self>) {
        self.error = None;
        self.value.clone_from(&background);
        cx.emit(PickerEvent::Picked(background));
        cx.notify();
    }

    /// Asks for a picture, copies it into Patchwork's folder, and picks it.
    pub(super) fn choose_picture(&mut self, cx: &mut Context<Self>) {
        crate::pick_paths(
            cx,
            gpui::PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: None,
            },
            move |p, paths, cx| {
                let Some(path) = paths
                    .first()
                    .map(|p| p.to_string_lossy().into_owned())
                    .filter(|p| is_picture(p))
                else {
                    return;
                };

                p.add(
                    move |s| s.import_background_image(&path).map(|f| vec![f]),
                    cx,
                );
            },
        );
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
        load: impl FnOnce(&Core) -> Result<Vec<String>, patchwork_core::Error> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.importing = true;
        self.error = None;
        cx.notify();
        let core = Arc::clone(&self.core);
        crate::off_thread(
            cx,
            move || load(&core),
            move |p, added, cx| {
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
            },
        );
    }

    /// What each choice picks, in order: the first choice, the presets, the butterfly, the
    /// animated backgrounds, then the pictures.
    pub(super) fn choices(&self) -> Vec<Option<ChatBackground>> {
        // The order of the Orchestrator's backgrounds in Settings.
        std::iter::once(self.cleared.clone())
            .chain(std::iter::once(Some(ChatBackground::Butterfly)))
            .chain(
                self.scenes
                    .iter()
                    .map(|s| Some(ChatBackground::Scene { id: s.id.clone() })),
            )
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
    pub(super) fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
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
        // The cards wrap to the width they are given, which sets how many make a row.
        let columns = ((self.grid_width.get() + GAP) / (CARD + GAP))
            .floor()
            .max(1.) as usize;
        let next = match ks.key.as_str() {
            "right" => at + 1,
            "left" => at.saturating_sub(1),
            // Down from a row above a shorter last one goes to the last card.
            "down" if at + columns <= last => at + columns,
            "down" if last / columns > at / columns => last,
            "down" => at,
            "up" => at.checked_sub(columns).unwrap_or(at),
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
    pub(super) fn clear_checked(&self) -> bool {
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
    pub fn remove_picture(&mut self, file: &str, name: String, cx: &mut Context<Self>) {
        self.error = None;
        match self.core.remove_background_image(file) {
            Ok(removed) => {
                self.undo = Some((name.clone(), removed));
                self.load_images(cx);
                cx.emit(PickerEvent::PicturesChanged);
                crate::after(cx, UNDO, move |p, cx| {
                    if p.undo.as_ref().is_some_and(|(n, _)| *n == name) {
                        p.undo = None;
                        cx.notify();
                    }
                });
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    /// Opens the menu of the picture at `at` among the choices: its one row deletes it.
    pub(super) fn open_menu(
        &mut self,
        (at, file, name): (usize, String, String),
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let picker = cx.entity().downgrade();
        let removed = name.clone();
        let delete = MenuItem::new("Delete", move |_, cx| {
            let _ = picker.update(cx, |p, cx| p.remove_picture(&file, removed.clone(), cx));
        })
        .icon("trash")
        .danger();
        let menu = Menu::open_for(
            name,
            vec![delete],
            window,
            cx,
            |p: &mut Self, window, cx| {
                // The picture may be gone, and then the choice now in its place takes the focus.
                if let Some((_, _, at)) = p.menu.take() {
                    let focus = p.radios.get(at).or_else(|| p.radios.first());
                    if let Some(focus) = focus {
                        window.focus(focus, cx);
                    }
                }
                cx.notify();
            },
        );
        self.menu = Some((menu, position, at));
        cx.notify();
    }

    pub(super) fn take_back(&mut self, cx: &mut Context<Self>) {
        let Some((_, removed)) = self.undo.take() else {
            return;
        };
        match self.core.restore_background_image(&removed) {
            Ok(()) => {
                self.load_images(cx);
                cx.emit(PickerEvent::PicturesChanged);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }
}
