//! The Android panel's state: the SDK and devices as last read, the device picked, and what is
//! under way.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::{Entity, FocusHandle, Pixels, Point, ScrollHandle, Task, WeakEntity, Window, prelude::*};
use patchwork_server::android::{self, Avd, Boot, Device, MobileProject, Sdk, Snapshot};
use patchwork_server::{Error, Server};

use super::create::Create;
use crate::ui::menu::Menu;
use crate::ui::text_area::{Submitted, TextArea};
use crate::ui::theme::theme;
use crate::workspace::Workspace;

/// How often the devices are read again while the panel is open: often enough that a phone
/// plugged in or an emulator closed shows within a moment, seldom enough that adb stays idle.
const POLL: Duration = Duration::from_secs(2);

/// What is being done to a virtual device.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Busy {
    Starting,
    Stopping,
    Deleting,
}

/// What the panel asks about before doing it to a virtual device.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Confirm {
    /// Erase its apps and data, then start it.
    Wipe,
    /// Delete the device itself.
    Delete,
}

/// Where running the app stands.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Run {
    Idle,
    /// Its device is starting, or its command is being put together.
    Preparing,
    /// Its command runs in a terminal.
    Running,
}

pub struct AndroidPanel {
    pub(super) workspace: WeakEntity<Workspace>,
    pub(super) server: Arc<Server>,
    pub project_id: String,
    pub(super) project: MobileProject,
    /// The SDK and devices as last read. None until the first read returns.
    pub(super) snapshot: Option<Snapshot>,
    /// Why this machine can't run an emulator at speed, once checked.
    pub(super) acceleration: Option<Error>,
    acceleration_checked: bool,
    /// The device the app runs on: a virtual device's name, or a connected device's serial.
    pub(super) selected: Option<String>,
    pub(super) busy: HashMap<String, Busy>,
    /// The virtual device the panel asks about, and what it asks.
    pub(super) confirm: Option<(String, Confirm)>,
    /// The last failure of an action, until dismissed.
    pub(super) failure: Option<Error>,
    /// The errors whose tool output is shown, by the id of their alert.
    pub(super) open_details: HashSet<String>,
    pub(super) sdk_field: Entity<TextArea>,
    pub(super) sdk_editing: bool,
    pub(super) run: Run,
    pub(super) create: Option<Create>,
    /// A virtual device's menu, while open, and where it opened.
    pub(super) menu: Option<(String, Entity<Menu>, Point<Pixels>)>,
    pub(super) scroll: ScrollHandle,
    pub(super) focus: FocusHandle,
    _poll: Task<()>,
}

impl AndroidPanel {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        server: Arc<Server>,
        project_id: String,
        project: MobileProject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let t = *theme(cx);
        let sdk_field = cx.new(|cx| {
            let mut field = TextArea::new("Android SDK folder", "Android SDK folder", cx);
            field.single_line = true;
            field.text_color = t.text;
            field.placeholder_color = crate::ui::theme::input_placeholder();
            field.selection_color = t.accent.opacity(0.3);
            field
        });
        cx.subscribe(&sdk_field, |this, _, _: &Submitted, cx| this.save_sdk(cx))
            .detach();
        let poll = cx.spawn(async move |this, cx| {
            loop {
                let Ok((server, root)) = this.update(cx, |this, _| {
                    (Arc::clone(&this.server), this.project.root.clone())
                }) else {
                    break;
                };
                let snapshot = cx
                    .background_spawn(async move { server.android_snapshot(&root) })
                    .await;
                if this
                    .update(cx, |this, cx| this.apply(snapshot, cx))
                    .is_err()
                {
                    break;
                }
                cx.background_executor().timer(POLL).await;
            }
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            selected: server.android_device(&project_id),
            workspace,
            server,
            project_id,
            project,
            snapshot: None,
            acceleration: None,
            acceleration_checked: false,
            busy: HashMap::new(),
            confirm: None,
            failure: None,
            open_details: HashSet::new(),
            sdk_field,
            sdk_editing: false,
            run: Run::Idle,
            create: None,
            menu: None,
            scroll: ScrollHandle::new(),
            focus,
            _poll: poll,
        }
    }

    /// The project's app was found again, perhaps as another framework or in another folder.
    pub fn set_project(&mut self, project: MobileProject, cx: &mut Context<Self>) {
        if self.project != project {
            self.project = project;
            cx.notify();
        }
    }

    pub(super) fn sdk(&self) -> Option<&Sdk> {
        self.snapshot.as_ref()?.sdk.as_ref().ok()
    }

    pub(super) fn avds(&self) -> &[Avd] {
        self.snapshot.as_ref().map_or(&[], |s| s.avds.as_slice())
    }

    pub(super) fn devices(&self) -> &[Device] {
        self.snapshot
            .as_ref()
            .and_then(|s| s.devices.as_ref().ok())
            .map_or(&[], Vec::as_slice)
    }

    /// The running emulator of a virtual device.
    pub(super) fn device_of(&self, avd: &str) -> Option<&Device> {
        self.devices()
            .iter()
            .find(|d| d.avd.as_deref() == Some(avd))
    }

    /// Takes a fresh read of the SDK and devices. The device picked stays while it still exists;
    /// else one that is running is picked, else the first virtual device, else none.
    fn apply(&mut self, snapshot: Snapshot, cx: &mut Context<Self>) {
        self.snapshot = Some(snapshot);
        let exists = |key: &String| {
            self.avds().iter().any(|avd| &avd.name == key)
                || self.devices().iter().any(|d| &d.serial == key)
        };
        if !self.selected.as_ref().is_some_and(exists) {
            let running = self.devices().first().map(device_key);
            let first = self.avds().first().map(|avd| avd.name.clone());
            self.selected = running.or(first);
        }
        if !self.acceleration_checked
            && let Some(sdk) = self.sdk().cloned()
        {
            self.acceleration_checked = true;
            let task = cx.background_spawn(async move { android::acceleration(&sdk) });
            cx.spawn(async move |this, cx| {
                let checked = task.await;
                this.update(cx, |this, cx| {
                    this.acceleration = checked.err();
                    cx.notify();
                })
            })
            .detach();
        }
        cx.notify();
    }

    /// Reads the SDK and devices now, rather than at the next poll, after something changed them.
    pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
        let (server, root) = (Arc::clone(&self.server), self.project.root.clone());
        let task = cx.background_spawn(async move { server.android_snapshot(&root) });
        cx.spawn(async move |this, cx| {
            let snapshot = task.await;
            this.update(cx, |this, cx| this.apply(snapshot, cx))
        })
        .detach();
    }

    /// Picks the device the app runs on, and remembers it for the project.
    pub(super) fn select(&mut self, key: String, cx: &mut Context<Self>) {
        if self.selected.as_ref() == Some(&key) {
            return;
        }
        self.selected = Some(key.clone());
        let (server, project) = (Arc::clone(&self.server), self.project_id.clone());
        cx.background_spawn(async move {
            if let Err(e) = server.set_android_device(&project, &key) {
                log::warn!(error:% = e; "could not remember the device: {{error}}");
            }
        })
        .detach();
        cx.notify();
    }

    /// Starts a virtual device and waits for Android to start in it; then runs the app on it
    /// when `then_run`.
    pub(super) fn start(&mut self, avd: &str, boot: Boot, then_run: bool, cx: &mut Context<Self>) {
        if self.busy.contains_key(avd) {
            return;
        }
        self.busy.insert(avd.to_string(), Busy::Starting);
        self.failure = None;
        self.confirm = None;
        let (server, root) = (Arc::clone(&self.server), self.project.root.clone());
        let name = avd.to_string();
        let task = cx.background_spawn({
            let name = name.clone();
            async move {
                let booting = server.start_emulator(&root, &name, boot)?;
                server.await_boot(&root, &booting)?;
                Ok::<_, Error>(booting.serial)
            }
        });
        cx.spawn(async move |this, cx| {
            let started = task.await;
            this.update(cx, |this, cx| {
                this.busy.remove(&name);
                match started {
                    Ok(serial) if then_run => this.launch(serial, Some(name.clone()), cx),
                    Ok(_) => {}
                    Err(e) => {
                        this.failure = Some(e);
                        this.run = Run::Idle;
                    }
                }
                this.refresh(cx);
                cx.notify();
            })
        })
        .detach();
        cx.notify();
    }

    /// Asks the emulator `serial` to close. `key` is what its row is busy under: the virtual
    /// device it runs, else its serial.
    pub(super) fn stop(&mut self, key: &str, serial: String, cx: &mut Context<Self>) {
        let avd = key;
        self.busy.insert(avd.to_string(), Busy::Stopping);
        self.failure = None;
        let (server, root) = (Arc::clone(&self.server), self.project.root.clone());
        let name = avd.to_string();
        let task = cx.background_spawn(async move { server.stop_emulator(&root, &serial) });
        cx.spawn(async move |this, cx| {
            let stopped = task.await;
            this.update(cx, |this, cx| {
                this.busy.remove(&name);
                this.failure = stopped.err();
                this.refresh(cx);
                cx.notify();
            })
        })
        .detach();
        cx.notify();
    }

    /// Deletes a stopped virtual device, with its apps and data.
    pub(super) fn delete(&mut self, avd: &str, cx: &mut Context<Self>) {
        self.busy.insert(avd.to_string(), Busy::Deleting);
        self.failure = None;
        self.confirm = None;
        let (server, root) = (Arc::clone(&self.server), self.project.root.clone());
        let name = avd.to_string();
        let task = cx.background_spawn({
            let name = name.clone();
            async move { server.delete_avd(&root, &name) }
        });
        cx.spawn(async move |this, cx| {
            let deleted = task.await;
            this.update(cx, |this, cx| {
                this.busy.remove(&name);
                this.failure = deleted.err();
                this.refresh(cx);
                cx.notify();
            })
        })
        .detach();
        cx.notify();
    }

    /// Builds, installs and runs the project on the device picked, starting it first when it is a
    /// virtual device that is stopped.
    pub(super) fn run(&mut self, cx: &mut Context<Self>) {
        if self.run != Run::Idle {
            return;
        }
        self.failure = None;
        let Some(key) = self.selected.clone() else {
            self.failure = Some("Pick a device to run on, or create one.".into());
            return cx.notify();
        };
        let device = self
            .devices()
            .iter()
            .find(|d| d.serial == key || d.avd.as_deref() == Some(&key))
            .cloned();
        match device {
            Some(device) => match android::problem(&device) {
                Some(problem) => self.failure = Some(problem),
                None => self.launch(device.serial, device.avd, cx),
            },
            None if self.avds().iter().any(|avd| avd.name == key) => {
                self.run = Run::Preparing;
                self.start(&key, Boot::Quick, true, cx);
            }
            None => self.failure = Some("That device is no longer connected. Pick another.".into()),
        }
        cx.notify();
    }

    /// Runs the framework's own command for the device `serial` in a terminal of its own. `avd` is
    /// the virtual device an emulator runs.
    fn launch(&mut self, serial: String, avd: Option<String>, cx: &mut Context<Self>) {
        self.run = Run::Preparing;
        let (server, project) = (Arc::clone(&self.server), self.project.clone());
        let task = cx
            .background_spawn(async move { server.android_run(&project, &serial, avd.as_deref()) });
        cx.spawn(async move |this, cx| {
            let prepared = task.await;
            this.update(cx, |this, cx| {
                match (prepared, this.workspace.upgrade()) {
                    (Ok((launch, _)), Some(workspace)) => {
                        this.run = Run::Running;
                        workspace.update(cx, |w, cx| w.run_in_terminal(launch, cx));
                    }
                    (Err(e), _) => {
                        this.run = Run::Idle;
                        this.failure = Some(e);
                    }
                    (Ok(_), None) => this.run = Run::Idle,
                }
                cx.notify();
            })
        })
        .detach();
    }

    /// The command a run started in the terminal has ended, with `code`, or did not start.
    pub fn run_finished(&mut self, code: Option<u32>, output: &str, cx: &mut Context<Self>) {
        if self.run != Run::Running {
            return;
        }
        self.run = Run::Idle;
        match code {
            Some(0) => {}
            Some(code) => {
                self.failure = Some(Error::Tool {
                    message: format!(
                        "The build or the run failed (exit code {code}). The terminal has its \
                         whole output: fix the first error it names, then run again."
                    ),
                    detail: output.to_string(),
                });
            }
            None => {
                self.failure = Some(Error::Tool {
                    message: "The run ended without an exit code: its terminal could not be \
                              opened, or the command was killed."
                        .into(),
                    detail: output.to_string(),
                });
            }
        }
        cx.notify();
    }

    /// Opens the field for the SDK's folder with the one in use in it.
    pub(super) fn edit_sdk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self
            .server
            .store
            .settings()
            .android_sdk
            .or_else(|| {
                self.sdk()
                    .map(|sdk| sdk.root.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        self.sdk_field
            .update(cx, |field, cx| field.set_text(&current, cx));
        self.sdk_editing = true;
        window.focus(
            &gpui::Focusable::focus_handle(self.sdk_field.read(cx), cx),
            cx,
        );
        cx.notify();
    }

    /// Keeps the SDK folder typed in the field, and looks for the SDK again.
    pub(super) fn save_sdk(&mut self, cx: &mut Context<Self>) {
        let path = self.sdk_field.read(cx).content.clone();
        let server = Arc::clone(&self.server);
        let task = cx.background_spawn(async move { server.set_android_sdk(&path) });
        cx.spawn(async move |this, cx| {
            let saved = task.await;
            this.update(cx, |this, cx| {
                match saved {
                    Ok(()) => {
                        this.sdk_editing = false;
                        this.failure = None;
                        this.acceleration_checked = false;
                        this.refresh(cx);
                    }
                    Err(e) => this.failure = Some(e),
                }
                cx.notify();
            })
        })
        .detach();
    }

    pub(super) fn toggle_detail(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.open_details.remove(id) {
            self.open_details.insert(id.to_string());
        }
        cx.notify();
    }

    pub(super) fn close(&mut self, cx: &mut Context<Self>) {
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, Workspace::close_android);
        }
    }

    /// The folder name the project's app is in, for the header.
    pub(super) fn folder(&self) -> String {
        PathBuf::from(&self.project.root)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// What picks a connected device: the virtual device it runs, else its serial.
pub(super) fn device_key(device: &Device) -> String {
    device.avd.clone().unwrap_or_else(|| device.serial.clone())
}
