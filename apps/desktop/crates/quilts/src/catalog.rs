//! The Quilts page's state: every quilt as last read, the gallery's search, filter and
//! pictures, and the quilt opened, its versions, the version shown, and the page that draws
//! it.

// Rust guideline compliant 2026-10-09

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    App, Bounds, ClipboardItem, Entity, EventEmitter, FocusHandle, Pixels, Point, ScrollHandle,
    UniformListScrollHandle, prelude::*,
};
use patchwork_backend::Core;
use patchwork_backend::quilts::{Quilt, QuiltKind, QuiltVersion, ChangeRow, quilt_changes};

use super::thumbs::Thumbs;
use patchwork_desktop_browser::{BrowserView, ElementPicked, PageErrorsChanged, PickingChanged};
use patchwork_ui_components::menu::Menu;
use patchwork_ui_theme::off_thread;
use patchwork_ui_editor::{Changed, TextArea};

/// What the page asks the window to do.
#[derive(Debug)]
pub enum QuiltsEvent {
    /// Open the chat a quilt came from.
    OpenChat(String),
    /// Send `text` to the chat `chat_id`, which made the quilt.
    AskFix { chat_id: String, text: String },
    /// Write `text`, about an element picked on the quilt, into the chat `chat_id`'s composer,
    /// for the person to finish and send.
    Picked { chat_id: String, text: String },
    /// Close the panel beside the chat.
    Closed,
    /// Show version `n` of the quilt `id` on the Quilts page instead of beside the chat.
    OpenFull { id: String, n: u32 },
    /// The page went to the quilt `id`, or back to the gallery, for the window's history.
    Shown(Option<String>),
}

/// What the viewer shows of the version.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Show {
    /// What it draws.
    Preview,
    /// Its source.
    Code,
    /// What changed since the version before it.
    Changes,
}

/// Where the quilts the gallery shows were made.
#[derive(Clone, PartialEq, Eq)]
pub(super) enum Place {
    All,
    /// The chat open when the page opened.
    Chat,
    /// A project's chats, by the project's id.
    Project(String),
}

pub struct QuiltsPage {
    pub(super) core: Arc<Core>,
    /// Every quilt, the most recently changed first.
    pub(super) list: Vec<Quilt>,
    /// Each chat's title, by its id, for the chat a quilt came from.
    pub(super) chats: HashMap<String, String>,
    /// Each chat's project, by the chat's id: the project's id and name.
    pub(super) projects: HashMap<String, (String, String)>,
    /// The chat open when the page opened, by its id.
    pub(super) came_from: Option<String>,
    pub(super) place: Place,
    pub(super) search: Entity<TextArea>,
    /// The kind the gallery shows, or every kind.
    pub(super) filter: Option<QuiltKind>,
    /// Whether the gallery shows cards rather than rows.
    pub(super) grid: bool,
    /// The versions' pictures, which the chats' cards share.
    pub(super) thumbs: Entity<Thumbs>,
    /// The quilt opened in the viewer; the gallery shows while there is none.
    pub(super) selected: Option<String>,
    /// The picked quilt's versions, the oldest first.
    pub(super) versions: Vec<QuiltVersion>,
    /// The number of the version shown.
    pub(super) shown: Option<u32>,
    pub(super) show: Show,
    /// What changed in the version shown since the one before it.
    pub(super) changes: Vec<ChangeRow>,
    pub(super) preview: Option<Entity<BrowserView>>,
    pub(super) error: Option<String>,
    /// A quilt's menu and where it shows.
    pub(super) menu: Option<(Entity<Menu>, Point<Pixels>)>,
    /// The field in place of the opened quilt's title while it is renamed.
    pub(super) renaming: Option<Entity<TextArea>>,
    /// Whether this is the panel beside a chat, which shows one quilt and no gallery.
    pub(super) docked: bool,
    /// The chat the window shows over the page, by its id.
    pub(super) chat_shown: Option<String>,
    pub(super) focus: FocusHandle,
    pub(super) gallery_scroll: ScrollHandle,
    pub(super) code_scroll: ScrollHandle,
    pub(super) changes_scroll: UniformListScrollHandle,
    /// What the mascot is doing, and since when.
    pub(super) patch: Option<(super::patch::Act, std::time::Instant)>,
    /// Where each quilt's card or row was last drawn, so a kind's tile can scroll to it.
    pub(super) spots: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
    /// The kind whose quilts are lit after its tile was picked, and since when.
    pub(super) flash: Option<(QuiltKind, std::time::Instant)>,
}

impl QuiltsPage {
    /// The page, which knows the chat `came_from` that was open when it opened.
    pub fn new(
        core: Arc<Core>,
        thumbs: Entity<Thumbs>,
        came_from: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| {
            let mut field = TextArea::new("Search quilts", "Search quilts", cx);
            field.single_line = true;
            field.placeholder_color = Some(patchwork_ui_theme::input_placeholder());
            field
        });
        cx.subscribe(&search, |_, _, _: &Changed, cx| cx.notify())
            .detach();
        cx.observe(&thumbs, |_, _, cx| cx.notify()).detach();
        let mut page = Self {
            core,
            list: Vec::new(),
            chats: HashMap::new(),
            projects: HashMap::new(),
            came_from,
            place: Place::All,
            search,
            filter: None,
            grid: true,
            thumbs,
            selected: None,
            versions: Vec::new(),
            shown: None,
            show: Show::Preview,
            changes: Vec::new(),
            preview: None,
            error: None,
            menu: None,
            renaming: None,
            docked: false,
            chat_shown: None,
            focus: cx.focus_handle(),
            gallery_scroll: ScrollHandle::new(),
            code_scroll: ScrollHandle::new(),
            changes_scroll: UniformListScrollHandle::new(),
            patch: None,
            spots: Rc::default(),
            flash: None,
        };
        page.reload(cx);
        page
    }

    /// The panel beside a chat, showing version `n` of the quilt `id`.
    pub fn beside_chat(
        core: Arc<Core>,
        thumbs: Entity<Thumbs>,
        id: String,
        n: u32,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut page = Self::new(core, thumbs, None, cx);
        page.docked = true;
        page.open(id, n, cx);
        page
    }

    /// Reads the quilts again, and the opened one's versions, following its newest when that
    /// was the one shown. Goes back to the gallery when the opened one is gone.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
        off_thread(
            cx,
            move || {
                let named: HashMap<String, String> = core
                    .settings()
                    .projects
                    .into_iter()
                    .map(|p| (p.id, p.name))
                    .collect();
                let all = core.list_chats();
                let chats = all
                    .iter()
                    .map(|t| (t.id.clone(), t.title.clone()))
                    .collect();
                let projects = all
                    .iter()
                    .filter_map(|t| {
                        let id = t.project_id.as_ref()?;
                        Some((t.id.clone(), (id.clone(), named.get(id)?.clone())))
                    })
                    .collect();
                (core.quilts(), chats, projects)
            },
            |this, (list, chats, projects), cx| {
                this.list = list;
                this.chats = chats;
                this.projects = projects;
                if this.show == Show::Preview && this.current().is_some_and(|a| !a.kind.drawn()) {
                    this.show = Show::Code;
                }
                if this.current().is_some() {
                    this.load_versions(cx);
                } else {
                    this.close(cx);
                }
                let dark = patchwork_ui_theme::theme(cx).is_dark();
                this.thumbs.update(cx, |thumbs, cx| {
                    for a in this.list.iter().filter(|a| a.kind.drawn()) {
                        thumbs.want(&a.id, a.versions, dark, cx);
                    }
                });
                cx.notify();
            },
        );
    }

    /// Shows only quilts of `kind`, or all of them.
    pub(super) fn set_filter(&mut self, kind: Option<QuiltKind>, cx: &mut Context<Self>) {
        self.filter = kind;
        cx.notify();
    }

    /// Shows only the quilts made in `place`.
    pub(super) fn set_place(&mut self, place: Place, cx: &mut Context<Self>) {
        self.place = place;
        cx.notify();
    }

    /// Shows the gallery as cards or as rows.
    pub(super) fn set_grid(&mut self, grid: bool, cx: &mut Context<Self>) {
        self.grid = grid;
        cx.notify();
    }

    /// Asks the window to open the chat the opened quilt came from.
    pub(super) fn open_chat(&mut self, cx: &mut Context<Self>) {
        if let Some(quilt) = self.current() {
            cx.emit(QuiltsEvent::OpenChat(quilt.chat_id.clone()));
        }
    }

    /// Notes the chat the window shows over the page, if any.
    pub fn set_chat_shown(&mut self, shown: Option<String>, cx: &mut Context<Self>) {
        self.chat_shown = shown;
        cx.notify();
    }

    /// Asks the window to show the opened quilt on the Quilts page.
    pub(super) fn open_full(&mut self, cx: &mut Context<Self>) {
        if let (Some(id), Some(n)) = (self.selected.clone(), self.shown) {
            cx.emit(QuiltsEvent::OpenFull { id, n });
        }
    }

    /// Goes back to the gallery, ending the opened quilt's browser; beside a chat, closes the
    /// panel.
    pub(super) fn close(&mut self, cx: &mut Context<Self>) {
        if self.docked {
            cx.emit(QuiltsEvent::Closed);
        } else {
            cx.emit(QuiltsEvent::Shown(None));
        }
        self.selected = None;
        self.versions.clear();
        self.shown = None;
        self.preview = None;
        self.error = None;
        self.renaming = None;
        cx.notify();
    }

    /// The quilt picked.
    pub(super) fn current(&self) -> Option<&Quilt> {
        let id = self.selected.as_ref()?;
        self.list.iter().find(|a| &a.id == id)
    }

    /// The version shown.
    pub(super) fn version(&self) -> Option<&QuiltVersion> {
        let n = self.shown?;
        self.versions.iter().find(|v| v.n == n)
    }

    /// The quilts the filters and the search match, by title, type or chat.
    pub(super) fn matching(&self, cx: &App) -> Vec<&Quilt> {
        let typed = self.search.read(cx).content.trim().to_lowercase();
        self.list
            .iter()
            .filter(|a| self.filter.is_none_or(|k| a.kind == k))
            .filter(|a| match &self.place {
                Place::All => true,
                Place::Chat => self.came_from.as_ref() == Some(&a.chat_id),
                Place::Project(id) => self.projects.get(&a.chat_id).is_some_and(|(p, _)| p == id),
            })
            .filter(|a| {
                typed.is_empty()
                    || a.title.to_lowercase().contains(&typed)
                    || a.kind.label().to_lowercase().contains(&typed)
                    || self
                        .chats
                        .get(&a.chat_id)
                        .is_some_and(|c| c.to_lowercase().contains(&typed))
            })
            .collect()
    }

    /// Opens the quilt `id` and shows its newest version.
    pub(super) fn select(&mut self, id: String, cx: &mut Context<Self>) {
        if !self.docked {
            cx.emit(QuiltsEvent::Shown(Some(id.clone())));
        }
        self.selected = Some(id);
        self.renaming = None;
        self.versions.clear();
        self.shown = None;
        self.error = None;
        // Before the list is read the kind is not known; the reading puts right a source shown
        // as a preview.
        self.show = if self.current().is_some_and(|a| !a.kind.drawn()) {
            Show::Code
        } else {
            Show::Preview
        };
        self.changes.clear();
        self.load_versions(cx);
        cx.notify();
    }

    /// Opens the quilt `id` on its version `n`, as a chat's card asks.
    pub fn open(&mut self, id: String, n: u32, cx: &mut Context<Self>) {
        if self.selected.as_ref() == Some(&id) {
            self.show(n, cx);
        } else {
            self.select(id, cx);
            self.shown = Some(n);
        }
    }

    /// Opens the quilt `open` on its newest version, or goes back to the gallery, unless the
    /// page already shows it, as the window's back and forward ask.
    pub fn go(&mut self, open: Option<String>, cx: &mut Context<Self>) {
        if self.selected == open {
            return;
        }
        match open {
            Some(id) => self.select(id, cx),
            None => self.close(cx),
        }
    }

    /// Reads the picked quilt's versions. The newest is shown when it was before, or when the
    /// one shown is gone.
    fn load_versions(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        let core = Arc::clone(&self.core);
        let asked = id.clone();
        off_thread(
            cx,
            move || core.quilt_versions(&id),
            move |this, versions, cx| {
                if this.selected.as_ref() != Some(&asked) {
                    return;
                }
                let newest_shown =
                    this.shown.is_none() || this.shown == this.versions.last().map(|v| v.n);
                let still_there = versions.iter().any(|v| Some(v.n) == this.shown);
                let changed = versions.len() != this.versions.len();
                this.versions = versions;
                if newest_shown || !still_there {
                    this.shown = this.versions.last().map(|v| v.n);
                }
                if changed || this.preview.is_none() {
                    this.draw(cx);
                    this.compare(cx);
                }
                cx.notify();
            },
        );
    }

    /// Shows version `n` of the picked quilt.
    pub(super) fn show(&mut self, n: u32, cx: &mut Context<Self>) {
        if self.shown == Some(n) {
            return;
        }
        self.shown = Some(n);
        self.draw(cx);
        self.compare(cx);
        cx.notify();
    }

    /// Steps `by` versions from the one shown, stopping at the first and the newest.
    pub(super) fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let at = self
            .shown
            .and_then(|n| self.versions.iter().position(|v| v.n == n))
            .unwrap_or_default();
        let last = self.versions.len().saturating_sub(1);
        let to = at.saturating_add_signed(by).min(last);
        if let Some(n) = self.versions.get(to).map(|v| v.n) {
            self.show(n, cx);
        }
    }

    /// Shows what the version draws, its source, or what changed in it.
    pub(super) fn set_show(&mut self, show: Show, cx: &mut Context<Self>) {
        self.show = show;
        self.draw(cx);
        self.compare(cx);
        cx.notify();
    }

    /// Works out what changed in the version shown since the one before it, while the changes
    /// show. The first version is all added.
    fn compare(&mut self, cx: &mut Context<Self>) {
        if self.show != Show::Changes {
            return;
        }
        let Some(at) = self
            .shown
            .and_then(|n| self.versions.iter().position(|v| v.n == n))
        else {
            return;
        };
        let new = self.versions[at].content.clone();
        let old = at
            .checked_sub(1)
            .map(|i| self.versions[i].content.clone())
            .unwrap_or_default();
        let asked = (self.selected.clone(), self.shown);
        off_thread(
            cx,
            move || quilt_changes(&old, &new),
            move |this, rows, cx| {
                if (this.selected.clone(), this.shown) == asked {
                    this.changes = rows;
                    cx.notify();
                }
            },
        );
    }

    /// Writes the version shown as a page and opens it in the preview.
    fn draw(&mut self, cx: &mut Context<Self>) {
        let (Some(quilt), Some(n)) = (self.current(), self.shown) else {
            return;
        };
        if self.show != Show::Preview || !quilt.kind.drawn() {
            return;
        }
        let (id, title) = (quilt.id.clone(), quilt.title.clone());
        let core = Arc::clone(&self.core);
        off_thread(
            cx,
            move || core.quilt_page(&id, n),
            move |this, page, cx| {
                match page {
                    Ok(url) => {
                        if let Some(view) = &this.preview {
                            view.update(cx, |view, cx| {
                                view.relabel(title);
                                view.open(&url, cx);
                            });
                        } else {
                            let view = cx.new(|cx| BrowserView::new(url, title, false, cx));
                            cx.subscribe(&view, |_, _, _: &PageErrorsChanged, cx| cx.notify())
                                .detach();
                            cx.subscribe(&view, |_, _, _: &PickingChanged, cx| cx.notify())
                                .detach();
                            cx.subscribe(&view, |this, _, picked: &ElementPicked, cx| {
                                this.picked(picked, cx);
                            })
                            .detach();
                            this.preview = Some(view);
                        }
                    }
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            },
        );
    }

    /// Asks where to save the version shown's source, and saves it there.
    pub(super) fn save_as(&mut self, cx: &mut Context<Self>) {
        let (Some(quilt), Some(version)) = (self.current(), self.version()) else {
            return;
        };
        let name = format!(
            "{}.{}",
            quilt.key,
            quilt.kind.extension(quilt.language.as_deref())
        );
        let text = version.content.clone();
        let home = patchwork_platform::files::paths::home_dir().unwrap_or_default();
        let picked = cx.prompt_for_new_path(&home, Some(&name));
        let core = Arc::clone(&self.core);
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let path = path.to_string_lossy().into_owned();
            let saved = cx
                .background_spawn(async move { core.save_text_file(&path, &text) })
                .await;
            if let Err(e) = saved {
                let _ = this.update(cx, |this, cx| {
                    this.error = Some(e.to_string());
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// The errors the page shown met.
    pub(super) fn page_errors<'a>(&self, cx: &'a App) -> &'a [String] {
        match (&self.preview, self.show) {
            (Some(view), Show::Preview) => view.read(cx).errors(),
            _ => &[],
        }
    }

    /// Asks the chat that made the quilt to fix the errors its page met.
    pub(super) fn ask_fix(&mut self, cx: &mut Context<Self>) {
        let (Some(quilt), Some(n)) = (self.current(), self.shown) else {
            return;
        };
        let errors = self.page_errors(cx).join("\n");
        if errors.is_empty() {
            return;
        }
        let text = format!(
            "Version {n} of the quilt \"{}\" (id {}) shows these errors when it runs:\n\n\
             ```\n{errors}\n```\n\nPlease fix them.",
            quilt.title, quilt.key
        );
        cx.emit(QuiltsEvent::AskFix {
            chat_id: quilt.chat_id.clone(),
            text,
        });
    }

    /// Whether the next click on the preview picks an element.
    pub(super) fn picking(&self, cx: &App) -> bool {
        self.preview
            .as_ref()
            .is_some_and(|view| self.show == Show::Preview && view.read(cx).picking())
    }

    /// Starts or stops picking an element on the preview.
    pub(super) fn toggle_picking(&mut self, cx: &mut Context<Self>) {
        let on = !self.picking(cx);
        if let Some(view) = &self.preview {
            view.update(cx, |view, cx| view.set_picking(on, cx));
        }
    }

    /// Hands the element picked to the chat that made the quilt, to say what to change about it.
    fn picked(&mut self, picked: &ElementPicked, cx: &mut Context<Self>) {
        let (Some(quilt), Some(n)) = (self.current(), self.shown) else {
            return;
        };
        let text = format!(
            "In version {n} of the quilt \"{}\" (id {}), this element:\n\n\
             Selector: `{}`\n\n```html\n{}\n```\n\nComputed styles:\n\n```css\n{}\n```\n\n",
            quilt.title, quilt.key, picked.selector, picked.html, picked.styles
        );
        cx.emit(QuiltsEvent::Picked {
            chat_id: quilt.chat_id.clone(),
            text,
        });
    }

    /// Saves the version shown again as the newest.
    pub(super) fn restore(&mut self, cx: &mut Context<Self>) {
        let (Some(quilt), Some(n)) = (self.current(), self.shown) else {
            return;
        };
        let id = quilt.id.clone();
        let core = Arc::clone(&self.core);
        off_thread(
            cx,
            move || core.restore_quilt(&id, n),
            |this, restored, cx| {
                match restored {
                    // The core tells the window, which reads them again and follows the newest.
                    Ok(n) => this.shown = Some(n),
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            },
        );
    }

    /// Puts the version shown on the clipboard.
    pub(super) fn copy(&self, cx: &mut Context<Self>) {
        if let Some(version) = self.version() {
            cx.write_to_clipboard(ClipboardItem::new_string(version.content.clone()));
        }
    }

    /// Opens the version shown in the system's browser.
    pub(super) fn open_outside(&mut self, cx: &mut Context<Self>) {
        let (Some(quilt), Some(n)) = (self.current(), self.shown) else {
            return;
        };
        let id = quilt.id.clone();
        let core = Arc::clone(&self.core);
        off_thread(
            cx,
            move || core.quilt_page(&id, n),
            |this, page, cx| match page {
                Ok(url) => cx.open_url(&url),
                Err(e) => {
                    this.error = Some(e.to_string());
                    cx.notify();
                }
            },
        );
    }
}

impl EventEmitter<QuiltsEvent> for QuiltsPage {}

impl gpui::Focusable for QuiltsPage {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl std::fmt::Debug for QuiltsPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuiltsPage").finish_non_exhaustive()
    }
}
