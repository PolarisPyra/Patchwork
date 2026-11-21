//! The Artifacts page's state: every artifact as last read, the gallery's search, filter and
//! pictures, and the artifact opened, its versions, the version shown, and the page that draws
//! it.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{
    App, ClipboardItem, Entity, EventEmitter, FocusHandle, Pixels, Point, ScrollHandle,
    UniformListScrollHandle, prelude::*,
};
use patchwork_core::Core;
use patchwork_core::artifacts::{Artifact, ArtifactKind, ArtifactVersion, ChangeRow, artifact_changes};

use super::thumbs::Thumbs;
use crate::browser::{BrowserView, PageErrorsChanged};
use crate::ui::menu::Menu;
use crate::ui::off_thread;
use crate::ui::text_area::{Changed, TextArea};

/// What the page asks the window to do.
pub enum ArtifactsEvent {
    /// Start a new chat whose draft asks for an artifact of this kind.
    MakeNew(ArtifactKind),
    /// Open the chat an artifact came from.
    OpenChat(String),
    /// Send `text` to the chat `thread_id`, which made the artifact.
    AskFix { thread_id: String, text: String },
    /// Close the panel beside the chat.
    Closed,
    /// Show version `n` of the artifact `id` on the Artifacts page instead of beside the chat.
    OpenFull { id: String, n: u32 },
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

/// Where the artifacts the gallery shows were made.
#[derive(Clone, PartialEq, Eq)]
pub(super) enum Place {
    All,
    /// The chat open when the page opened.
    Chat,
    /// A project's chats, by the project's id.
    Project(String),
}

pub struct ArtifactsPage {
    pub(super) core: Arc<Core>,
    /// Every artifact, the most recently changed first.
    pub(super) list: Vec<Artifact>,
    /// Each chat's title, by its id, for the chat an artifact came from.
    pub(super) chats: HashMap<String, String>,
    /// Each chat's project, by the chat's id: the project's id and name.
    pub(super) projects: HashMap<String, (String, String)>,
    /// The chat open when the page opened, by its id.
    pub(super) came_from: Option<String>,
    pub(super) place: Place,
    pub(super) search: Entity<TextArea>,
    /// The kind the gallery shows, or every kind.
    pub(super) filter: Option<ArtifactKind>,
    /// Whether the gallery shows cards rather than rows.
    pub(super) grid: bool,
    /// The versions' pictures, which the chats' cards share.
    pub(super) thumbs: Entity<Thumbs>,
    /// The artifact opened in the viewer; the gallery shows while there is none.
    pub(super) selected: Option<String>,
    /// The picked artifact's versions, the oldest first.
    pub(super) versions: Vec<ArtifactVersion>,
    /// The number of the version shown.
    pub(super) shown: Option<u32>,
    pub(super) show: Show,
    /// What changed in the version shown since the one before it.
    pub(super) changes: Vec<ChangeRow>,
    pub(super) preview: Option<Entity<BrowserView>>,
    pub(super) error: Option<String>,
    /// An artifact's menu and where it shows.
    pub(super) menu: Option<(Entity<Menu>, Point<Pixels>)>,
    /// The field in place of the opened artifact's title while it is renamed.
    pub(super) renaming: Option<Entity<TextArea>>,
    /// Whether this is the panel beside a chat, which shows one artifact and no gallery.
    pub(super) docked: bool,
    /// The chat the window shows over the page, by its id.
    pub(super) chat_shown: Option<String>,
    pub(super) focus: FocusHandle,
    pub(super) gallery_scroll: ScrollHandle,
    pub(super) code_scroll: ScrollHandle,
    pub(super) changes_scroll: UniformListScrollHandle,
}

impl ArtifactsPage {
    /// The page, which knows the chat `came_from` that was open when it opened.
    pub fn new(
        core: Arc<Core>,
        thumbs: Entity<Thumbs>,
        came_from: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| {
            let mut field = TextArea::new("Search artifacts", "Search artifacts", cx);
            field.single_line = true;
            field.placeholder_color = Some(crate::ui::theme::input_placeholder());
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
        };
        page.reload(cx);
        page
    }

    /// The panel beside a chat, showing version `n` of the artifact `id`.
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

    /// Reads the artifacts again, and the opened one's versions, following its newest when that
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
                let threads = core.list_threads();
                let chats = threads
                    .iter()
                    .map(|t| (t.id.clone(), t.title.clone()))
                    .collect();
                let projects = threads
                    .iter()
                    .filter_map(|t| {
                        let id = t.project_id.as_ref()?;
                        Some((t.id.clone(), (id.clone(), named.get(id)?.clone())))
                    })
                    .collect();
                (core.artifacts(), chats, projects)
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
                this.thumbs.update(cx, |thumbs, cx| {
                    for a in this.list.iter().filter(|a| a.kind.drawn()) {
                        thumbs.want(&a.id, a.versions, cx);
                    }
                });
                cx.notify();
            },
        );
    }

    /// Shows only artifacts of `kind`, or all of them.
    pub(super) fn set_filter(&mut self, kind: Option<ArtifactKind>, cx: &mut Context<Self>) {
        self.filter = kind;
        cx.notify();
    }

    /// Shows only the artifacts made in `place`.
    pub(super) fn set_place(&mut self, place: Place, cx: &mut Context<Self>) {
        self.place = place;
        cx.notify();
    }

    /// Shows the gallery as cards or as rows.
    pub(super) fn set_grid(&mut self, grid: bool, cx: &mut Context<Self>) {
        self.grid = grid;
        cx.notify();
    }

    /// Asks the window for a new chat that makes an artifact of `kind`.
    pub(super) fn make_new(&mut self, kind: ArtifactKind, cx: &mut Context<Self>) {
        cx.emit(ArtifactsEvent::MakeNew(kind));
    }

    /// Asks the window to open the chat the opened artifact came from.
    pub(super) fn open_chat(&mut self, cx: &mut Context<Self>) {
        if let Some(artifact) = self.current() {
            cx.emit(ArtifactsEvent::OpenChat(artifact.thread_id.clone()));
        }
    }

    /// Notes the chat the window shows over the page, if any.
    pub fn set_chat_shown(&mut self, shown: Option<String>, cx: &mut Context<Self>) {
        self.chat_shown = shown;
        cx.notify();
    }

    /// Asks the window to show the opened artifact on the Artifacts page.
    pub(super) fn open_full(&mut self, cx: &mut Context<Self>) {
        if let (Some(id), Some(n)) = (self.selected.clone(), self.shown) {
            cx.emit(ArtifactsEvent::OpenFull { id, n });
        }
    }

    /// Goes back to the gallery, ending the opened artifact's browser; beside a chat, closes the
    /// panel.
    pub(super) fn close(&mut self, cx: &mut Context<Self>) {
        if self.docked {
            cx.emit(ArtifactsEvent::Closed);
        }
        self.selected = None;
        self.versions.clear();
        self.shown = None;
        self.preview = None;
        self.error = None;
        self.renaming = None;
        cx.notify();
    }

    /// The artifact picked.
    pub(super) fn current(&self) -> Option<&Artifact> {
        let id = self.selected.as_ref()?;
        self.list.iter().find(|a| &a.id == id)
    }

    /// The version shown.
    pub(super) fn version(&self) -> Option<&ArtifactVersion> {
        let n = self.shown?;
        self.versions.iter().find(|v| v.n == n)
    }

    /// The artifacts the filters and the search match, by title, type or chat.
    pub(super) fn matching(&self, cx: &App) -> Vec<&Artifact> {
        let typed = self.search.read(cx).content.trim().to_lowercase();
        self.list
            .iter()
            .filter(|a| self.filter.is_none_or(|k| a.kind == k))
            .filter(|a| match &self.place {
                Place::All => true,
                Place::Chat => self.came_from.as_ref() == Some(&a.thread_id),
                Place::Project(id) => self
                    .projects
                    .get(&a.thread_id)
                    .is_some_and(|(p, _)| p == id),
            })
            .filter(|a| {
                typed.is_empty()
                    || a.title.to_lowercase().contains(&typed)
                    || a.kind.label().to_lowercase().contains(&typed)
                    || self
                        .chats
                        .get(&a.thread_id)
                        .is_some_and(|c| c.to_lowercase().contains(&typed))
            })
            .collect()
    }

    /// Opens the artifact `id` and shows its newest version.
    pub(super) fn select(&mut self, id: String, cx: &mut Context<Self>) {
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

    /// Opens the artifact `id` on its version `n`, as a chat's card asks.
    pub fn open(&mut self, id: String, n: u32, cx: &mut Context<Self>) {
        self.select(id, cx);
        self.shown = Some(n);
    }

    /// Reads the picked artifact's versions. The newest is shown when it was before, or when the
    /// one shown is gone.
    fn load_versions(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        let core = Arc::clone(&self.core);
        let asked = id.clone();
        off_thread(
            cx,
            move || core.artifact_versions(&id),
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

    /// Shows version `n` of the picked artifact.
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
            move || artifact_changes(&old, &new),
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
        let (Some(artifact), Some(n)) = (self.current(), self.shown) else {
            return;
        };
        if self.show != Show::Preview || !artifact.kind.drawn() {
            return;
        }
        let (id, title) = (artifact.id.clone(), artifact.title.clone());
        let core = Arc::clone(&self.core);
        off_thread(
            cx,
            move || core.artifact_page(&id, n),
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
        let (Some(artifact), Some(version)) = (self.current(), self.version()) else {
            return;
        };
        let name = format!(
            "{}.{}",
            artifact.key,
            artifact.kind.extension(artifact.language.as_deref())
        );
        let text = version.content.clone();
        let home = patchwork_core::system::fs::home_dir().unwrap_or_default();
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

    /// Asks the chat that made the artifact to fix the errors its page met.
    pub(super) fn ask_fix(&mut self, cx: &mut Context<Self>) {
        let (Some(artifact), Some(n)) = (self.current(), self.shown) else {
            return;
        };
        let errors = self.page_errors(cx).join("\n");
        if errors.is_empty() {
            return;
        }
        let text = format!(
            "Version {n} of the artifact \"{}\" (id {}) shows these errors when it runs:\n\n\
             ```\n{errors}\n```\n\nPlease fix them.",
            artifact.title, artifact.key
        );
        cx.emit(ArtifactsEvent::AskFix {
            thread_id: artifact.thread_id.clone(),
            text,
        });
    }

    /// Saves the version shown again as the newest.
    pub(super) fn restore(&mut self, cx: &mut Context<Self>) {
        let (Some(artifact), Some(n)) = (self.current(), self.shown) else {
            return;
        };
        let id = artifact.id.clone();
        let core = Arc::clone(&self.core);
        off_thread(
            cx,
            move || core.restore_artifact(&id, n),
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
        let (Some(artifact), Some(n)) = (self.current(), self.shown) else {
            return;
        };
        let id = artifact.id.clone();
        let core = Arc::clone(&self.core);
        off_thread(
            cx,
            move || core.artifact_page(&id, n),
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

impl EventEmitter<ArtifactsEvent> for ArtifactsPage {}

impl gpui::Focusable for ArtifactsPage {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
