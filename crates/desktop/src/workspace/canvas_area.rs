//! The canvas beside the chat: what opens in it, and how wide it is.

// Rust guideline compliant 2026-10-09

use gpui::{MouseButton, Role, Window, div, prelude::*, px};
use patchwork_server::threads::Message;
use crate::canvas::{CANVAS_MAX_FRACTION, CANVAS_MIN, Canvas, CanvasTab};
use crate::ui::theme::{ACTIVITY_BAR_W, theme};
use super::state::Workspace;

impl Workspace {
    /// Opens a changed file's diff, or the file itself when it has no changes.
    pub fn open_change(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let tab = if self.changes.iter().any(|c| c.path == path) {
            CanvasTab::Diff
        } else {
            CanvasTab::Code
        };
        self.open_canvas(path, None, tab, window, cx);
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
        let (id, root) = (project.id.clone(), project.path.clone());
        let workspace = cx.entity().downgrade();
        let path = Some(path.to_string());
        self.canvas =
            Some(cx.new(|cx| Canvas::new(workspace, id, root, path, message, tab, window, cx)));
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
        let sidebar = if self.view.is_some() {
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
        let canvas = self.canvas.clone()?;
        if Some(&canvas.read(cx).project_id) != self.project().map(|p| &p.id) {
            return None;
        }
        let t = theme(cx);
        let (width, max) = self.canvas_width(window);
        let line = t.border;
        let handle = div()
            .id("canvas-resize")
            .role(Role::Splitter)
            .aria_label("Resize file panel")
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
                .child(div().flex().w(px(width)).min_h_0().child(canvas)),
        )
    }
}
