//! Setup's third step: open a folder as a project, or make a new one, and
//! pick the one to start in.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{
    AnyElement, Div, Focusable, FontWeight, Role, SharedString, Stateful, Toggled, Window, div,
    prelude::*, px,
};
use patchwork_server::Server;

use super::{Onboarding, OnboardingEvent};
use super::rows::picked;
use crate::ui::input_box;
use crate::ui::theme::{font_mono, FS_LG, FS_SM, radius, radius_lg, radius_sm, Theme};
use crate::ui::{BtnKind, btn, button, icon};

impl Onboarding {
    /// Runs a project change in the background, then reads the projects again.
    pub(super) fn run_project(
        &mut self,
        task: impl FnOnce(&Server) -> Result<(), patchwork_server::Error> + Send + 'static,
        done: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.busy = true;
        self.project_error = None;
        let server = Arc::clone(&self.server);
        let work = cx.background_spawn(async move { task(&server) });
        cx.spawn(async move |this, cx| {
            let out = work.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match out {
                    Ok(()) => {
                        this.settings = this.server.store.settings();
                        cx.emit(OnboardingEvent::Changed);
                        done(this, cx);
                    }
                    Err(e) => this.project_error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Asks for a folder, then hands it to `then`.
    pub(super) fn pick_folder(
        &mut self,
        then: impl FnOnce(&mut Self, String, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let path = match picked.await {
                Ok(Ok(Some(paths))) => paths.first().map(|p| p.to_string_lossy().into_owned()),
                Ok(Err(e)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.project_error = Some(e.to_string());
                        cx.notify();
                    });
                    None
                }
                _ => None,
            };
            if let Some(path) = path {
                let _ = this.update(cx, |this, cx| then(this, path, cx));
            }
        })
        .detach();
    }

    pub(super) fn open_existing(&mut self, cx: &mut Context<Self>) {
        self.pick_folder(
            |this, path, cx| {
                this.run_project(move |s| s.add_project(&path).map(|_| ()), |_, _| {}, cx);
            },
            cx,
        );
    }

    /// A new project is an empty folder with a git repository, made inside the folder picked here.
    pub(super) fn create_in(&mut self, cx: &mut Context<Self>) {
        let name = self.name.read(cx).content.trim().to_string();
        if name.is_empty() || self.busy {
            return;
        }
        self.pick_folder(
            move |this, parent, cx| {
                this.run_project(
                    move |s| s.create_project(&parent, &name).map(|_| ()),
                    |this, cx| {
                        this.name.update(cx, |field, cx| field.set_text("", cx));
                        this.creating = false;
                    },
                    cx,
                );
            },
            cx,
        );
    }

    /// Checks the project `id`, or clears the check when it is the one already checked.
    pub(super) fn set_active(&mut self, id: String, cx: &mut Context<Self>) {
        self.save(
            |s| {
                s.active_project_id =
                    (s.active_project_id.as_deref() != Some(id.as_str())).then_some(id);
            },
            cx,
        );
    }
}

/// One of the projects step's two ways in: a large plain tile with an icon.
fn way_in(
    id: &'static str,
    label: &'static str,
    icon_name: &str,
    busy: bool,
    t: &Theme,
) -> Stateful<Div> {
    let hover = t.bg_hover;
    button(id, label)
        .flex_1()
        .flex_col()
        .justify_center()
        .gap(px(12.))
        .rounded(px(radius_lg()))
        .px(px(24.))
        .py(px(28.))
        .text_size(px(FS_LG))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(t.text)
        .bg(t.bg_panel)
        .child(
            div()
                .size(px(48.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(radius()))
                .bg(t.bg_selected_muted)
                .text_color(t.text_muted)
                .child(icon(icon_name, 22.)),
        )
        .child(label)
        .map(|b| {
            if busy {
                b.opacity(0.6).cursor_default()
            } else {
                b.hover(move |s| s.bg(hover))
            }
        })
}

pub fn projects_step(
    this: &Onboarding,
    t: &Theme,
    window: &mut Window,
    cx: &mut Context<Onboarding>,
) -> AnyElement {
    let busy = this.busy;
    let open = way_in("setup-open-folder", "Open a folder", "folder-open", busy, t);
    let open = if busy {
        open
    } else {
        open.on_click(cx.listener(|this, _, _, cx| this.open_existing(cx)))
    };
    let new =
        way_in("setup-new-project", "New project", "plus", busy, t).aria_expanded(this.creating);
    let new = if this.creating { picked(new, t) } else { new };
    let new = if busy {
        new
    } else {
        new.on_click(cx.listener(|this, _, window, cx| {
            this.creating = !this.creating;
            if this.creating {
                let field = this.name.read(cx).focus_handle(cx);
                window.focus(&field, cx);
            }
            cx.notify();
        }))
    };

    let create = this.creating.then(|| {
        let field_focused = this.name.read(cx).focus_handle(cx).is_focused(window);
        let empty = this.name.read(cx).content.trim().is_empty();
        let off = busy || empty;
        let go = btn("setup-create-in", "Create in…", BtnKind::Primary, off, t).child("Create in…");
        let go = if off {
            go
        } else {
            go.on_click(cx.listener(|this, _, _, cx| this.create_in(cx)))
        };
        div()
            .flex()
            .gap(px(8.))
            .rounded(px(radius_lg()))
            .bg(t.bg_panel)
            .p(px(12.))
            .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                if e.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.creating = false;
                    window.focus(&this.heading, cx);
                    cx.notify();
                }
            }))
            .child(
                input_box(field_focused, t)
                    .flex_1()
                    .child(this.name.clone()),
            )
            .child(go)
    });

    let active = this.settings.active_project_id.clone();
    let projects = this.settings.projects.iter().map(|p| {
        let is_active = active.as_deref() == Some(p.id.as_str());
        let hover = t.bg_hover;
        let id = p.id.clone();
        let row = button(
            SharedString::from(format!("setup-project-{}", p.id)),
            p.name.clone(),
        )
        .aria_toggled(if is_active {
            Toggled::True
        } else {
            Toggled::False
        })
        .w_full()
        .gap(px(12.))
        .rounded(px(radius_sm()))
        .border_1()
        .border_color(gpui::transparent_black())
        .px(px(12.))
        .py(px(8.))
        .child(
            div()
                .w(px(14.))
                .flex_none()
                .text_color(t.text)
                .when(is_active, |d| d.child(icon("check-bold", 14.))),
        )
        .child(
            div()
                .flex_none()
                .font_weight(FontWeight::MEDIUM)
                .text_color(t.text)
                .child(p.name.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_right()
                .font_family(font_mono())
                .text_size(px(FS_SM))
                .text_color(t.text_muted)
                .child(p.path.clone()),
        )
        .on_click(cx.listener(move |this, _, _, cx| this.set_active(id.clone(), cx)));
        div()
            .id(SharedString::from(format!("setup-project-item-{}", p.id)))
            .role(Role::ListItem)
            .child(if is_active {
                picked(row, t)
            } else {
                row.hover(move |s| s.bg(hover))
            })
    });
    let has_projects = !this.settings.projects.is_empty();

    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(div().flex().gap(px(12.)).child(open).child(new))
        .children(create)
        .when_some(this.project_error.clone(), |d, e| {
            d.child(
                div()
                    .id("setup-project-error")
                    .role(Role::Alert)
                    .text_size(px(FS_SM))
                    .text_color(t.danger)
                    .child(e),
            )
        })
        .when(has_projects, |d| {
            d.child(
                div()
                    .id("setup-projects")
                    .role(Role::List)
                    .aria_label("Projects")
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(projects),
            )
        })
        .into_any_element()
}
