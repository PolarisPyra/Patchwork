//! Watching the source: which saves count, and sending them to the main loop.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use anyhow::Context;
use globset::{Glob, GlobSet, GlobSetBuilder};
use notify::event::ModifyKind;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::Event;
use crate::config::Watch;

/// What a save asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A rebuild, then a restart.
    Build,
    /// A restart with the binary the app has.
    Restart,
    /// Sending the file to the running app.
    Send,
}

/// Decides what a save to a path asks for.
#[derive(Debug)]
struct Filter {
    root: PathBuf,
    extensions: Vec<String>,
    ignore: GlobSet,
    restart: GlobSet,
    send: GlobSet,
}

fn globs(globs: &[String]) -> anyhow::Result<GlobSet> {
    let mut set = GlobSetBuilder::new();
    for glob in globs {
        set.add(Glob::new(glob).with_context(|| format!("`{glob}` is not a glob"))?);
    }
    set.build().context("could not build the globs")
}

impl Filter {
    fn new(root: &Path, watch: &Watch) -> anyhow::Result<Self> {
        Ok(Self {
            root: root.to_path_buf(),
            extensions: watch.extensions.clone(),
            ignore: globs(&watch.ignore)?,
            restart: globs(&watch.restart)?,
            send: globs(&watch.send)?,
        })
    }

    /// What a save to `path` asks for, if anything. Editors' hidden and backup files ask for
    /// nothing.
    fn kind(&self, path: &Path) -> Option<Kind> {
        let name = path.file_name().and_then(|n| n.to_str())?;
        if name.starts_with('.') || name.starts_with('#') || name.ends_with('~') {
            return None;
        }
        let from_root = path.strip_prefix(&self.root).unwrap_or(path);
        if self.ignore.is_match(from_root) {
            return None;
        }
        if self.send.is_match(from_root) {
            return Some(Kind::Send);
        }
        if self.restart.is_match(from_root) {
            return Some(Kind::Restart);
        }
        let known = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| self.extensions.iter().any(|x| x == e));
        known.then_some(Kind::Build)
    }
}

/// Watches `watch.paths` under `root` and sends each save that asks for something.
///
/// The watcher stops when the returned value is dropped.
///
/// # Errors
///
/// When a glob is invalid or a path can't be watched.
pub fn start(
    root: &Path,
    watch: &Watch,
    send: Sender<Event>,
) -> anyhow::Result<RecommendedWatcher> {
    let filter = Filter::new(root, watch)?;
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else { return };
        // Reads and changes of only the time or the permissions say nothing about the code.
        let wanted = matches!(
            event.kind,
            EventKind::Create(_)
                | EventKind::Remove(_)
                | EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Name(_) | ModifyKind::Any)
        );
        if !wanted {
            return;
        }
        for path in event.paths {
            if let Some(kind) = filter.kind(&path) {
                // The loop has ended when nobody receives.
                let _ = send.send(Event::Changed(path, kind));
            }
        }
    })
    .context("could not start watching files")?;
    for path in &watch.paths {
        let path = root.join(path);
        watcher
            .watch(&path, RecursiveMode::Recursive)
            .with_context(|| format!("could not watch {}", path.display()))?;
    }
    Ok(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter() -> Filter {
        let watch = Watch {
            ignore: vec!["target/**".into(), "crates/skip/**".into()],
            restart: vec!["assets/**".into()],
            send: vec!["assets/themes/*.json".into()],
            ..Watch::default()
        };
        Filter::new(Path::new("/repo"), &watch).expect("the globs are valid")
    }

    #[test]
    fn builds_for_sources_and_manifests() {
        let f = filter();
        assert_eq!(
            f.kind(Path::new("/repo/crates/ui/src/lib.rs")),
            Some(Kind::Build)
        );
        assert_eq!(f.kind(Path::new("/repo/Cargo.toml")), Some(Kind::Build));
    }

    #[test]
    fn sends_before_it_restarts() {
        let f = filter();
        let theme = Path::new("/repo/assets/themes/lagoon.json");
        assert_eq!(f.kind(theme), Some(Kind::Send));
        let icon = Path::new("/repo/assets/icons/x.svg");
        assert_eq!(f.kind(icon), Some(Kind::Restart));
    }

    #[test]
    fn skips_editor_files_ignored_paths_and_other_kinds() {
        let f = filter();
        for path in [
            "/repo/crates/ui/src/.lib.rs.swp",
            "/repo/crates/ui/src/lib.rs~",
            "/repo/crates/ui/src/4913",
            "/repo/target/debug/build/x/out/gen.rs",
            "/repo/crates/skip/src/main.rs",
            "/repo/README.md",
            "/repo/assets/icons/.x.svg.swp",
        ] {
            assert_eq!(f.kind(Path::new(path)), None, "{path}");
        }
    }
}
