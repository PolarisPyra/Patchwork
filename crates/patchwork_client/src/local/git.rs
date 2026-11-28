//! Where the open project's git stands: its branch, its root, the files changed in it, and the
//! files each reply in the open chat changed.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};

use gpui::{App, AppContext as _, Context, Entity, Global};
use patchwork_core::orchestration::changes::turn_files::TurnFile;
use patchwork_core::projects::FileMedia;
use patchwork_core::source_control::FileChange;

/// The state of a reply's files, loaded once for the whole reply. While the files change, as they
/// do all through a turn, the last state shows until the next one loads.
#[derive(Debug, Default)]
pub struct ReplyFiles {
    /// What the reply's turn did to each file; `None` until loaded, and for replies without
    /// snapshots.
    pub turn_files: Option<Vec<TurnFile>>,
    /// The files that no longer exist in the project.
    pub missing: HashSet<String>,
    /// How each file is shown; `None` until loaded.
    pub media: Option<Vec<FileMedia>>,
    /// The paths and the git state it was loaded for, so a change to either loads it again.
    pub loaded_for: (Vec<String>, u64),
    /// Whether a load is under way.
    pub loading: bool,
}

/// The open project's git: what Source Control, the bars and the replies' file lists show.
#[derive(Debug, Default)]
pub struct GitModel {
    /// The branch checked out in the open project's folder, or in the open chat's worktree.
    pub branch: Option<String>,
    /// The root of the git repository the project is in, when it is in one.
    pub git_root: Option<String>,
    /// Counts each read of git, so a read that finishes after a later one started is dropped.
    pub git_seq: u64,
    /// The files changed in the project.
    pub changes: Vec<FileChange>,
    /// The reply whose "Show changes" was clicked, as its chat and message ids: its files lead
    /// the Changes view.
    pub change_focus: Option<(String, String)>,
    /// Counts each change to the project's files, so the replies' files load again after one.
    pub changes_generation: u64,
    /// The files of each reply in the open chat, by message id.
    pub reply_files: HashMap<String, ReplyFiles>,
}

/// The app's one [`GitModel`].
struct GitGlobal(Entity<GitModel>);

impl Global for GitGlobal {}

impl GitModel {
    /// Makes the app's git model; call once at startup, before any window opens.
    pub(crate) fn init(cx: &mut App) {
        let model = cx.new(|_| Self::default());
        cx.set_global(GitGlobal(model));
    }

    /// The app's git model, to observe.
    #[must_use]
    pub fn entity(cx: &App) -> Entity<Self> {
        cx.global::<GitGlobal>().0.clone()
    }

    /// The app's git model, to read.
    #[must_use]
    pub fn read(cx: &App) -> &Self {
        cx.global::<GitGlobal>().0.read(cx)
    }

    /// Changes the app's git model with `f` and tells its observers.
    pub fn update<R>(cx: &mut App, f: impl FnOnce(&mut Self, &mut Context<Self>) -> R) -> R {
        Self::entity(cx).update(cx, |model, cx| {
            let r = f(model, cx);
            cx.notify();
            r
        })
    }
}
