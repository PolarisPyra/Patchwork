//! The app on the device picked: the variant runs build, reloading and stopping a run that
//! stays attached, installing an APK, and the bar under the header that holds their controls.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{AnyElement, ClickEvent, ExternalPaths, Role, Toggled, Window, div, prelude::*, px};
use patchwork_server::android::{self, Device, DeviceState, Framework, Reload, Variant};
use patchwork_server::{Error, Server};

use super::state::{AndroidPanel, Run};
use crate::ui::menu::{Menu, MenuItem};
use crate::ui::theme::{FS_SM, Theme};
use crate::ui::{dropdown_chip, icon_button, spinner, tooltip};

/// What stops a command in a terminal: the interrupt key.
const INTERRUPT: &str = "\u{3}";

impl AndroidPanel {
    /// Reads the variants the app can be built as, and the one last picked for the project.
    pub(super) fn load_variants(&self, cx: &mut Context<Self>) {
        let (server, id) = (Arc::clone(&self.server), self.project_id.clone());
        let project = self.project.clone();
        let task = cx.background_spawn(async move {
            (
                android::variants(&project),
                server.android_variant(&id, &project),
            )
        });
        cx.spawn(async move |this, cx| {
            let (variants, variant) = task.await;
            this.update(cx, |this, cx| {
                this.variants = variants;
                this.variant = variant;
                cx.notify();
            })
        })
        .detach();
    }

    /// Builds `variant` from the next run on, and remembers it for the project.
    fn pick_variant(&mut self, variant: Variant, cx: &mut Context<Self>) {
        if self.variant == variant {
            return;
        }
        self.variant = variant.clone();
        // Another variant may install under another id, whose lines the log then shows.
        if self.log.take().is_some() {
            self.open_log(cx);
        }
        let (server, id) = (Arc::clone(&self.server), self.project_id.clone());
        cx.background_spawn(async move {
            if let Err(e) = server.set_android_variant(&id, &variant) {
                log::warn!(error:% = e; "could not remember the build variant: {{error}}");
            }
        })
        .detach();
        cx.notify();
    }

    fn open_variant_menu(&mut self, e: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        let items = self
            .variants
            .iter()
            .map(|variant| {
                let (this, picked) = (this.clone(), variant.clone());
                MenuItem::new(variant.name(), move |_, cx| {
                    let _ = this.update(cx, |this, cx| this.pick_variant(picked.clone(), cx));
                })
                .radio(*variant == self.variant)
            })
            .collect();
        self.open_menu("Build variant", items, e, window, cx);
    }

    /// Opens a menu of the panel's at the pointer; its `label` also says which one is open.
    pub(super) fn open_menu(
        &mut self,
        label: &str,
        items: Vec<MenuItem>,
        e: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let menu = Menu::open_for(label.to_string(), items, window, cx, |this, window, cx| {
            this.menu = None;
            window.focus(&this.focus, cx);
            cx.notify();
        });
        self.menu = Some((label.to_string(), menu, e.position()));
        cx.notify();
    }

    pub(super) fn menu_open(&self, label: &str) -> bool {
        self.menu.as_ref().is_some_and(|(of, _, _)| of == label)
    }

    /// The device picked, when it is running and ready.
    pub(super) fn picked_device(&self) -> Option<&Device> {
        let key = self.selected.as_deref()?;
        self.devices().iter().find(|d| {
            d.state == DeviceState::Online && (d.serial == key || d.avd.as_deref() == Some(key))
        })
    }

    /// Whether the run under way stays attached to the app, to reload it.
    pub(super) fn attached(&self) -> bool {
        self.run == Run::Running && self.project.framework.reload().is_some()
    }

    /// What to type into the run's terminal now that the project-relative `paths` were saved.
    /// None when no run is attached, or the framework reloads by itself.
    pub fn reload_on_save(&self, paths: &[String]) -> Option<&'static str> {
        let framework = self.project.framework;
        if self.run != Run::Running || !paths.iter().any(|path| framework.reloads_on_save(path)) {
            return None;
        }
        match framework.reload() {
            Some(Reload::Typed(keys)) => Some(keys),
            _ => None,
        }
    }

    /// Reloads the running app with the code as it is now, keeping its state; a restart starts
    /// it over.
    fn reload(&mut self, restart: bool, cx: &mut Context<Self>) {
        let framework = self.project.framework;
        let keys = if restart {
            framework.restart()
        } else {
            match framework.reload() {
                Some(Reload::Typed(keys)) => Some(keys),
                Some(Reload::DeviceKeys) => {
                    let Some(serial) = self.running_on.clone() else {
                        return;
                    };
                    let root = self.project.root.clone();
                    return self.device_action("Reloading the app", None, cx, move |server| {
                        server.android_reload(&root, &serial)
                    });
                }
                None => None,
            }
        };
        self.type_in_run(keys, cx);
    }

    /// Ends the run that stays attached to the app.
    fn stop_run(&mut self, cx: &mut Context<Self>) {
        self.stop_asked = true;
        let keys = if self.project.framework == Framework::Flutter {
            "q"
        } else {
            INTERRUPT
        };
        self.type_in_run(Some(keys), cx);
    }

    fn type_in_run(&self, keys: Option<&'static str>, cx: &mut Context<Self>) {
        if let (Some(keys), Some(workspace)) = (keys, self.workspace.upgrade()) {
            workspace.update(cx, |w, cx| w.type_in_run(keys, cx));
        }
    }

    /// Asks a device to do something off the UI thread, showing `doing` until it has and `done`
    /// after.
    pub(super) fn device_action(
        &mut self,
        doing: &str,
        done: Option<String>,
        cx: &mut Context<Self>,
        act: impl FnOnce(&Server) -> Result<(), Error> + Send + 'static,
    ) {
        self.working = Some(doing.to_string());
        self.failure = None;
        self.notice = None;
        let server = Arc::clone(&self.server);
        let task = cx.background_spawn(async move { act(&server) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.working = None;
                match result {
                    Ok(()) => this.notice = done,
                    Err(e) => this.failure = Some(e),
                }
                this.load_states(cx);
                this.refresh(cx);
                cx.notify();
            })
        })
        .detach();
        cx.notify();
    }

    /// Installs the APK files among `paths` on the device picked.
    pub(super) fn install(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let apks: Vec<PathBuf> = paths
            .iter()
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("apk"))
            })
            .cloned()
            .collect();
        let names: Vec<String> = apks
            .iter()
            .filter_map(|apk| Some(apk.file_name()?.to_string_lossy().into_owned()))
            .collect();
        self.notice = None;
        if apks.is_empty() {
            self.failure = Some("Only .apk files can be installed on a device.".into());
            return cx.notify();
        }
        let Some(device) = self.picked_device() else {
            self.failure = Some("Start or connect the device picked, then install.".into());
            return cx.notify();
        };
        let (serial, root) = (device.serial.clone(), self.project.root.clone());
        let names = names.join(", ");
        self.device_action(
            &format!("Installing {names}"),
            Some(format!("Installed {names}")),
            cx,
            move |server| {
                apks.iter()
                    .try_for_each(|apk| server.android_install(&root, &serial, apk))
            },
        );
    }

    /// Asks for APK files, then installs them on the device picked.
    pub(super) fn choose_apk(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Install".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.install(&paths, cx));
        })
        .detach();
    }

    pub(super) fn dropped(&mut self, files: &ExternalPaths, cx: &mut Context<Self>) {
        self.install(files.paths(), cx);
    }

    /// The bar under the header: the variant runs build, the controls of an attached run, and
    /// the log's switch.
    pub(super) fn run_bar(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.sdk()?;
        if self.create.is_some() || self.sheet.is_some() {
            return None;
        }
        let variant = (self.variants.len() > 1).then(|| {
            dropdown_chip(
                "android-variant",
                format!("Build variant: {}", self.variant.name()),
                self.variant.name(),
                self.menu_open("Build variant"),
                t,
            )
            .tooltip(tooltip("Build variant"))
            .on_click(cx.listener(Self::open_variant_menu))
        });
        let mut controls: Vec<AnyElement> = Vec::new();
        if self.attached() {
            controls.push(
                icon_button(
                    "android-reload",
                    "Reload the app",
                    "arrows-clockwise",
                    14.,
                    cx,
                )
                .tooltip(tooltip("Reload"))
                .on_click(cx.listener(|this, _, _, cx| this.reload(false, cx)))
                .into_any_element(),
            );
            if self.project.framework.restart().is_some() {
                controls.push(
                    icon_button(
                        "android-restart",
                        "Restart the app",
                        "arrow-counter-clockwise",
                        14.,
                        cx,
                    )
                    .tooltip(tooltip("Restart"))
                    .on_click(cx.listener(|this, _, _, cx| this.reload(true, cx)))
                    .into_any_element(),
                );
            }
            controls.push(
                icon_button("android-stop-run", "Stop the run", "stop", 14., cx)
                    .tooltip(tooltip("Stop the run"))
                    .on_click(cx.listener(|this, _, _, cx| this.stop_run(cx)))
                    .into_any_element(),
            );
        }
        let log_open = self.log.is_some();
        controls.push(
            icon_button("android-log", "App log", "list-bullets", 14., cx)
                .aria_toggled(if log_open {
                    Toggled::True
                } else {
                    Toggled::False
                })
                .when(log_open, |button| button.bg(t.bg_selected_muted))
                .tooltip(tooltip("Log"))
                .on_click(cx.listener(|this, _, _, cx| this.toggle_log(cx)))
                .into_any_element(),
        );
        Some(
            div()
                .id("android-app-bar")
                .role(Role::Toolbar)
                .aria_label("App")
                .flex()
                .flex_none()
                .items_center()
                .gap(px(4.))
                .min_h(px(40.))
                .px(px(14.))
                .border_b_1()
                .border_color(t.border)
                .children(variant)
                .child(div().flex_1())
                .children(controls)
                .into_any_element(),
        )
    }

    /// What a device is being asked to do, and what the last action did.
    pub(super) fn progress(&self, t: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let row = |id: &'static str, text: String| {
            div()
                .id(id)
                .role(Role::Status)
                .aria_label(text.clone())
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(FS_SM))
                .text_color(t.text_muted)
        };
        let working = self.working.as_ref().map(|doing| {
            row("android-working", doing.clone())
                .child(spinner(14., t.text_muted))
                .child(div().flex_1().min_w_0().truncate().child(doing.clone()))
                .into_any_element()
        });
        let notice = self.notice.as_ref().map(|done| {
            row("android-notice", done.clone())
                .child(div().flex_1().min_w_0().truncate().child(done.clone()))
                .child(
                    icon_button("android-notice-dismiss", "Dismiss", "x", 12., cx)
                        .size(px(20.))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.notice = None;
                            cx.notify();
                        })),
                )
                .into_any_element()
        });
        working.into_iter().chain(notice).collect()
    }
}
