//! Settings → About: the running version, the platform and the data folder, then where updating
//! stands and the action that moves it on, how often to look for a new version, and the notes of
//! the newest releases.

// Rust guideline compliant 2026-10-09

use gpui::component::button::Button;
use gpui::component::Disableable as _;
use gpui::component::Sizable as _;
use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use gpui::{
    AnyElement, App, ClipboardItem, Div, ElementId, FontWeight, Role, SharedString, Stateful, div,
    prelude::*, px,
};
use patchwork_client::local::core::updates::{RELEASES_URL, ReleaseNotes, UpdateState, UpdateStatus};

use patchwork_design_text::markdown::markdown;
use crate::settings::controls::{Section, minutes_stepper, token_row};
use crate::settings::{SettingsDialog, SettingsEvent};
use patchwork_design_icons::IconName;
use patchwork_design_theme::{font_mono, fs_sm, fs_xs, theme};
use patchwork_design_components::button;
use patchwork_design_icons::icon;

/// The releases opened, by version; None until the pane first draws, which opens the newest.
#[derive(Default)]
pub struct UpdatesState {
    open: Option<HashSet<String>>,
}

/// Where updating stands, in one line.
fn status_text(status: &UpdateStatus) -> String {
    match &status.state {
        UpdateState::Idle if status.can_install => "Updates install on their own".into(),
        UpdateState::Idle => "This copy can't install updates".into(),
        UpdateState::Checking => "Checking for updates…".into(),
        UpdateState::UpToDate => "Up to date".into(),
        UpdateState::Available { version } => format!("Patchwork {version} is available"),
        UpdateState::Downloading {
            version,
            downloaded,
            total,
        } => {
            let progress = match total {
                Some(total) if *total > 0 => format!("{}%", downloaded * 100 / total),
                // Tenths of a MiB.
                Some(_) | None => {
                    let tenths = (downloaded * 10) >> 20;
                    format!("{}.{} MB", tenths / 10, tenths % 10)
                }
            };
            format!("Downloading Patchwork {version} · {progress}")
        }
        UpdateState::Ready { version } => {
            format!("Patchwork {version} is installed. Restart to use it.")
        }
        UpdateState::Failed { message } => message.clone(),
    }
}

/// A release's section of CHANGELOG.md, its `## [0.2.0] - 2026-01-02` heading read as
/// `## 0.2.0 · 2026-01-02`.
fn release_notes(section: &str) -> String {
    let Some(rest) = section.strip_prefix("## [") else {
        return section.to_string();
    };
    match rest.split_once("] - ") {
        Some((version, rest)) if !version.contains('\n') => format!("## {version} · {rest}"),
        _ => section.to_string(),
    }
}

struct Release {
    version: String,
    date: String,
    notes: ReleaseNotes,
}

/// Release notes as markdown, without the top margin a leading heading keeps: the row above
/// already spaces them.
fn notes_markdown(text: &str, key: impl Into<SharedString>, cx: &App) -> Div {
    markdown(text, key, cx).when(text.starts_with('#'), |d| d.mt(px(-12.)))
}

/// The link under notes that leave changes out, to the release's page.
fn read_more(id: impl Into<ElementId>, url: &str, cx: &App) -> Stateful<Div> {
    let url = url.to_string();
    button(id, "Read more")
        .role(Role::Link)
        .mt(px(6.))
        .text_color(theme(cx).accent_text)
        .hover(gpui::Styled::underline)
        .child("Read more")
        .on_click(move |_, _, cx| cx.open_url(&url))
}

/// The newest three releases in CHANGELOG.md as it was when this copy was built, newest first.
fn releases() -> &'static [Release] {
    static RELEASES: OnceLock<Vec<Release>> = OnceLock::new();
    RELEASES.get_or_init(|| {
        include_str!("../../../../../CHANGELOG.md")
            .split("\n## [")
            .skip(1)
            .take(3)
            .map(|section| {
                let (heading, notes) = section.split_once('\n').unwrap_or((section, ""));
                let (version, rest) = heading.split_once(']').unwrap_or((heading, ""));
                Release {
                    version: version.to_string(),
                    date: rest.trim_start_matches(" - ").trim().to_string(),
                    notes: patchwork_client::local::core::updates::notes(version, notes),
                }
            })
            .collect()
    })
}

pub fn render(d: &mut SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let open = d.updates.open.get_or_insert_with(|| {
        releases()
            .iter()
            .take(1)
            .map(|r| r.version.clone())
            .collect()
    });
    let list = releases().iter().enumerate().map(|(i, r)| {
        let is_open = open.contains(&r.version);
        let version = r.version.clone();
        div()
            .child(
                button(
                    SharedString::from(format!("release-{}", r.version)),
                    r.version.clone(),
                )
                .aria_expanded(is_open)
                .w_full()
                .gap(px(6.))
                .py(px(8.))
                .px(px(16.))
                .text_color(t.text)
                .child(div().text_color(t.text_muted).child(icon(
                    if is_open {
                        IconName::CaretDown
                    } else {
                        IconName::CaretRight
                    },
                    12.,
                )))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(r.version.clone()),
                )
                .when(!r.date.is_empty(), |b| {
                    b.child(div().text_color(t.text_muted).child(r.date.clone()))
                })
                .on_click(cx.listener(move |d, _, _, cx| {
                    let open = d.updates.open.get_or_insert_with(HashSet::new);
                    if !open.remove(&version) {
                        open.insert(version.clone());
                    }
                    cx.notify();
                })),
            )
            .when(is_open, |d| {
                d.child(
                    div()
                        .pb(px(12.))
                        .pl(px(36.))
                        .pr(px(16.))
                        .flex()
                        .flex_col()
                        .items_start()
                        .child(notes_markdown(
                            &r.notes.text,
                            format!("release-notes-{i}"),
                            cx,
                        ))
                        .children(
                            r.notes
                                .more
                                .as_deref()
                                .map(|url| read_more(("release-more", i), url, cx)),
                        ),
                )
            })
    });
    let list: Vec<_> = list.collect();
    let status = d.core.update_status();
    let failed = matches!(status.state, UpdateState::Failed { .. });
    let action = match &status.state {
        UpdateState::Ready { .. } => Button::new("update-action")
            .small()
            .label("Restart to update")
            .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::RestartToUpdate))),
        UpdateState::Available { .. } => Button::new("update-action")
            .small()
            .label("Download")
            .on_click(|_, _, cx| cx.open_url(RELEASES_URL)),
        UpdateState::Idle
        | UpdateState::Checking
        | UpdateState::UpToDate
        | UpdateState::Downloading { .. }
        | UpdateState::Failed { .. } => {
            let working = matches!(
                status.state,
                UpdateState::Checking | UpdateState::Downloading { .. }
            );
            let label = if status.state == UpdateState::Checking {
                "Checking…"
            } else {
                "Check for updates"
            };
            Button::new("update-action")
                .small()
                .disabled(working)
                .label(label)
                .when(!working, |b| {
                    b.on_click(cx.listener(|d, _, _, _| Arc::clone(&d.core).check_for_updates()))
                })
        }
    };
    // The newer release's notes, which an up-to-date copy has no use for.
    let notes = status
        .notes
        .as_ref()
        .filter(|_| status.state != UpdateState::UpToDate);
    let data = d
        .core
        .backgrounds_dir()
        .parent()
        .map(|dir| dir.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mono = |text: String| {
        div()
            .min_w_0()
            .max_w(px(280.))
            .truncate()
            .font_family(font_mono())
            .text_size(px(fs_xs()))
            .text_color(t.text_muted)
            .child(text)
    };
    let (copy_data, open_data) = (data.clone(), data.clone());
    let about = Section::untitled()
        .child(token_row("Version", &t).child(mono(patchwork_platform::VERSION.to_string())))
        .child(token_row("Platform", &t).child(mono(format!(
            "{} · {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ))))
        .child(
            token_row("Data folder", &t)
                .child(mono(data))
                .child(
                    Button::new("about-copy-data")
                        .small()
                        .accessibility_label("Copy the data folder")
                        .child("Copy")
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_data.clone()));
                        }),
                )
                .child(
                    Button::new("about-open-data")
                        .small()
                        .accessibility_label("Open the data folder")
                        .child("Open")
                        .on_click(move |_, _, cx| cx.open_with_system(open_data.as_ref())),
                ),
        );
    div()
        .flex()
        .flex_col()
        .child(about)
        .child(
            Section::new("Updates")
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .px(px(16.))
                        .py(px(10.))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .child(
                                    div().font_weight(FontWeight::SEMIBOLD).child(format!(
                                        "Patchwork {}",
                                        patchwork_platform::VERSION
                                    )),
                                )
                                .when(
                                    !matches!(
                                        status.state,
                                        UpdateState::Idle | UpdateState::UpToDate
                                    ),
                                    |col| {
                                        col.child(
                                            div()
                                                .id("update-status")
                                                .role(if failed {
                                                    Role::Alert
                                                } else {
                                                    Role::Status
                                                })
                                                .text_size(px(fs_sm()))
                                                .text_color(if failed {
                                                    t.danger
                                                } else {
                                                    t.text_muted
                                                })
                                                .child(status_text(&status)),
                                        )
                                    },
                                ),
                        )
                        .child(action),
                )
                .child(
                    token_row("Check for updates every", &t).child(minutes_stepper(
                        "Check for updates every",
                        d.settings.updates.check_every_minutes,
                        &t,
                        |d, minutes, cx| d.update(|s| s.updates.check_every_minutes = minutes, cx),
                        cx,
                    )),
                )
                .children(notes.map(|notes| {
                    div()
                        .px(px(16.))
                        .pt(px(4.))
                        .pb(px(12.))
                        .flex()
                        .flex_col()
                        .items_start()
                        .child(notes_markdown(
                            &release_notes(&notes.text),
                            "update-notes",
                            cx,
                        ))
                        .children(
                            notes
                                .more
                                .as_deref()
                                .map(|url| read_more("update-more", url, cx)),
                        )
                })),
        )
        .child(Section::new("Release notes").children(list))
        .into_any_element()
}
