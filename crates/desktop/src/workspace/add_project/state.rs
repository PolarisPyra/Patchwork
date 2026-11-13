//! The Add a project dialog: the folders it browses, on this machine or another, and adding the one picked.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use gpui::{Entity, EventEmitter, FocusHandle, KeyDownEvent, ScrollHandle, Window, prelude::*};
use patchwork_server::Server;
use patchwork_server::projects::Project;
use patchwork_server::system::browse::{self, DirEntry, DirListing, Machine};
use patchwork_server::system::remote;
use crate::ui::text_area::{Arrow, Changed, Submitted, TextArea};
use crate::ui::theme::theme;

/// The dialog closed: with the project it added, or with none.
pub struct Closed(pub Option<Project>);

#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate piece of view state"
)]
pub struct AddProject {
    pub(super) server: Arc<Server>,
    pub(super) machines: Option<Vec<Machine>>,
    pub(super) browsing: bool,
    /// The SSH host whose folders are shown, or none for this computer's.
    pub(super) host: Option<String>,
    pub(super) listing: Option<DirListing>,
    pub(super) field: Entity<TextArea>,
    /// The field's text before its last change: deleting the separator after the folder shown
    /// steps up to its parent.
    pub(super) last: String,
    pub(super) index: usize,
    pub(super) loading: bool,
    /// Counts the listings asked for, so one that returns after a later one was asked for is
    /// dropped.
    pub(super) browse_seq: u64,
    pub(super) adding: bool,
    pub(super) error: Option<String>,
    pub(super) focus: FocusHandle,
    /// The view changed, so the next frame moves the focus: to the field, or to the dialog.
    pub(super) refocus: bool,
    pub(super) list_scroll: ScrollHandle,
    /// The keyboard moved the highlight, so the list scrolls to keep it in view.
    pub(super) follow: bool,
}

impl EventEmitter<Closed> for AddProject {}

/// `path` ending in its separator, so typing after it filters the folders inside.
pub(super) fn with_sep(path: &str) -> String {
    let sep = if path.contains('\\') && !path.contains('/') {
        '\\'
    } else {
        '/'
    };
    if path.ends_with(sep) {
        path.to_string()
    } else {
        format!("{path}{sep}")
    }
}

impl AddProject {
    pub fn new(server: Arc<Server>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let t = *theme(cx);
        let field = cx.new(|cx| {
            let mut field = TextArea::new("", "Folder path", cx);
            field.single_line = true;
            field.text_color = t.text;
            field.placeholder_color = crate::ui::theme::input_placeholder();
            field.selection_color = t.accent.opacity(0.3);
            field
        });
        cx.subscribe(&field, |this, field, _: &Changed, cx| {
            let draft = field.read(cx).content.clone();
            let base = this.listing.as_ref().map(|l| with_sep(&l.path));
            let parent = this.listing.as_ref().and_then(|l| l.parent.clone());
            // Backspace on the folder's own path steps up to its parent.
            let stepped_up = base.is_some_and(|base| {
                this.last == base && draft.len() + 1 == base.len() && base.starts_with(&draft)
            });
            this.last = draft;
            this.index = 0;
            this.follow = true;
            match parent {
                Some(parent) if stepped_up => this.browse(Some(parent), cx),
                _ => cx.notify(),
            }
        })
        .detach();
        cx.subscribe(&field, |this, _, arrow: &Arrow, cx| {
            let count = this.visible(cx).len();
            let next = this.index.min(count.saturating_sub(1)) as isize + arrow.0;
            this.index = next.clamp(0, count.saturating_sub(1) as isize) as usize;
            this.follow = true;
            cx.notify();
        })
        .detach();
        cx.subscribe_in(&field, window, |this, field, _: &Submitted, window, cx| {
            if window.modifiers().control || window.modifiers().platform {
                this.add(cx);
                return;
            }
            let (query, picked) = {
                let visible = this.visible(cx);
                let active = this.index.min(visible.len().saturating_sub(1));
                (this.query(cx), visible.get(active).map(|e| e.path.clone()))
            };
            match (query, picked) {
                (Some(_), Some(path)) => this.browse(Some(path), cx),
                (None, _) => {
                    let draft = field.read(cx).content.clone();
                    this.browse(Some(draft), cx);
                }
                _ => {}
            }
        })
        .detach();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);

        // The machine picked in the toolbar opens at once, alongside the machine list; this
        // computer when none is picked.
        let host = server.store.settings().default_machine;
        let mut dialog = Self {
            server,
            machines: None,
            browsing: true,
            host,
            listing: None,
            field,
            last: String::new(),
            index: 0,
            loading: true,
            browse_seq: 0,
            adding: false,
            error: None,
            focus,
            refocus: true,
            list_scroll: ScrollHandle::new(),
            follow: false,
        };
        dialog.browse(None, cx);
        let task = cx.background_spawn(async move { browse::machines() });
        cx.spawn(async move |this, cx| {
            let machines = task.await;
            this.update(cx, |this, cx| {
                // A picked host the SSH config no longer lists gives way to this computer.
                let listed = this
                    .host
                    .as_ref()
                    .is_none_or(|h| machines.iter().any(|m| !m.local && m.name == *h));
                this.machines = Some(machines);
                if !listed {
                    this.open_machine(None, cx);
                }
                cx.notify();
            })
        })
        .detach();
        dialog
    }

    pub(super) fn browse(&mut self, path: Option<String>, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        self.browse_seq += 1;
        let seq = self.browse_seq;
        let host = self.host.clone();
        let task = cx.background_spawn(async move {
            match host {
                Some(host) => browse::browse_host(&host, path.as_deref()),
                None => browse::browse(path.as_deref()),
            }
        });
        cx.spawn(async move |this, cx| {
            let listing = task.await;
            this.update(cx, |this, cx| {
                if this.browse_seq != seq {
                    return;
                }
                match listing {
                    Ok(listing) => {
                        let draft = with_sep(&listing.path);
                        this.field.update(cx, |f, cx| f.set_text(&draft, cx));
                        this.last = draft;
                        this.listing = Some(listing);
                        this.index = 0;
                        this.follow = true;
                    }
                    Err(e) => this.error = Some(e.to_string()),
                }
                this.loading = false;
                cx.notify();
            })
        })
        .detach();
        cx.notify();
    }

    pub(super) fn add(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.listing.as_ref().map(|l| match &self.host {
            Some(host) => remote::root(host, &l.path),
            None => l.path.clone(),
        }) else {
            return;
        };
        if self.loading || self.adding {
            return;
        }
        self.adding = true;
        self.error = None;
        let server = Arc::clone(&self.server);
        let task = cx.background_spawn(async move { server.add_project(&path) });
        cx.spawn(async move |this, cx| {
            let added = task.await;
            this.update(cx, |this, cx| match added {
                Ok(project) => cx.emit(Closed(Some(project))),
                Err(e) => {
                    this.error = Some(e.to_string());
                    this.adding = false;
                    cx.notify();
                }
            })
        })
        .detach();
        cx.notify();
    }

    /// Text typed after the folder shown, with no separator in it, which filters its folders.
    pub(super) fn query(&self, cx: &gpui::App) -> Option<String> {
        let base = with_sep(&self.listing.as_ref()?.path);
        let typed = self.field.read(cx).content.strip_prefix(&base)?;
        (!typed.contains(['/', '\\'])).then(|| typed.to_lowercase())
    }

    pub(super) fn visible(&self, cx: &gpui::App) -> Vec<&DirEntry> {
        let query = self.query(cx);
        self.listing
            .iter()
            .flat_map(|l| &l.entries)
            .filter(|e| {
                let name = e.name.to_lowercase();
                // Hidden folders show only once the filter asks for them.
                if name.starts_with('.') && !query.as_ref().is_some_and(|q| q.starts_with('.')) {
                    return false;
                }
                query.as_ref().is_none_or(|q| name.contains(q.as_str()))
            })
            .collect()
    }

    pub(super) fn show_machines(&mut self, browsing: bool, cx: &mut Context<Self>) {
        self.browsing = browsing;
        self.refocus = true;
        cx.notify();
    }

    /// Shows the folders of `host`, or of this computer when it is none, from its home folder.
    pub(crate) fn open_machine(&mut self, host: Option<String>, cx: &mut Context<Self>) {
        if self.host != host {
            self.host = host;
            self.listing = None;
            self.last.clear();
            self.field.update(cx, |f, cx| f.set_text("", cx));
            self.browse(None, cx);
        }
        self.show_machines(true, cx);
    }

    pub(super) fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            cx.emit(Closed(None));
            cx.stop_propagation();
        }
    }
}
