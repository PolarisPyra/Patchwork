//! The create flow's state: the profiles and images read for it, what is picked, and the
//! download and creation under way.

// Rust guideline compliant 2026-10-09

use std::sync::mpsc::Sender;

use futures::StreamExt;
use futures::channel::mpsc::unbounded;
use gpui::{Entity, ScrollHandle, Window, prelude::*};
use patchwork_server::Error;
use patchwork_server::android::{self, HardwareProfile, InstallEvent, SystemImage};

use crate::android::state::AndroidPanel;
use crate::ui::text_area::{Submitted, TextArea};
use crate::ui::theme::theme;

/// The flow's two steps.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Profile,
    Image,
}

/// A system image's download, while it runs.
pub(super) struct Install {
    pub(super) percent: u8,
    pub(super) step: String,
    /// The licence the download waits on.
    pub(super) license: Option<String>,
    /// Where the answer to the licence goes.
    pub(super) answers: Sender<bool>,
}

pub(in crate::android) struct Create {
    pub(super) step: Step,
    /// The hardware profiles, once `avdmanager` has listed them.
    pub(super) profiles: Option<Result<Vec<HardwareProfile>, Error>>,
    pub(super) profile: Option<HardwareProfile>,
    /// The installed images at first, then every image once the list is fetched.
    pub(super) images: Vec<SystemImage>,
    pub(super) fetching: bool,
    pub(super) fetch_error: Option<Error>,
    /// The ABI that runs at full speed on this machine, and whether the others show too.
    pub(super) abi: &'static str,
    pub(super) every_abi: bool,
    pub(super) image: Option<SystemImage>,
    pub(super) name: Entity<TextArea>,
    pub(super) install: Option<Install>,
    /// The image is downloading or the device is being created.
    pub(super) working: bool,
    pub(super) error: Option<Error>,
    pub(super) profile_scroll: ScrollHandle,
    pub(super) image_scroll: ScrollHandle,
}

impl AndroidPanel {
    /// Opens the flow, and reads the profiles and images behind it.
    pub(in crate::android) fn open_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(sdk) = self.sdk().cloned() else {
            return;
        };
        let t = *theme(cx);
        let name = cx.new(|cx| {
            let mut field = TextArea::new("", "Device name", cx);
            field.single_line = true;
            field.text_color = t.text;
            field.placeholder_color = crate::ui::theme::input_placeholder();
            field.selection_color = t.accent.opacity(0.3);
            field
        });
        cx.subscribe_in(&name, window, |this, _, _: &Submitted, _, cx| {
            this.submit_create(cx);
        })
        .detach();
        self.create = Some(Create {
            step: Step::Profile,
            profiles: None,
            profile: None,
            images: Vec::new(),
            fetching: true,
            fetch_error: None,
            abi: android::abi_for(std::env::consts::ARCH),
            every_abi: false,
            image: None,
            name,
            install: None,
            working: false,
            error: None,
            profile_scroll: ScrollHandle::new(),
            image_scroll: ScrollHandle::new(),
        });
        self.failure = None;

        let for_profiles = sdk.clone();
        let profiles = cx.background_spawn(async move { android::profiles(&for_profiles) });
        cx.spawn(async move |this, cx| {
            let listed = profiles.await;
            this.update(cx, |this, cx| {
                if let Some(create) = &mut this.create {
                    create.profiles = Some(listed);
                    cx.notify();
                }
            })
        })
        .detach();

        let for_installed = sdk.clone();
        let installed = cx.background_spawn(async move {
            (
                android::host_abi(),
                android::installed_images(&for_installed),
            )
        });
        cx.spawn(async move |this, cx| {
            let (abi, images) = installed.await;
            this.update(cx, |this, cx| {
                if let Some(create) = &mut this.create
                    && create.images.is_empty()
                {
                    create.abi = abi;
                    create.image = images.iter().find(|image| image.abi == abi).cloned();
                    create.images = images;
                    this.name_device(cx);
                    cx.notify();
                }
            })
        })
        .detach();

        let available = cx.background_spawn(async move { android::available_images(&sdk) });
        cx.spawn(async move |this, cx| {
            let fetched = available.await;
            this.update(cx, |this, cx| {
                if let Some(create) = &mut this.create {
                    create.fetching = false;
                    match fetched {
                        Ok(images) => {
                            // The one picked, as the fetched list names it.
                            if let Some(picked) = &create.image {
                                create.image =
                                    images.iter().find(|image| image.same(picked)).cloned();
                            }
                            create.images = images;
                        }
                        Err(e) => create.fetch_error = Some(e),
                    }
                    cx.notify();
                }
            })
        })
        .detach();
        cx.notify();
    }

    /// The name the device gets unless one is typed, shown in the empty field.
    fn suggested_name(&self) -> Option<String> {
        let create = self.create.as_ref()?;
        let (profile, image) = (create.profile.as_ref()?, create.image.as_ref()?);
        Some(android::suggest_name(
            &profile.name,
            &image.api,
            self.avds(),
        ))
    }

    pub(super) fn name_device(&mut self, cx: &mut Context<Self>) {
        let suggested = self.suggested_name().unwrap_or_default();
        if let Some(create) = &self.create {
            create.name.update(cx, |field, cx| {
                field.placeholder = suggested.into();
                cx.notify();
            });
        }
    }

    /// Downloads the image when it is not installed, then creates the device and picks it.
    pub(super) fn submit_create(&mut self, cx: &mut Context<Self>) {
        let (Some(sdk), Some(suggested)) = (self.sdk().cloned(), self.suggested_name()) else {
            return;
        };
        let Some(create) = &mut self.create else {
            return;
        };
        let (Some(profile), Some(image), false) =
            (create.profile.clone(), create.image.clone(), create.working)
        else {
            return;
        };
        let typed = create.name.read(cx).content.trim().to_string();
        let name = if typed.is_empty() { suggested } else { typed };
        create.working = true;
        create.error = None;
        let (events, mut received) = unbounded::<InstallEvent>();
        let (answers, waiting) = std::sync::mpsc::channel::<bool>();
        if !image.installed {
            create.install = Some(Install {
                percent: 0,
                step: "Starting the download".into(),
                license: None,
                answers,
            });
        }
        let task = cx.background_spawn({
            let name = name.clone();
            async move {
                if !image.installed {
                    let mut report = |event| {
                        let _ = events.unbounded_send(event);
                    };
                    android::install_image(&sdk, &image.package, &mut report, &waiting)?;
                }
                drop(events);
                android::create(&sdk, &name, &image, &profile.id)
            }
        });
        cx.spawn(async move |this, cx| {
            while let Some(event) = received.next().await {
                let applied = this.update(cx, |this, cx| {
                    if let Some(install) = this.create.as_mut().and_then(|c| c.install.as_mut()) {
                        match event {
                            InstallEvent::Progress { percent, step } => {
                                install.percent = percent;
                                install.step = step;
                            }
                            InstallEvent::License { text } => install.license = Some(text),
                        }
                        cx.notify();
                    }
                });
                if applied.is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let created = task.await;
            this.update(cx, |this, cx| {
                match created {
                    Ok(()) => {
                        this.create = None;
                        this.select(name, cx);
                        this.refresh(cx);
                    }
                    Err(e) => {
                        if let Some(create) = &mut this.create {
                            create.working = false;
                            create.install = None;
                            create.error = Some(e);
                        } else {
                            this.failure = Some(e);
                        }
                    }
                }
                cx.notify();
            })
        })
        .detach();
        cx.notify();
    }

    /// Accepts or declines the licence the download waits on.
    pub(super) fn answer_license(&mut self, accepted: bool, cx: &mut Context<Self>) {
        if let Some(install) = self.create.as_mut().and_then(|c| c.install.as_mut()) {
            install.license = None;
            // The download has ended when nothing takes the answer.
            let _ = install.answers.send(accepted);
            cx.notify();
        }
    }
}
