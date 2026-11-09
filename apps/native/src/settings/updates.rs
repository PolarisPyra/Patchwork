//! Settings → Updates (`system/UpdatesPane.tsx`): the running version, how often to look for a
//! new one, and the notes of the newest releases. Installing updates is the desktop package's
//! job, so this window says it can't, as a copy without the updater does; checking still says
//! whether a newer release is out and links to it.

use std::collections::HashSet;
use std::sync::OnceLock;
use std::time::Duration;

use gpui::{AnyElement, Context, FontWeight, SharedString, div, prelude::*, px};

use crate::chat::markdown::markdown;
use crate::settings::SettingsDialog;
use crate::settings::controls::{minutes_stepper, token_row};
use crate::theme::{FS_SM, theme};
use crate::ui::{BtnKind, btn, button, icon};

const LATEST_RELEASE: &str =
    "https://github.com/PolarisPyra/Patchwork/releases/latest/download/latest.json";
const RELEASES_URL: &str = "https://github.com/PolarisPyra/Patchwork/releases/latest";

/// Where checking stands (`UpdateState`), less the states only an installing copy reaches.
#[derive(Clone, Default)]
enum Check {
    #[default]
    Idle,
    Checking,
    UpToDate,
    /// A newer release, or an older one when the running version was withdrawn, and its notes.
    Available {
        version: String,
        notes: Option<String>,
    },
    Failed(String),
}

/// The releases opened, by version; None until the pane first draws, which opens the newest.
#[derive(Default)]
pub struct UpdatesState {
    open: Option<HashSet<String>>,
    check: Check,
}

/// Asks GitHub for the latest release and compares it with the running version. Blocks.
fn check_latest() -> Check {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .user_agent(format!("Patchwork/{}", patchwork_server::VERSION))
        .build()
        .into();
    let release = match agent.get(LATEST_RELEASE).call() {
        Ok(mut response) => match response.body_mut().read_json::<serde_json::Value>() {
            Ok(release) => release,
            Err(e) => {
                return Check::Failed(format!("The update server sent an unreadable release:{e}"));
            }
        },
        Err(e) => return Check::Failed(format!("Could not reach the update server: {e}")),
    };
    let version = release["tag_name"]
        .as_str()
        .unwrap_or_default()
        .trim_start_matches('v')
        .to_string();
    let current = patchwork_server::VERSION;
    let parts = |v: &str| -> Option<Vec<u64>> { v.split('.').map(|p| p.parse().ok()).collect() };
    let newer = matches!((parts(&version), parts(current)), (Some(a), Some(b)) if a > b);
    let withdrawn = release["withdrawn"].as_array().is_some_and(|tags| {
        tags.iter()
            .filter_map(|tag| tag.as_str())
            .any(|tag| tag.trim_start_matches('v') == current)
    });
    if !newer && !withdrawn {
        return Check::UpToDate;
    }
    let notes = release["body"]
        .as_str()
        .filter(|body| !body.trim().is_empty())
        .map(release_notes);
    Check::Available { version, notes }
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

impl SettingsDialog {
    fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        self.updates.check = Check::Checking;
        cx.notify();
        let task = cx.background_spawn(async move { check_latest() });
        cx.spawn(async move |this, cx| {
            let check = task.await;
            this.update(cx, |this, cx| {
                if let Check::Failed(e) = &check {
                    log::warn!("update check failed: {e}");
                }
                this.updates.check = check;
                cx.notify();
            })
        })
        .detach();
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
                    notes: notes.trim().to_string(),
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
    let check = d.updates.check.clone();
    let status = match &check {
        Check::Idle => "This copy can't install updates".to_string(),
        Check::Checking => "Checking for updates…".to_string(),
        Check::UpToDate => "Up to date".to_string(),
        Check::Available { version, .. } => format!("Patchwork {version} is available"),
        Check::Failed(e) => e.clone(),
    };
    let action = match &check {
        Check::Available { .. } => btn("update-action", "Download", BtnKind::Plain, false, &t)
            .child("Download")
            .on_click(|_, _, cx| cx.open_url(RELEASES_URL)),
        _ => {
            let checking = matches!(check, Check::Checking);
            let label = if checking {
                "Checking…"
            } else {
                "Check for updates"
            };
            btn("update-action", label, BtnKind::Plain, checking, &t)
                .child(label)
                .when(!checking, |b| {
                    b.on_click(cx.listener(|d, _, _, cx| d.check_for_updates(cx)))
                })
        }
    };
    let notes = match &check {
        Check::Available {
            notes: Some(notes), ..
        } => Some(notes.clone()),
        _ => None,
    };
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
                                .text_size(px(FS_SM))
                                .text_color(if matches!(check, Check::Failed(_)) {
                                    t.danger
                                } else {
                                    t.text_muted
                                })
                                .child(status),
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
