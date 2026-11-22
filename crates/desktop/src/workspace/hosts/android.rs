//! The Android panel beside the chat: whether the open project is a mobile app, opening and
//! closing the panel, and what it asks the window to do.

// Rust guideline compliant 2026-10-09

use gpui::{Entity, Window, prelude::*};

use patchwork_pages::android::{AndroidEvent, AndroidPanel};
use crate::workspace::window::Workspace;

impl Workspace {
    /// Looks for a mobile app when the open project is another than the one last looked in. The
    /// Android panel belongs to the project it opened for: it hides while another is open and
    /// shows again on the way back.
    pub(crate) fn sync_mobile(&mut self, cx: &mut Context<Self>) {
        let id = self.project().map(|p| p.id.clone());
        if id != self.mobile_for {
            self.mobile_for = id;
            self.mobile = None;
            self.detect_mobile(cx);
        }
    }

    /// Reads the open project's manifests and build files, off the main thread, for whether it
    /// is a mobile app; the toolbar shows the phone only for one.
    pub(crate) fn detect_mobile(&mut self, cx: &mut Context<Self>) {
        let Some(project) = self.project() else {
            return;
        };
        let (id, root) = (project.id.clone(), std::path::PathBuf::from(&project.path));
        patchwork_ui::off_thread(
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
        let android = cx.new(|cx| AndroidPanel::new(core, id, mobile, window, cx));
        cx.subscribe(&android, Workspace::on_android_event).detach();
        self.android = Some(android);
        cx.notify();
    }

    /// Whether the Android panel is open for the open project.
    pub(crate) fn android_open(&self, cx: &gpui::App) -> bool {
        let project = self.project().map(|p| &p.id);
        self.android
            .as_ref()
            .is_some_and(|a| Some(&a.read(cx).project_id) == project)
    }

    fn close_android(&mut self, cx: &mut Context<Self>) {
        self.android = None;
        cx.notify();
    }

    /// Does what the Android panel asks: closes it, or runs or types into the app.
    fn on_android_event(
        &mut self,
        _: Entity<AndroidPanel>,
        event: &AndroidEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            AndroidEvent::Close => self.close_android(cx),
            AndroidEvent::Run(launch) => self.run_in_terminal(launch.clone(), cx),
            AndroidEvent::Type(keys) => self.type_in_run(keys, cx),
        }
    }
}
