//! The open project in the sidebar: its git state, its folder, which view shows, and the machines
//! projects can live on.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{Entity, Window, prelude::*, EventEmitter, FontWeight, Role, SharedString, div, px};
use patchwork_backend::projects::{Project, ProjectChanged};
use patchwork_desktop_dialogs::add_project::{AddProject, Closed as AddProjectClosed};
use patchwork_platform::files::browse::{self, Machine};
use patchwork_platform::machine::device::{self, Kind};
use patchwork_ui_theme::{IconName, fs_md, fs_sm, radius_sm, theme};
use patchwork_ui_components::{button, icon_button, spinner};
use patchwork_ui_theme::icon;

use crate::workspace::{View, Workspace};

impl Workspace {
    /// Reads the project's branch and changed files off the main thread, and watches the project
    /// so they stay current whoever changes its files: an agent, an editor, or git in a terminal.
    /// Source Control shows the open task's worktree when it works in one.
    pub fn load_git(&mut self, cx: &mut Context<Self>) {
        self.read_git(true, cx);
    }

    /// Like [`Self::load_git`], but asks where the branch stands (over the network) only when
    /// `sync` is set or Source Control's folder changed, as it does on moving to another task.
    pub(crate) fn read_git(&mut self, sync: bool, cx: &mut Context<Self>) {
        self.git_seq += 1;
        let seq = self.git_seq;
        let Some(project) = self.project() else {
            self.branch = None;
            self.changes.clear();
            self.git_root = None;
            self.changes_generation += 1;
            self.changes_view
                .update(cx, |v, cx| v.refresh(None, None, true, cx));
            return;
        };
        let id = project.id.clone();
        let chat = self.chat.as_ref().map(|t| t.id.clone());
        let (core, events) = (Arc::clone(&self.core), self.project_events.clone());
        let read = id.clone();
        patchwork_ui_theme::off_thread(
            cx,
            move || {
                let root = core.source_root(&read, chat.as_deref())?;
                let watched = core.watch_project(&read, chat.as_deref(), seq, move |changed| {
                    let _ = events.unbounded_send(changed);
                });
                let branch = patchwork_backend::source_control::git_branch(&root);
                let changes = patchwork_backend::source_control::git_changes(&root)
                    .inspect_err(|e| {
                        log::warn!(error:% = e; "could not list the changed files: {{error}}");
                    })
                    .ok();
                Ok::<_, patchwork_backend::Error>((root, watched, branch, changes))
            },
            move |this, read, cx| {
                // A newer read, for another task or project, has started since.
                if this.git_seq != seq {
                    return;
                }
                let (root, watched, branch, changes) = match read {
                    Ok(read) => read,
                    Err(e) => {
                        this.error = Some(e.to_string());
                        return cx.notify();
                    }
                };
                if let Err(e) = watched {
                    this.error = Some(e.to_string());
                }
                let moved = this.git_root.as_ref() != Some(&root);
                match changes {
                    Some(changes) => this.changes = changes,
                    // A failed read keeps the last good list of the same folder, and shows none
                    // of another's.
                    None if moved => this.changes.clear(),
                    None => {}
                }
                this.git_root = Some(root.clone());
                this.branch = branch;
                this.changes_generation += 1;
                this.changes_view.update(cx, |v, cx| {
                    v.refresh(Some(id), Some(root), sync || moved, cx);
                });
                cx.notify();
            },
        );
    }

    /// Git's state after a settled burst of changes in the watched project.
    pub(crate) fn on_project_changed(&mut self, changed: ProjectChanged, cx: &mut Context<Self>) {
        // A burst from a folder Source Control no longer shows, sent before its watch ended.
        if self.project().map(|p| &p.id) != Some(&changed.project_id)
            || self.git_root.as_ref() != Some(&changed.root)
        {
            return;
        }
        if let Some(canvas) = &self.canvas {
            canvas.update(cx, |c, cx| c.on_paths(&changed.paths, cx));
        }
        if changed
            .paths
            .iter()
            .any(|p| patchwork_backend::android::is_marker(p))
        {
            self.detect_mobile(cx);
        }
        // A run that stays attached takes saved code without a rebuild.
        let reload = self
            .android
            .as_ref()
            .and_then(|android| android.read(cx).reload_on_save(&changed.paths));
        if let Some(keys) = reload {
            self.type_in_run(keys, cx);
        }
        // A Kotlin or Java app has no tool that stays, so a save builds and installs it again.
        if let Some(android) = self.android.clone() {
            android.update(cx, |android, cx| {
                android.rebuild_on_save(&changed.paths, cx);
            });
        }
        let Some(files) = changed.changes else {
            return;
        };
        let had_changes = !self.changes.is_empty();
        let git = changed.branch != self.branch
            || had_changes == files.is_empty()
            || changed.paths.iter().any(|p| p.starts_with(".git/"));
        let counted = files.len() != self.changes.len();
        self.branch = changed.branch;
        self.changes = files;
        self.changes_generation += 1;
        // The composer's worktree status counts the same commits and files, and otherwise reads
        // them only when a message arrives.
        if git || counted {
            self.reload_worktree(cx);
        }
        let (id, root) = (changed.project_id, changed.root);
        self.changes_view
            .update(cx, |v, cx| v.refresh(Some(id), Some(root), git, cx));
        cx.notify();
    }

    pub fn toggle_collapsed(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(id) {
            self.collapsed.insert(id.to_string());
        }
        self.save_collapsed(cx);
        cx.notify();
    }

    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        for project in &self.settings.projects {
            self.collapsed.insert(project.id.clone());
        }
        self.save_collapsed(cx);
        cx.notify();
    }

    /// Saves which projects and folders are folded away, off the UI thread.
    pub fn save_collapsed(&self, cx: &gpui::App) {
        let ids: Vec<String> = self.collapsed.iter().cloned().collect();
        let core = Arc::clone(&self.core);
        cx.background_spawn(async move {
            if let Err(e) = core.set_collapsed_sections(&ids) {
                log::warn!(error:% = e; "could not save the open sections: {{error}}");
            }
        })
        .detach();
    }
}

impl Workspace {
    /// Makes `id` the project new tasks go in.
    pub(crate) fn remember_project(&mut self, id: Option<String>) {
        if self.settings.window.active_project_id == id {
            return;
        }
        self.settings.window.active_project_id.clone_from(&id);
        if let Err(e) = self
            .core
            .update_settings(|s| s.window.active_project_id = id)
        {
            log::warn!(error:% = e; "could not record the active project: {{error}}");
        }
    }

    /// Opens the project's newest chat, or an empty one in it when it has none.
    pub fn select_project(&mut self, id: &str, cx: &mut Context<Self>) {
        self.remember_project(Some(id.to_string()));
        let first = self
            .chats
            .iter()
            .find(|t| {
                t.project_id.as_deref() == Some(id) && t.archived_at.is_none() && t.parent.is_none()
            })
            .map(|t| t.id.clone());
        if let Some(chat) = first {
            self.open_chat(&chat, cx);
        } else {
            // No chat opens to read the project's branch and changes, so they are read here.
            self.new_chat(cx);
            self.load_git(cx);
        }
    }

    /// Shows the chat in the task list, opening its project and folder, and opens it.
    pub(crate) fn reveal_chat(&mut self, id: &str, cx: &mut Context<Self>) {
        self.view = Some(View::Tasks);
        if let Some(chat) = self.chats.iter().find(|t| t.id == id) {
            let mut opened = false;
            if let Some(project) = &chat.project_id {
                opened |= self.collapsed.remove(project);
            }
            if let Some(folder) = &chat.folder_id {
                opened |= self.collapsed.remove(&format!("folder:{folder}"));
            }
            if opened {
                self.save_collapsed(cx);
            }
        }
        self.open_chat(id, cx);
    }

    /// Opens the dialog that picks a folder to add as a project, on the machine picked in the
    /// toolbar.
    pub fn open_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialogs.read(cx).add_project.is_none() {
            let host = self.settings.window.default_machine.clone();
            self.open_folder_on(host, window, cx);
        }
    }

    /// Opens that dialog on `host`, or on this computer when it is none.
    fn open_folder_on(
        &mut self,
        host: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(dialog) = self.dialogs.read(cx).add_project.clone() {
            dialog.update(cx, |d, cx| d.open_machine(host, cx));
            return;
        }
        let return_focus = window.focused(cx);
        let core = Arc::clone(&self.core);
        let dialog = cx.new(|cx| AddProject::new(core, host, window, cx));
        cx.subscribe_in(
            &dialog,
            window,
            move |this, _, closed: &AddProjectClosed, window, cx| {
                this.dialogs_mut(cx, |d| d.add_project = None);
                match &return_focus {
                    Some(focus) => window.focus(focus, cx),
                    None => window.focus(&this.focus, cx),
                }
                if let Some(project) = &closed.0 {
                    this.project_added(project, cx);
                }
                cx.notify();
            },
        )
        .detach();
        self.dialogs_mut(cx, |d| d.add_project = Some(dialog));
        cx.notify();
    }

    /// The Devices view, made and hooked up the first time it is shown.
    pub fn devices_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<DevicesView> {
        if let Some(view) = &self.devices_view {
            return view.clone();
        }
        let view = cx.new(DevicesView::new);
        // The toolbar's machine button shows what the view learns about each machine.
        cx.observe(&view, |_, _, cx| cx.notify()).detach();
        cx.subscribe_in(&view, window, |this, _, open: &OpenDevice, window, cx| {
            this.open_folder_on(open.0.clone(), window, cx);
        })
        .detach();
        self.devices_view = Some(view.clone());
        view
    }

    /// Adds the folders dropped on the sidebar as projects, off the main thread, and opens the
    /// last one added.
    pub fn add_dropped_folders(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
        let paths = paths.to_vec();
        patchwork_ui_theme::off_thread(
            cx,
            move || {
                paths
                    .iter()
                    .map(|path| core.add_project(&path.to_string_lossy()))
                    .collect::<Vec<_>>()
            },
            move |this, added, cx| {
                for result in added {
                    match result {
                        Ok(project) => this.project_added(&project, cx),
                        Err(e) => this.error = Some(e.to_string()),
                    }
                }
                cx.notify();
            },
        );
    }

    fn project_added(&mut self, project: &Project, cx: &mut Context<Self>) {
        // A new project starts closed like the rest of the list; one already there keeps its
        // state.
        if !self.settings.projects.iter().any(|p| p.id == project.id) {
            self.collapsed.insert(project.id.clone());
            self.save_collapsed(cx);
        }
        self.settings = self.core.settings();
        self.select_project(&project.id, cx);
    }
}

/// The icon for a kind of machine: a desktop, a laptop, or a drive for anything else.
#[must_use]
pub(crate) fn kind_icon(kind: Kind) -> IconName {
    match kind {
        Kind::Desktop => IconName::Desktop,
        Kind::Laptop => IconName::Laptop,
        Kind::Other => IconName::HardDrives,
    }
}

/// A device was picked: the SSH host's alias, or none for this computer.
pub struct OpenDevice(pub Option<String>);

pub struct DevicesView {
    /// The machines, once read, each with its kind once it answered.
    devices: Option<Vec<(Machine, Option<Kind>)>>,
    /// Counts the loads started, so an older load's answers are dropped.
    run: usize,
}

impl EventEmitter<OpenDevice> for DevicesView {}

impl DevicesView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            devices: None,
            run: 0,
        };
        this.load(false, cx);
        this
    }

    /// Reads the machines, then asks each what it is, off the UI thread. A host that is slow to
    /// answer holds up only its own icon. `fresh` asks each host again, where it would otherwise
    /// say what the host answered before.
    fn load(&mut self, fresh: bool, cx: &mut Context<Self>) {
        self.run += 1;
        let run = self.run;
        self.devices = None;
        patchwork_ui_theme::off_thread(cx, browse::machines, move |this, machines, cx| {
            if this.run != run {
                return;
            }
            for (i, machine) in machines.iter().enumerate() {
                let machine = machine.clone();
                let task = cx.background_spawn(async move {
                    if fresh {
                        device::forget(&machine);
                    }
                    device::kind(&machine)
                });
                cx.spawn(async move |this, cx| {
                    let kind = task.await;
                    this.update(cx, |this, cx| {
                        if this.run != run {
                            return;
                        }
                        if let Some((_, slot)) = this.devices.as_mut().and_then(|d| d.get_mut(i)) {
                            *slot = Some(kind);
                            cx.notify();
                        }
                    })
                })
                .detach();
            }
            this.devices = Some(machines.into_iter().map(|m| (m, None)).collect());
            cx.notify();
        });
        cx.notify();
    }

    /// The machines, once read, each with its kind once it answered.
    pub(crate) fn machines(&self) -> Option<&[(Machine, Option<Kind>)]> {
        self.devices.as_deref()
    }
}

impl Render for DevicesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let (fg, hover) = (t.text, t.tree_hover);
        let heading = div()
            .h(px(32.))
            .mt(px(6.))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .pl(px(12.))
            .pr(px(12.))
            .text_size(px(fs_md()))
            .font_weight(FontWeight::SEMIBOLD)
            .child("Devices")
            .child(
                icon_button(
                    "refresh-devices",
                    "Refresh",
                    IconName::ArrowsClockwise,
                    18.,
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| this.load(true, cx))),
            );
        let rows = self
            .devices
            .iter()
            .flatten()
            .enumerate()
            .map(|(i, (m, kind))| {
                let name = m.name.clone();
                let host = (!m.local).then(|| m.name.clone());
                let mark = match kind {
                    Some(kind) => icon(kind_icon(*kind), 16.).into_any_element(),
                    None => spinner(14., t.text_muted).into_any_element(),
                };
                button(SharedString::from(format!("device-{i}")), name.clone())
                    .role(Role::ListItem)
                    .h(px(patchwork_ui_theme::roomy(28.)))
                    .gap(px(6.))
                    .mx(px(4.))
                    .px(px(8.))
                    .rounded(px(radius_sm()))
                    .text_size(px(fs_md()))
                    .text_color(fg)
                    .hover(move |s| s.bg(hover))
                    .child(
                        div()
                            .w(px(16.))
                            .flex_none()
                            .flex()
                            .justify_center()
                            .text_color(t.text_muted)
                            .child(mark),
                    )
                    .child(div().min_w_0().flex_1().truncate().child(name))
                    .when(m.local, |row| {
                        row.child(
                            div()
                                .flex_none()
                                .text_size(px(fs_sm()))
                                .text_color(t.text_subtle)
                                .child("This computer"),
                        )
                    })
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(OpenDevice(host.clone()))))
            });
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(heading)
            .child(
                div()
                    .id("devices")
                    .role(Role::List)
                    .aria_label("Devices")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .when(self.devices.is_none(), |d| {
                        d.child(
                            div()
                                .id("devices-loading")
                                .px(px(12.))
                                .role(Role::Status)
                                .aria_label("Loading devices")
                                .child(spinner(14., t.text_muted)),
                        )
                    })
                    .children(rows)
                    // With no SSH hosts, a greyed row in the hosts' place says so, laid out like
                    // the rows above it.
                    .when(self.devices.as_ref().is_some_and(|d| d.len() == 1), |d| {
                        d.child(
                            div()
                                .id("device-none")
                                .role(Role::ListItem)
                                .aria_label("No remote devices available")
                                .h(px(patchwork_ui_theme::roomy(28.)))
                                .mx(px(4.))
                                .px(px(8.))
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .text_size(px(fs_md()))
                                .text_color(t.text_muted)
                                .opacity(0.6)
                                .child(
                                    div()
                                        .w(px(16.))
                                        .flex_none()
                                        .flex()
                                        .justify_center()
                                        .child(icon(IconName::HardDrives, 16.)),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .truncate()
                                        .child("No remote devices available"),
                                ),
                        )
                    }),
            )
    }
}
