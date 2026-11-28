//! The canvas beside the chat: what opens in it, and how wide it is. A quilt opened from the
//! chat shows there too.

// Rust guideline compliant 2026-10-09

use patchwork_desktop_chat::prelude::*;
use patchwork_client::GitModel;
use gpui::{App, MouseButton, Role, WeakEntity, Window, div, prelude::*, px};
use patchwork_core::chats::Message;
use patchwork_core::source_control::FileChange;
use std::rc::Rc;
use std::sync::Arc;

use patchwork_desktop_quilts::QuiltsPage;
use patchwork_desktop_canvas::{
    CANVAS_MAX_FRACTION, CANVAS_MIN, Canvas, CanvasEvent, CanvasHost, CanvasTab,
};
use patchwork_design_theme::{ACTIVITY_BAR_W, theme};
use crate::window::Workspace;

/// The canvas's view of the workspace: the open chat's messages and the project's changes.
struct Host(WeakEntity<Workspace>);

impl CanvasHost for Host {
    fn message(&self, id: &str, cx: &App) -> Option<Arc<Message>> {
        let workspace = self.0.upgrade()?;
        let chat = workspace.read(cx).chat.as_ref()?;
        chat.messages.iter().find(|m| m.id == id).cloned()
    }

    fn change(&self, path: &str, cx: &App) -> Option<FileChange> {
        GitModel::read(cx)
            .changes
            .iter()
            .find(|c| c.path == path)
            .cloned()
    }
}

impl Workspace {
    /// Shows version `n` of the quilt `id` beside the chat, or on the Quilts page when that
    /// shows.
    pub(crate) fn open_quilt_beside(
        &mut self,
        id: String,
        n: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.chat.as_ref().map(|t| t.id.clone()) else {
            return;
        };
        if self.page == crate::nav::Page::Quilts {
            self.open_quilt(id, n, window, cx);
            return;
        }
        match &self.quilt_beside {
            Some((of, page)) if *of == chat => page.update(cx, |page, cx| page.open(id, n, cx)),
            _ => {
                cx.background_spawn(async { patchwork_core::browser::warm_browser() })
                    .detach();
                let (core, thumbs) = (Arc::clone(&self.core), self.thumbs.clone());
                let host = Rc::new(super::quilts::BrowserPreviews);
                let page = cx.new(|cx| QuiltsPage::beside_chat(core, host, thumbs, id, n, cx));
                cx.subscribe_in(&page, window, Self::on_quilts).detach();
                self.quilt_beside = Some((chat, page));
            }
        }
        cx.notify();
    }

    /// Opens a changed file's diff, or the file itself when it has no changes.
    pub fn open_change(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        // A file in conflict opens on its code, where its conflicts are settled.
        let change = GitModel::read(cx).changes.iter().find(|c| c.path == path);
        let tab = if change.is_some_and(|c| c.status != "conflicted") {
            CanvasTab::Diff
        } else {
            CanvasTab::Code
        };
        let Some(project) = self.project(cx) else {
            return;
        };
        // The file is in the folder Source Control shows, which may be the task's worktree.
        let root = GitModel::read(cx)
            .git_root
            .clone()
            .unwrap_or_else(|| project.path.clone());
        self.open_canvas_at(path, root, None, tab, window, cx);
    }

    /// Opens a changed file's diff showing one part of it: what is staged, or what is not.
    pub fn open_change_side(
        &mut self,
        path: &str,
        side: patchwork_core::source_control::DiffSide,
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
        let Some(root) = GitModel::read(cx).git_root.clone() else {
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
        let Some(project) = self.project(cx) else {
            return;
        };
        // A reply's files are in the folder its task worked in, which may be its worktree.
        let root = message
            .as_ref()
            .and_then(|_| self.chat_folder())
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
        let Some(project) = self.project(cx) else {
            return;
        };
        let id = project.id.clone();
        let host: Rc<dyn CanvasHost> = Rc::new(Host(cx.entity().downgrade()));
        let path = Some(path.to_string());
        if self.android_open(cx) {
            self.android = None;
        }
        let canvas = cx.new(|cx| Canvas::new(host, id, root, path, message, tab, window, cx));
        cx.subscribe(&canvas, Self::on_canvas).detach();
        self.canvas = Some(canvas);
        cx.notify();
    }

    /// Does what the canvas asks of the workspace.
    fn on_canvas(&mut self, _: gpui::Entity<Canvas>, event: &CanvasEvent, cx: &mut Context<Self>) {
        match event {
            CanvasEvent::Close => self.close_canvas(cx),
            CanvasEvent::GitChanged => self.load_git(cx),
            CanvasEvent::Review { project, text } => self.send_review(project, text.clone(), cx),
        }
    }

    pub fn close_canvas(&mut self, cx: &mut Context<Self>) {
        self.canvas = None;
        cx.notify();
    }

    /// The review notes go to the open chat when it works in their project, else to a new task.
    pub fn send_review(&mut self, project: &str, text: String, cx: &mut Context<Self>) {
        let open = self
            .chat
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
    pub(crate) fn canvas_width(&self, window: &Window) -> (f32, f32) {
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
    pub(crate) fn canvas_area(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let project = self.project(cx).map(|p| &p.id);
        let chat = self.chat.as_ref().map(|t| &t.id);
        let beside = self
            .quilt_beside
            .as_ref()
            .filter(|(of, _)| Some(of) == chat);
        let (content, label): (gpui::AnyView, _) = match (&self.android, &self.canvas) {
            _ if let Some((_, page)) = beside => (page.clone().into(), "Resize quilt panel"),
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
