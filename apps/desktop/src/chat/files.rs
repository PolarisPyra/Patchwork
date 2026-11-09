//! What a reply's files look like now (`hooks/useReplyFiles.ts`), and the two lists under a reply
//! that show them: its produced files (`FileCard.tsx`) and its changed files
//! (`transcript/components/ChangeSummary.tsx`).

use std::collections::HashSet;
use std::path::PathBuf;

use gpui::{Context, Div, ObjectFit, Role, SharedString, StyledImage, div, img, prelude::*, px};
use patchwork_server::orchestration::changes::checkpoint::{TurnFile, TurnFileState, turn_files};
use patchwork_server::threads::Message;
use patchwork_server::workspace::{FileChange, FileMedia, PreviewKind, file_media, missing_files};

use crate::canvas::CanvasTab;
use crate::ui::theme::{FS_SM, FS_XS, RADIUS, RADIUS_SM, Theme};
use crate::ui::{BtnKind, base_name, btn, button, dir_name, icon, tooltip};
use crate::workspace::{View, Workspace};

/// Most files the summary lists; the rest are in the Changes view.
const SUMMARY_FILES: usize = 5;
/// Most files the cards show, as the summary lists most of its files.
const SHOWN_CARDS: usize = 3;
const BYTE_UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

/// The state of a reply's files, loaded once for the whole reply. While the files change, as they
/// do all through a turn, the last state shows until the next one loads.
#[derive(Default)]
pub struct ReplyFiles {
    /// What the reply's turn did to each file; `None` until loaded, and for replies without
    /// snapshots.
    pub turn_files: Option<Vec<TurnFile>>,
    /// The files that no longer exist in the project.
    pub missing: HashSet<String>,
    /// How each file is shown; `None` until loaded.
    pub media: Option<Vec<FileMedia>>,
    /// The paths and the git state it was loaded for, so a change to either loads it again.
    loaded_for: (Vec<String>, u64),
    /// Whether a load is under way.
    pub loading: bool,
}

impl Workspace {
    /// Loads the files of every reply in the open chat whose state is missing or out of date.
    pub fn load_reply_files(&mut self, cx: &mut Context<Self>) {
        let (Some(thread), Some(project)) = (&self.thread, self.project()) else {
            return;
        };
        let root = project.path.clone();
        let generation = self.changes_generation;
        let stale: Vec<Message> = thread
            .messages
            .iter()
            .filter(|m| !m.files.is_empty())
            .filter(|m| {
                self.reply_files
                    .get(&m.id)
                    .is_none_or(|f| f.loaded_for != (m.files.clone(), generation))
            })
            .cloned()
            .collect();
        for m in stale {
            let entry = self.reply_files.entry(m.id.clone()).or_default();
            entry.loaded_for = (m.files.clone(), generation);
            entry.loading = true;
            let root = root.clone();
            let load = cx.background_spawn(async move {
                let turn = m.turn.as_ref().and_then(|turn| {
                    turn_files(&root, turn, &m.files)
                        .inspect_err(|e| log::warn!("could not read the turn's files: {e}"))
                        .ok()
                });
                let missing = missing_files(&root, &m.files).into_iter().collect();
                let media = file_media(&root, &m.files)
                    .inspect_err(|e| log::warn!("could not read the reply's files: {e}"))
                    .ok();
                (m.id, turn, missing, media)
            });
            cx.spawn(async move |this, cx| {
                let (id, turn, missing, media) = load.await;
                let _ = this.update(cx, |this, cx| {
                    if let Some(files) = this.reply_files.get_mut(&id) {
                        files.turn_files = turn;
                        files.missing = missing;
                        files.media = media;
                        files.loading = false;
                        cx.notify();
                    }
                });
            })
            .detach();
        }
    }

    /// Opens a reply's file: a reply with snapshots keeps its turn's diff, even once committed, and a
    /// file with nothing left to diff opens on its contents.
    pub fn open_reply_file(
        &mut self,
        path: &str,
        message: &Message,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let tab = if message.turn.is_some() || self.changes.iter().any(|c| c.path == path) {
            CanvasTab::Diff
        } else {
            CanvasTab::Code
        };
        self.open_canvas(path, Some(message.clone()), tab, window, cx);
    }

    /// "Show changes": the reply's files lead the Changes view.
    pub fn show_changes(&mut self, message_id: &str, cx: &mut Context<Self>) {
        if let Some(thread) = &self.thread {
            self.change_focus = Some((thread.id.clone(), message_id.to_string()));
        }
        self.view = Some(View::Changes);
        cx.notify();
    }
}

/// The produced files and the changed files under a reply, or nothing for a reply without files.
pub fn reply_files(
    w: &Workspace,
    m: &Message,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Option<Div> {
    if m.files.is_empty() || w.project().is_none() {
        return None;
    }
    let empty = ReplyFiles::default();
    let files = w.reply_files.get(&m.id).unwrap_or(&empty);
    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .children(file_cards(w, m, files, t, cx))
            .child(change_summary(w, m, files, t, cx)),
    )
}

/// The reply's produced files, one row each with its type and size, and a small thumbnail for an
/// image. Text files get no row; the summary below still lists every file.
fn file_cards(
    w: &Workspace,
    m: &Message,
    files: &ReplyFiles,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Option<gpui::Stateful<Div>> {
    let cards: Vec<&FileMedia> = files
        .media
        .iter()
        .flatten()
        .filter(|f| !matches!(f.kind, PreviewKind::Text))
        .collect();
    if cards.is_empty() {
        return None;
    }
    // One file over the limit is shown rather than hidden behind "1 more".
    let shown = if cards.len() > SHOWN_CARDS + 1 {
        SHOWN_CARDS
    } else {
        cards.len()
    };
    let more = cards.len() - shown;
    let rows = cards
        .into_iter()
        .take(shown)
        .map(|file| {
            let deleted = files
                .turn_files
                .iter()
                .flatten()
                .any(|f| f.path == file.path && matches!(f.state, TurnFileState::Deleted))
                || w.changes
                    .iter()
                    .any(|c| c.path == file.path && c.status == "deleted")
                || files.missing.contains(&file.path);
            produced_file(w, m, file, deleted, t, cx)
        })
        .collect::<Vec<_>>();
    Some(
        div()
            .id(SharedString::from(format!("cards-{}", m.id)))
            .role(gpui::Role::List)
            .aria_label("Files from this reply")
            .flex()
            .flex_col()
            .rounded(px(RADIUS))
            .border_1()
            .border_color(t.border)
            .bg(t.bg_panel)
            .px(px(12.))
            .py(px(8.))
            .text_size(px(FS_SM))
            .children(rows)
            .when(more > 0, |d| {
                d.child(more_button(format!("cards-more-{}", m.id), more, m, t, cx))
            }),
    )
}

/// One produced file, opened in the canvas on click. An image shows a small thumbnail; a file the
/// turn deleted, or that is gone, shows `Deleted` and never a broken image.
fn produced_file(
    w: &Workspace,
    m: &Message,
    file: &FileMedia,
    deleted: bool,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> impl IntoElement + use<> {
    let name = base_name(&file.path).to_string();
    let kind = name
        .rfind('.')
        .map(|dot| name[dot + 1..].to_lowercase())
        .unwrap_or_default();
    let thumb = if matches!(file.kind, PreviewKind::Image) && !deleted {
        // A neutral backing keeps a transparent image visible on both themes.
        img(PathBuf::from(&file.absolute))
            .size(px(24.))
            .rounded(px(RADIUS_SM))
            .bg(t.bg_hover)
            .object_fit(ObjectFit::Cover)
            .into_any_element()
    } else {
        div()
            .size(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .child(icon("file", 15.).text_color(t.text_muted))
            .into_any_element()
    };
    let hover = t.bg_hover;
    let path = file.path.clone();
    let message = m.clone();
    let mut open = button(
        SharedString::from(format!("card-{}-{}", m.id, file.path)),
        name.clone(),
    )
    .flex_1()
    .min_w_0()
    .min_h(px(28.))
    .gap(px(8.))
    .px(px(4.))
    .rounded(px(RADIUS_SM))
    .tooltip(tooltip(file.path.clone()))
    .child(thumb)
    .child(
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_color(if deleted { t.text_muted } else { t.text })
            .when(deleted, |d| d.line_through())
            .child(name.clone()),
    )
    .when(!kind.is_empty(), |d| {
        d.child(
            div()
                .flex_none()
                .max_w(px(80.))
                .truncate()
                .rounded(px(RADIUS_SM))
                .bg(t.bg_hover)
                .px(px(4.))
                .text_size(px(FS_XS))
                .text_color(t.text_muted)
                .child(kind),
        )
    })
    .child(
        div()
            .flex_none()
            .min_w(px(52.))
            .flex()
            .justify_end()
            .whitespace_nowrap()
            .text_size(px(FS_XS))
            .text_color(t.text_subtle)
            .child(if deleted {
                "Deleted".to_string()
            } else {
                format_bytes(file.size)
            }),
    );
    if deleted {
        open = open.cursor_default();
    } else {
        open = open.hover(move |s| s.bg(hover)).on_click(
            cx.listener(move |w, _, window, cx| w.open_reply_file(&path, &message, window, cx)),
        );
    }
    let (fg, root, rel) = (
        t.text,
        w.project().map(|p| p.path.clone()),
        file.path.clone(),
    );
    div()
        .id(SharedString::from(format!(
            "card-row-{}-{}",
            m.id, file.path
        )))
        .role(Role::ListItem)
        .flex()
        .items_center()
        .gap(px(8.))
        .child(open)
        // The last column keeps its width on a deleted file, which has no button.
        .child(
            div()
                .flex_none()
                .size(px(24.))
                .children(root.filter(|_| !deleted).map(|root| {
                    button(
                        SharedString::from(format!("card-app-{}-{}", m.id, file.path)),
                        format!("Open {name} in its app"),
                    )
                    .size(px(24.))
                    .justify_center()
                    .rounded(px(RADIUS_SM))
                    .text_color(t.text_muted)
                    .hover(move |s| s.bg(hover).text_color(fg))
                    .tooltip(tooltip("Open in its app"))
                    .child(icon("arrow-square-out", 14.))
                    .on_click(move |_, _, cx| {
                        match patchwork_server::workspace::resolve_inside(&root, &rel) {
                            Ok(file) => cx.open_with_system(&file),
                            Err(e) => log::warn!("could not open {rel}: {e}"),
                        }
                    })
                })),
        )
}

/// The files a reply touched, each opening its diff. A reply with snapshots of its turn keeps what
/// the turn did, line counts included, once its changes are committed or edited again, and notes
/// where each file stands now. Older replies show the files' uncommitted changes.
fn change_summary(
    w: &Workspace,
    m: &Message,
    files: &ReplyFiles,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Div {
    let paths = &m.files;
    let (additions, deletions) = match &files.turn_files {
        Some(found) => found
            .iter()
            .fold((0, 0), |(a, d), f| (a + f.additions, d + f.deletions)),
        None => w
            .changes
            .iter()
            .filter(|c| paths.contains(&c.path))
            .fold((0, 0), |(a, d), c| (a + c.additions, d + c.deletions)),
    };
    let listed = if paths.len() > SUMMARY_FILES + 1 {
        SUMMARY_FILES
    } else {
        paths.len()
    };
    let rows = paths
        .iter()
        .take(listed)
        .map(|path| {
            if m.turn.is_some() {
                let file = files.turn_files.iter().flatten().find(|f| &f.path == path);
                let note = file.and_then(|f| turn_note(&f.state));
                let lines = file.map(|f| (f.additions, f.deletions));
                let deleted = file.is_some_and(|f| matches!(f.state, TurnFileState::Deleted));
                changed_file(m, path, note, lines, deleted, t, cx)
            } else {
                let change: Option<&FileChange> = w.changes.iter().find(|c| &c.path == path);
                let deleted =
                    files.missing.contains(path) || change.is_some_and(|c| c.status == "deleted");
                let note = if deleted {
                    Some("Deleted")
                } else if change.is_some() {
                    None
                } else {
                    Some("No changes")
                };
                changed_file(m, path, note, None, deleted, t, cx)
            }
        })
        .collect::<Vec<_>>();
    let id = m.id.clone();
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .rounded(px(RADIUS))
        .border_1()
        .border_color(t.border)
        .bg(t.bg_panel)
        .px(px(12.))
        .py(px(8.))
        .text_size(px(FS_SM))
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(12.))
                .child(icon("git-diff", 14.).flex_none().text_color(t.text_muted))
                .child(div().whitespace_nowrap().child(format!(
                    "{} {} changed",
                    paths.len(),
                    if paths.len() == 1 { "file" } else { "files" }
                )))
                .when(additions + deletions > 0, |d| {
                    d.child(div().text_color(t.success).child(format!("+{additions}")))
                        .child(div().text_color(t.danger).child(format!("−{deletions}")))
                })
                .child(div().flex_1())
                .child(
                    btn(
                        SharedString::from(format!("show-changes-{}", m.id)),
                        "Show changes",
                        BtnKind::Plain,
                        false,
                        t,
                    )
                    .child("Show changes")
                    .on_click(cx.listener(move |w, _, _, cx| w.show_changes(&id, cx))),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .children(rows)
                .when(listed < paths.len(), |d| {
                    d.child(more_button(
                        format!("summary-more-{}", m.id),
                        paths.len() - listed,
                        m,
                        t,
                        cx,
                    ))
                }),
        )
}

/// How a file the turn changed stands now, when that is worth a note.
fn turn_note(state: &TurnFileState) -> Option<&'static str> {
    match state {
        TurnFileState::Uncommitted => None,
        TurnFileState::Committed => Some("Committed"),
        TurnFileState::Reverted => Some("Reverted"),
        TurnFileState::Deleted => Some("Deleted"),
        TurnFileState::Unchanged => Some("No changes"),
    }
}

/// One touched file: its name, struck through once deleted, then its folder, and a note on where
/// it stands, such as "Committed".
fn changed_file(
    m: &Message,
    path: &str,
    note: Option<&'static str>,
    lines: Option<(u32, u32)>,
    deleted: bool,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> impl IntoElement + use<> {
    let counted = lines.filter(|(a, d)| a + d > 0);
    let hover = t.bg_hover;
    let (rel, message) = (path.to_string(), m.clone());
    button(
        SharedString::from(format!("changed-{}-{path}", m.id)),
        path.to_string(),
    )
    .min_h(px(24.))
    .min_w_0()
    .gap(px(8.))
    .px(px(4.))
    .rounded(px(RADIUS_SM))
    .hover(move |s| s.bg(hover))
    .tooltip(tooltip(path.to_string()))
    .child(
        div()
            .flex_none()
            .max_w(gpui::relative(0.6))
            .truncate()
            .text_color(if deleted { t.text_muted } else { t.text })
            .when(deleted, |d| d.line_through())
            .child(base_name(path).to_string()),
    )
    .child(
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_color(t.text_subtle)
            .child(dir_name(path).to_string()),
    )
    .children(note.map(|note| {
        div()
            .flex_none()
            .text_size(px(FS_XS))
            .text_color(t.text_subtle)
            .child(note)
    }))
    .children(counted.map(|(a, d)| {
        div()
            .flex_none()
            .flex()
            .gap(px(8.))
            .text_size(px(FS_XS))
            .child(
                div()
                    .min_w(px(28.))
                    .flex()
                    .justify_end()
                    .text_color(t.success)
                    .child(format!("+{a}")),
            )
            .child(
                div()
                    .min_w(px(28.))
                    .flex()
                    .justify_end()
                    .text_color(t.danger)
                    .child(format!("−{d}")),
            )
    }))
    .on_click(cx.listener(move |w, _, window, cx| w.open_reply_file(&rel, &message, window, cx)))
}

/// "N more", which opens the Changes view, where the rest are.
fn more_button(
    id: String,
    more: usize,
    m: &Message,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> impl IntoElement + use<> {
    let (hover, fg) = (t.bg_hover, t.text);
    let label = format!("{more} more");
    let message_id = m.id.clone();
    button(SharedString::from(id), label.clone())
        .w_full()
        .min_h(px(24.))
        .px(px(4.))
        .rounded(px(RADIUS_SM))
        .text_color(t.text_muted)
        .hover(move |s| s.bg(hover).text_color(fg))
        .child(label)
        .on_click(cx.listener(move |w, _, _, cx| w.show_changes(&message_id, cx)))
}

/// `formatBytes`: a size in decimal units, one decimal under 100.
fn format_bytes(bytes: u64) -> String {
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000. && unit < BYTE_UNITS.len() - 1 {
        value /= 1000.;
        unit += 1;
    }
    if unit == 0 {
        return format!("{bytes} B");
    }
    if value < 100. {
        format!("{value:.1} {}", BYTE_UNITS[unit])
    } else {
        format!("{} {}", value.round(), BYTE_UNITS[unit])
    }
}
