//! The canvas beside the chat: what opens in it, and how wide it is. An artifact opened from the
//! chat shows there too.

// Rust guideline compliant 2026-10-09

use gpui::{MouseButton, Role, Window, div, prelude::*, px};
use patchwork_core::threads::Message;
use std::sync::Arc;

use crate::artifacts::ArtifactsPage;
use crate::canvas::{CANVAS_MAX_FRACTION, CANVAS_MIN, Canvas, CanvasTab};
use crate::ui::theme::{ACTIVITY_BAR_W, theme};
use super::state::Workspace;

impl Workspace {
    /// Shows version `n` of the artifact `id` beside the chat, or on the Artifacts page when that
    /// shows.
    pub(crate) fn open_artifact_beside(
        &mut self,
        id: String,
        n: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(thread) = self.thread.as_ref().map(|t| t.id.clone()) else {
            return;
        };
        if self.page == super::Page::Artifacts {
            self.open_artifact(id, n, window, cx);
            return;
        }
        match &self.artifact_beside {
            Some((of, page)) if *of == thread => page.update(cx, |page, cx| page.open(id, n, cx)),
            _ => {
                cx.background_spawn(async { patchwork_core::browser::warm_browser() })
                    .detach();
                let (core, thumbs) = (Arc::clone(&self.core), self.thumbs.clone());
                let page = cx.new(|cx| ArtifactsPage::beside_chat(core, thumbs, id, n, cx));
                cx.subscribe_in(&page, window, Self::on_artifacts).detach();
                self.artifact_beside = Some((thread, page));
            }
        }
        cx.notify();
    }

    /// Opens a changed file's diff, or the file itself when it has no changes.
    pub fn open_change(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        // A file in conflict opens on its code, where its conflicts are settled.
        let change = self.changes.iter().find(|c| c.path == path);
        let tab = if change.is_some_and(|c| c.status != "conflicted") {
            CanvasTab::Diff
        } else {
            CanvasTab::Code
        };
        let Some(project) = self.project() else {
            return;
        };
        // The file is in the folder Source Control shows, which may be the task's worktree.
        let root = self
            .git_root
            .clone()
            .unwrap_or_else(|| project.path.clone());
        self.open_canvas_at(path, root, None, tab, window, cx);
    }

    /// Opens a changed file's diff showing one part of it: what is staged, or what is not.
    pub fn open_change_side(
        &mut self,
        path: &str,
        side: patchwork_core::workspace::DiffSide,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_change(path, window, cx);
        // The canvas reads its diff once this update ends.
        if let Some(canvas) = &self.canvas {
            canvas.update(cx, |canvas, _| canvas.side = side);
        }
    }

    /// Opens a file's diff from commit `since`, where a task's branch left the branch it goes
    /// back into, to the file as it is now.
    pub fn open_change_since(
        &mut self,
        path: &str,
        since: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.git_root.clone() else {
            return;
        };
        self.open_canvas_at(path, root, None, CanvasTab::Diff, window, cx);
        // The canvas reads its diff once this update ends.
        if let Some(canvas) = &self.canvas {
            canvas.update(cx, |canvas, _| canvas.since = Some(since));
        }
    }

    /// Opens `path` in the file panel, on `tab`, with the reply it came from.
    pub fn open_canvas(
        &mut self,
        path: &str,
        message: Option<Message>,
        tab: CanvasTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.project() else {
            return;
        };
        // A reply's files are in the folder its task worked in, which may be its worktree.
        let root = message
            .as_ref()
            .and_then(|_| self.thread_folder())
            .unwrap_or_else(|| project.path.clone());
        self.open_canvas_at(path, root, message, tab, window, cx);
    }

    /// Opens `path`, in the folder `root`, in the file panel on `tab`, with the reply it came
    /// from.
    pub fn open_canvas_at(
        &mut self,
        path: &str,
        root: String,
        message: Option<Message>,
        tab: CanvasTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.project() else {
            return;
        };
        let id = project.id.clone();
        let workspace = cx.entity().downgrade();
        let path = Some(path.to_string());
        if self.android_open(cx) {
            self.android = None;
        }
        self.canvas =
            Some(cx.new(|cx| Canvas::new(workspace, id, root, path, message, tab, window, cx)));
        cx.notify();
    }

    /// Looks for a mobile app when the open project is another than the one last looked in. The
    /// Android panel belongs to the project it opened for: it hides while another is open and
    /// shows again on the way back.
    pub(super) fn sync_mobile(&mut self, cx: &mut Context<Self>) {
        let id = self.project().map(|p| p.id.clone());
        if id != self.mobile_for {
            self.mobile_for = id;
            self.mobile = None;
            self.detect_mobile(cx);
        }
    }

    /// Reads the open project's manifests and build files, off the main thread, for whether it
    /// is a mobile app; the toolbar shows the phone only for one.
    pub(super) fn detect_mobile(&mut self, cx: &mut Context<Self>) {
        let Some(project) = self.project() else {
            return;
        };
        let (id, root) = (project.id.clone(), std::path::PathBuf::from(&project.path));
        crate::ui::off_thread(
            cx,
            move || patchwork_core::android::detect(&root),
            move |this, found, cx| {
                if this.mobile_for.as_ref() != Some(&id) {
                    return;
                }
                let ours = this
                    .android
                    .as_ref()
                    .filter(|a| a.read(cx).project_id == id)
                    .cloned();
                if found.is_none() && ours.is_some() {
                    this.android = None;
                } else if let (Some(android), Some(found)) = (ours, &found) {
                    android.update(cx, |a, cx| a.set_project(found.clone(), cx));
                }
                this.mobile = found;
                cx.notify();
            },
        );
    }

    /// Opens the Android panel beside the chat, or closes it.
    pub(crate) fn toggle_android(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.android_open(cx) {
            self.android = None;
            window.focus(&self.focus, cx);
            return cx.notify();
        }
        let (Some(project), Some(mobile)) = (self.project(), self.mobile.clone()) else {
            return;
        };
        let (id, core) = (project.id.clone(), std::sync::Arc::clone(&self.core));
        let workspace = cx.entity().downgrade();
        self.android =
            Some(cx.new(|cx| {
                crate::android::AndroidPanel::new(workspace, core, id, mobile, window, cx)
            }));
        cx.notify();
    }

    /// Whether the Android panel is open for the open project.
    pub(crate) fn android_open(&self, cx: &gpui::App) -> bool {
        let project = self.project().map(|p| &p.id);
        self.android
            .as_ref()
            .is_some_and(|a| Some(&a.read(cx).project_id) == project)
    }

    pub fn close_android(&mut self, cx: &mut Context<Self>) {
        self.android = None;
        cx.notify();
    }

    pub fn close_canvas(&mut self, cx: &mut Context<Self>) {
        self.canvas = None;
        cx.notify();
    }

    /// The review notes go to the open thread when it works in their project, else to a new task.
    pub fn send_review(&mut self, project: &str, text: String, cx: &mut Context<Self>) {
        let open = self
            .thread
            .as_ref()
            .filter(|t| t.project_id.as_deref() == Some(project))
            .map(|t| t.id.clone());
        match open {
            Some(id) => self.send_to(&id, text, cx),
            None => self.start_task("Review", text, cx),
        }
    }

    /// The file panel's width: the dragged one, else `max(420px, 46%)` of the row it sits in,
    /// kept between its narrowest and 70% of the row.
    pub(super) fn canvas_width(&self, window: &Window) -> (f32, f32) {
        let sidebar = if self.sidebar_view().is_some() {
            self.sidebar_width
        } else {
            0.
        };
        let body = f32::from(window.viewport_size().width) - ACTIVITY_BAR_W - sidebar;
        let max = body * CANVAS_MAX_FRACTION;
        let width = self.canvas_width.unwrap_or((body * 0.46).max(420.));
        (width.min(max).max(CANVAS_MIN), max)
    }

    /// The file panel and the handle that resizes it, while its project is the open one.
    pub(super) fn canvas_area(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let project = self.project().map(|p| &p.id);
        let thread = self.thread.as_ref().map(|t| &t.id);
        let beside = self
            .artifact_beside
            .as_ref()
            .filter(|(of, _)| Some(of) == thread);
        let (content, label): (gpui::AnyView, _) = match (&self.android, &self.canvas) {
            _ if let Some((_, page)) = beside => (page.clone().into(), "Resize artifact panel"),
            (Some(android), _) if Some(&android.read(cx).project_id) == project => {
                (android.clone().into(), "Resize Android panel")
            }
            (_, Some(canvas)) if Some(&canvas.read(cx).project_id) == project => {
                (canvas.clone().into(), "Resize file panel")
            }
            _ => return None,
        };
        let t = theme(cx);
        let (width, max) = self.canvas_width(window);
        let line = t.border;
        let handle = div()
            .id("canvas-resize")
            .role(Role::Splitter)
            .aria_label(label)
            .aria_orientation(gpui::Orientation::Vertical)
            .aria_numeric_value(f64::from(width.round()))
            .aria_min_numeric_value(f64::from(CANVAS_MIN))
            .aria_max_numeric_value(f64::from(max.round()))
            .tab_index(0)
            .relative()
            .w(px(4.))
            .flex_none()
            .cursor_col_resize()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(1.5))
                    .w(px(1.))
                    .bg(line),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
                    this.canvas_resizing = Some((f32::from(e.position.x), width));
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, _, cx| {
                let step = match e.keystroke.key.as_str() {
                    "left" => 16.,
                    "right" => -16.,
                    _ => return,
                };
                this.canvas_width = Some((width + step).min(max).max(CANVAS_MIN));
                cx.stop_propagation();
                cx.notify();
            }));
        Some(
            div()
                .flex()
                .flex_none()
                .min_h_0()
                .child(handle)
                .child(div().flex().w(px(width)).min_h_0().child(content)),
        )
    }
}
