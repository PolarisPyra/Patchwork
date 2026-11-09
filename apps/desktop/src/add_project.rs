//! Add a project (`features/workspace/components/AddProjectDialog.tsx`): picks a project folder
//! inside the app, laid out like search. The field holds the folder being shown: type after it to
//! filter its folders, or type or paste another path and press Enter to go there. When
//! `~/.ssh/config` names hosts, a list of machines comes first; only this computer can be browsed
//! today, so the hosts are listed but disabled.

use std::sync::Arc;

use gpui::{
    Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent, MouseButton,
    Role, ScrollHandle, SharedString, Window, div, point, prelude::*, px,
};
use patchwork_server::Server;
use patchwork_server::projects::Project;
use patchwork_server::system::browse::{self, DirEntry, DirListing, Machine};

use crate::chat::text_area::{Arrow, Changed, Submitted, TextArea};
use crate::theme::{FONT_MONO, FS_MD, FS_SM, FS_XS, RADIUS, RADIUS_LG, theme};
use crate::ui::{BtnKind, btn, icon, icon_button, popup_shadow, spinner, tooltip};

/// The dialog closed: with the project it added, or with none.
pub struct Closed(pub Option<Project>);

pub struct AddProject {
    server: Arc<Server>,
    machines: Option<Vec<Machine>>,
    browsing: bool,
    listing: Option<DirListing>,
    field: Entity<TextArea>,
    /// The field's text before its last change: deleting the separator after the folder shown
    /// steps up to its parent.
    last: String,
    index: usize,
    loading: bool,
    adding: bool,
    error: Option<String>,
    focus: FocusHandle,
    /// The view changed, so the next frame moves the focus: to the field, or to the dialog.
    refocus: bool,
    list_scroll: ScrollHandle,
    /// The keyboard moved the highlight, so the list scrolls to keep it in view.
    follow: bool,
}

impl EventEmitter<Closed> for AddProject {}

/// `path` ending in its separator, so typing after it filters the folders inside.
fn with_sep(path: &str) -> String {
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
            field.placeholder_color = crate::theme::input_placeholder();
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

        // The home folder loads alongside the machine list, so this computer opens at once either
        // way.
        let mut dialog = Self {
            server,
            machines: None,
            browsing: false,
            listing: None,
            field,
            last: String::new(),
            index: 0,
            loading: true,
            adding: false,
            error: None,
            focus,
            refocus: false,
            list_scroll: ScrollHandle::new(),
            follow: false,
        };
        dialog.browse(None, cx);
        let task = cx.background_spawn(async move { browse::machines() });
        cx.spawn(async move |this, cx| {
            let machines = task.await;
            this.update(cx, |this, cx| {
                if !machines.iter().any(|m| !m.local) {
                    this.browsing = true;
                    this.refocus = true;
                }
                this.machines = Some(machines);
                cx.notify();
            })
        })
        .detach();
        dialog
    }

    fn browse(&mut self, path: Option<String>, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        let task = cx.background_spawn(async move { browse::browse(path) });
        cx.spawn(async move |this, cx| {
            let listing = task.await;
            this.update(cx, |this, cx| {
                match listing {
                    Ok(listing) => {
                        let draft = with_sep(&listing.path);
                        this.field.update(cx, |f, cx| f.set_text(&draft, cx));
                        this.last = draft;
                        this.listing = Some(listing);
                        this.index = 0;
                        this.follow = true;
                    }
                    Err(e) => this.error = Some(e),
                }
                this.loading = false;
                cx.notify();
            })
        })
        .detach();
        cx.notify();
    }

    fn add(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.listing.as_ref().map(|l| l.path.clone()) else {
            return;
        };
        if self.loading || self.adding {
            return;
        }
        self.adding = true;
        self.error = None;
        let server = self.server.clone();
        let task = cx.background_spawn(async move { server.add_project(&path) });
        cx.spawn(async move |this, cx| {
            let added = task.await;
            this.update(cx, |this, cx| match added {
                Ok(project) => cx.emit(Closed(Some(project))),
                Err(e) => {
                    this.error = Some(e);
                    this.adding = false;
                    cx.notify();
                }
            })
        })
        .detach();
        cx.notify();
    }

    /// Text typed after the folder shown, with no separator in it, which filters its folders.
    fn query(&self, cx: &gpui::App) -> Option<String> {
        let base = with_sep(&self.listing.as_ref()?.path);
        let typed = self.field.read(cx).content.strip_prefix(&base)?;
        (!typed.contains(['/', '\\'])).then(|| typed.to_lowercase())
    }

    fn visible(&self, cx: &gpui::App) -> Vec<&DirEntry> {
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

    fn show_machines(&mut self, browsing: bool, cx: &mut Context<Self>) {
        self.browsing = browsing;
        self.refocus = true;
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            cx.emit(Closed(None));
            cx.stop_propagation();
        }
    }
}

impl Focusable for AddProject {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for AddProject {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        if std::mem::take(&mut self.refocus) {
            if self.browsing {
                window.focus(&self.field.read(cx).focus_handle(cx), cx);
            } else {
                window.focus(&self.focus, cx);
            }
        }
        let hosts: Vec<(String, Option<String>)> = self
            .machines
            .iter()
            .flatten()
            .filter(|m| !m.local)
            .map(|m| (m.name.clone(), m.hostname.clone()))
            .collect();
        let local_name = self
            .machines
            .iter()
            .flatten()
            .find(|m| m.local)
            .map_or_else(|| "This computer".to_string(), |m| m.name.clone());
        let row = || {
            div()
                .flex()
                .flex_none()
                .h(px(36.))
                .w_full()
                .items_center()
                .gap(px(12.))
                .px(px(12.))
                .rounded(px(RADIUS))
                .text_size(px(FS_MD))
                .text_color(t.text)
        };
        let hover = t.bg_hover;
        let viewport = window.viewport_size();
        let error = self.error.clone().map(|e| {
            div()
                .id("add-project-error")
                .role(Role::Alert)
                .border_t_1()
                .border_color(t.border)
                .px(px(20.))
                .py(px(8.))
                .text_size(px(FS_SM))
                .text_color(t.danger)
                .child(e)
        });

        let body = if !self.browsing {
            div()
                .flex()
                .flex_col()
                .min_h_0()
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .h(px(56.))
                        .items_center()
                        .px(px(20.))
                        .border_b_1()
                        .border_color(t.border)
                        .text_size(px(crate::theme::FS_LG))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Add a project"),
                )
                .child(
                    div()
                        .id("machines")
                        .role(Role::Group)
                        .aria_label("Machines")
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .p(px(8.))
                        .child(
                            row()
                                .id("machine-local")
                                .role(Role::Button)
                                .aria_label(SharedString::from(local_name.clone()))
                                .tab_index(0)
                                .cursor_pointer()
                                .hover(move |s| s.bg(hover))
                                .child(
                                    div()
                                        .flex_none()
                                        .text_color(t.text_muted)
                                        .child(icon("desktop", 16.)),
                                )
                                .child(div().min_w_0().flex_1().truncate().child(local_name))
                                .child(
                                    div()
                                        .flex_none()
                                        .text_size(px(FS_XS))
                                        .text_color(t.text_subtle)
                                        .child("This computer"),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_color(t.text_subtle)
                                        .child(icon("caret-right", 12.)),
                                )
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.show_machines(true, cx)),
                                ),
                        )
                        .child(
                            div()
                                .px(px(12.))
                                .pt(px(12.))
                                .pb(px(6.))
                                .text_size(px(FS_XS))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(t.text_subtle)
                                .child("SSH HOSTS"),
                        )
                        .children(hosts.iter().map(|(name, hostname)| {
                            row()
                                .id(SharedString::from(format!("machine-{}", name)))
                                .role(Role::Button)
                                .aria_label(SharedString::from(name.clone()))
                                .aria_description("Connecting to SSH hosts is not supported yet")
                                .tab_index(0)
                                .opacity(0.5)
                                .tooltip(tooltip("Connecting to SSH hosts is not supported yet"))
                                .child(
                                    div()
                                        .flex_none()
                                        .text_color(t.text_muted)
                                        .child(icon("hard-drives", 16.)),
                                )
                                .child(div().min_w_0().flex_1().truncate().child(name.clone()))
                                .children(hostname.clone().map(|h| {
                                    div()
                                        .flex_none()
                                        .max_w(px(220.))
                                        .truncate()
                                        .font_family(FONT_MONO)
                                        .text_size(px(FS_XS))
                                        .text_color(t.text_subtle)
                                        .child(h)
                                }))
                        })),
                )
                .children(error)
                .into_any_element()
        } else {
            let follow = std::mem::take(&mut self.follow);
            let query = self.query(cx);
            let visible = self.visible(cx);
            let active = self.index.min(visible.len().saturating_sub(1));
            if follow {
                if active == 0 {
                    self.list_scroll.set_offset(point(px(0.), px(0.)));
                } else {
                    self.list_scroll.scroll_to_item(active);
                }
            }
            let empty = (!self.loading && visible.is_empty()).then(|| {
                div()
                    .px(px(12.))
                    .py(px(24.))
                    .text_center()
                    .text_size(px(FS_MD))
                    .text_color(t.text_subtle)
                    .child(if query.as_deref().is_some_and(|q| !q.is_empty()) {
                        "No matches"
                    } else {
                        "No folders"
                    })
            });
            let rows: Vec<_> = visible
                .iter()
                .enumerate()
                .map(|(i, entry)| {
                    let path = entry.path.clone();
                    let is_active = i == active;
                    row()
                        .id(SharedString::from(format!("folder-{i}")))
                        .role(Role::ListBoxOption)
                        .aria_label(SharedString::from(entry.name.clone()))
                        .aria_selected(is_active)
                        .cursor_pointer()
                        .map(|r| {
                            if is_active {
                                r.bg(t.bg_selected)
                            } else {
                                r.hover(move |s| s.bg(hover))
                            }
                        })
                        .child(
                            div()
                                .flex_none()
                                .text_color(t.text_muted)
                                .child(icon("folder", 16.)),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .child(entry.name.clone()),
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.browse(Some(path.clone()), cx)),
                        )
                })
                .collect();
            let parent = self.listing.as_ref().and_then(|l| l.parent.clone());
            let disabled = self.listing.is_none() || self.loading || self.adding;
            div()
                .flex()
                .flex_col()
                .min_h_0()
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .h(px(56.))
                        .items_center()
                        .gap(px(4.))
                        .pl(px(12.))
                        .pr(px(20.))
                        .border_b_1()
                        .border_color(t.border)
                        .when(!hosts.is_empty(), |bar| {
                            bar.child(
                                icon_button(
                                    "add-project-back",
                                    "Back to machines",
                                    "caret-left",
                                    16.,
                                    cx,
                                )
                                .tooltip(tooltip("Machines"))
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.show_machines(false, cx)),
                                ),
                            )
                        })
                        .child(
                            icon_button("add-project-up", "Parent folder", "arrow-up", 16., cx)
                                .tooltip(tooltip("Parent folder"))
                                .map(|b| match parent {
                                    Some(parent) => {
                                        b.on_click(cx.listener(move |this, _, _, cx| {
                                            this.browse(Some(parent.clone()), cx)
                                        }))
                                    }
                                    None => b.opacity(0.4).cursor_default(),
                                }),
                        )
                        .child(
                            div()
                                .ml(px(4.))
                                .flex_1()
                                .min_w_0()
                                .font_family(FONT_MONO)
                                .text_size(px(FS_MD))
                                .child(self.field.clone()),
                        )
                        .when(self.loading, |bar| bar.child(spinner(14., t.text_subtle))),
                )
                .child(
                    div()
                        .id("add-project-folders")
                        .role(Role::ListBox)
                        .aria_label("Folders")
                        .flex()
                        .flex_col()
                        .h(px(320.))
                        .min_h_0()
                        .p(px(8.))
                        .overflow_y_scroll()
                        .track_scroll(&self.list_scroll)
                        .children(empty)
                        .children(rows),
                )
                .children(error)
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .h(px(48.))
                        .items_center()
                        .gap(px(16.))
                        .pl(px(20.))
                        .pr(px(10.))
                        .border_t_1()
                        .border_color(t.border)
                        .text_size(px(FS_XS))
                        .text_color(t.text_subtle)
                        .child("↑↓ navigate")
                        .child("↵ open")
                        .child("⌫ up")
                        .child(
                            btn("add-project", "Add project", BtnKind::Primary, disabled, &t)
                                .ml_auto()
                                .when(self.adding, |b| b.child(spinner(13., t.text_on_accent)))
                                .child("Add project")
                                .when(!disabled, |b| {
                                    b.on_click(cx.listener(|this, _, _, cx| this.add(cx)))
                                }),
                        ),
                )
                .into_any_element()
        };

        // The machine list waits for the hosts, so it never flashes before switching to the
        // folders.
        let shown = self.machines.is_some() || self.browsing;
        div()
            .id("add-project-backdrop")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_start()
            .justify_center()
            .pt(viewport.height * 0.15)
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(Closed(None))),
            )
            .on_key_down(cx.listener(Self::on_key))
            .when(shown, |backdrop| {
                backdrop.child(
                    div()
                        .id("add-project")
                        .role(Role::Dialog)
                        .aria_label("Add a project")
                        .track_focus(&self.focus)
                        .flex()
                        .flex_col()
                        .w(px(560.))
                        .max_w(viewport.width - px(32.))
                        .max_h(viewport.height * 0.6)
                        .overflow_hidden()
                        .rounded(px(RADIUS_LG))
                        .border_1()
                        .border_color(t.border_strong)
                        .bg(t.bg_elevated)
                        .shadow(popup_shadow(&t))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(body),
                )
            })
    }
}
