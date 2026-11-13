//! Settings → Updates: the running version, where updating stands and the action that moves it
//! on, how often to look for a new version, and the notes of the newest releases.

// Rust guideline compliant 2026-10-09

use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use gpui::{AnyElement, FontWeight, Role, SharedString, div, prelude::*, px};
use patchwork_server::updates::{RELEASES_URL, UpdateState, UpdateStatus};

use crate::ui::markdown::markdown;
use crate::settings::controls::{minutes_stepper, token_row};
use crate::settings::{SettingsDialog, SettingsEvent};
use crate::ui::theme::{FS_SM, theme};
use crate::ui::{BtnKind, btn, button, icon};

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
    notes: String,
}

/// The newest three releases in CHANGELOG.md as it was when this copy was built, newest first.
fn releases() -> &'static [Release] {
    static RELEASES: OnceLock<Vec<Release>> = OnceLock::new();
    RELEASES.get_or_init(|| {
        include_str!("../../../../CHANGELOG.md")
            .split("\n## [")
            .skip(1)
            .take(3)
            .map(|section| {
                let (heading, notes) = section.split_once('\n').unwrap_or((section, ""));
                let (version, rest) = heading.split_once(']').unwrap_or((heading, ""));
                Release {
                    version: version.to_string(),
                    date: rest.trim_start_matches(" - ").trim().to_string(),
                    // The folded full changelog is for the release page.
                    notes: notes
                        .split("<details>")
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string(),
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
            .when(i + 1 < releases().len(), |d| {
                d.border_b_1().border_color(t.border)
            })
            .child(
                button(
                    SharedString::from(format!("release-{}", r.version)),
                    r.version.clone(),
                )
                .aria_expanded(is_open)
                .w_full()
                .gap(px(6.))
                .py(px(8.))
                // The 6px a web page's unstyled button keeps at each side.
                .px(px(6.))
                .text_color(t.text)
                .child(div().text_color(t.text_muted).child(icon(
                    if is_open { "caret-down" } else { "caret-right" },
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
                d.child(div().pb(px(12.)).pl(px(18.)).child(markdown(
                    &r.notes,
                    format!("release-notes-{i}"),
                    cx,
                )))
            })
    });
    let list: Vec<_> = list.collect();
    let status = d.server.update_status();
    let failed = matches!(status.state, UpdateState::Failed { .. });
    let action = match &status.state {
        UpdateState::Ready { .. } => btn(
            "update-action",
            "Restart to update",
            BtnKind::Plain,
            false,
            &t,
        )
        .child("Restart to update")
        .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::RestartToUpdate))),
        UpdateState::Available { .. } => {
            btn("update-action", "Download", BtnKind::Plain, false, &t)
                .child("Download")
                .on_click(|_, _, cx| cx.open_url(RELEASES_URL))
        }
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
            btn("update-action", label, BtnKind::Plain, working, &t)
                .child(label)
                .when(!working, |b| {
                    b.on_click(cx.listener(|d, _, _, _| Arc::clone(&d.server).check_for_updates()))
                })
        }
    };
    // The newer release's notes, which an up-to-date copy has no use for.
    let notes = status
        .notes
        .as_deref()
        .filter(|_| status.state != UpdateState::UpToDate)
        .map(release_notes);
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .border_b_1()
                .border_color(t.border)
                .py(px(10.))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(format!("Patchwork {}", patchwork_server::VERSION)),
                        )
                        .child(
                            div()
                                .id("update-status")
                                .role(if failed { Role::Alert } else { Role::Status })
                                .text_size(px(FS_SM))
                                .text_color(if failed { t.danger } else { t.text_muted })
                                .child(status_text(&status)),
                        ),
                )
                .child(action),
        )
        .child(
            token_row("Check for updates every", None, &t).child(minutes_stepper(
                "Check for updates every",
                d.settings.updates.check_every_minutes,
                &t,
                |d, minutes, cx| d.update(|s| s.updates.check_every_minutes = minutes, cx),
                cx,
            )),
        )
        .children(notes.map(|notes| {
            div()
                .border_b_1()
                .border_color(t.border)
                .pt(px(4.))
                .pb(px(12.))
                .child(markdown(&notes, "update-notes", cx))
        }))
        .child(div().pt(px(4.)).children(list))
        .into_any_element()
}
