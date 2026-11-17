//! The icon the desktop shows for Patchwork in its launcher, title bar and taskbar.
//!
//! On Linux these come from the launcher entry that was made when the AppImage was added to the
//! menu, which points at an icon copied out of the AppImage at the time. An update replaces the
//! AppImage but not that copy, so each start writes the picked icon over it and tells the desktop
//! its icons changed. Windows and macOS take the icon from the app's own files.

// Rust guideline compliant 2026-10-09

/// Sets the desktop's icon for Patchwork to `svg`, an app icon of `assets/logos/`.
///
/// Writes nothing that already matches, and nothing at all from a dev build, which shares the
/// installed app's launcher entry. Blocks on file IO, so it runs off the UI thread.
pub fn set_launcher_icon(svg: &[u8]) {
    #[cfg(target_os = "linux")]
    if !cfg!(debug_assertions) {
        linux::set(svg);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = svg;
}

#[cfg(target_os = "linux")]
mod linux {
    use std::path::{Path, PathBuf};

    /// The window's app id.
    const APP_ID: &str = "patchwork";

    pub(super) fn set(svg: &[u8]) {
        let Some(data_home) = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| crate::fs::home_dir().map(|home| home.join(".local/share")))
        else {
            return;
        };
        let tree = match resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default()) {
            Ok(tree) => tree,
            Err(e) => {
                log::warn!(error:% = e; "the app icon cannot be read: {{error}}");
                return;
            }
        };
        let hicolor = data_home.join("icons/hicolor");
        let mut names = Vec::new();
        let mut themed_changed = false;
        for entry in entries(&data_home.join("applications")) {
            let Some(icon) = entry.icon else { continue };
            let changed = if Path::new(&icon).is_absolute() {
                write_png(&tree, Path::new(&icon), 512)
            } else {
                names.push(icon);
                false
            };
            if changed {
                touch(&entry.path);
            }
        }
        names.sort();
        names.dedup();
        for name in &names {
            themed_changed |= write_png(
                &tree,
                &hicolor.join(format!("512x512/apps/{name}.png")),
                512,
            );
            // Sizes the menu entry was given when it was made, so no older one is picked first.
            for (dir, side) in size_dirs(&hicolor) {
                let file = dir.join(format!("apps/{name}.png"));
                if side != 512 && file.exists() {
                    themed_changed |= write_png(&tree, &file, side);
                }
            }
        }
        if themed_changed {
            refresh_theme(&hicolor);
        }
    }

    /// A launcher entry of Patchwork's, and the icon it names.
    struct Entry {
        path: PathBuf,
        icon: Option<String>,
    }

    /// The launcher entries that start this AppImage or own its windows.
    fn entries(applications: &Path) -> Vec<Entry> {
        let appimage = std::env::var("APPIMAGE").ok();
        let Ok(dir) = std::fs::read_dir(applications) else {
            return Vec::new();
        };
        dir.flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "desktop"))
            .filter_map(|path| {
                let text = std::fs::read_to_string(&path).ok()?;
                let value = |key: &str| {
                    text.lines()
                        .find_map(|l| l.strip_prefix(key)?.trim_start().strip_prefix('='))
                        .map(|v| v.trim().to_owned())
                };
                let starts_us = ["Exec", "TryExec"].iter().any(|key| {
                    let program = value(key).and_then(|v| {
                        v.split_whitespace()
                            .next()
                            .map(|p| p.trim_matches('"').to_owned())
                    });
                    program.is_some() && program == appimage
                });
                let owns_windows = value("StartupWMClass").as_deref() == Some(APP_ID);
                (starts_us || owns_windows).then(|| Entry {
                    path,
                    icon: value("Icon"),
                })
            })
            .collect()
    }

    /// The `NxN` folders of the icon theme, with their side.
    fn size_dirs(hicolor: &Path) -> Vec<(PathBuf, u32)> {
        let Ok(dir) = std::fs::read_dir(hicolor) else {
            return Vec::new();
        };
        dir.flatten()
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let (w, h) = name.split_once('x')?;
                let side = w
                    .parse::<u32>()
                    .ok()
                    .filter(|&s| h.parse() == Ok(s) && s <= 1024)?;
                Some((e.path(), side))
            })
            .collect()
    }

    /// Writes `tree` as a `side`-pixel PNG to `file`, unless it already holds that. Whether it
    /// wrote.
    fn write_png(tree: &resvg::usvg::Tree, file: &Path, side: u32) -> bool {
        let Some(mut pixmap) = resvg::tiny_skia::Pixmap::new(side, side) else {
            return false;
        };
        let scale = side as f32 / tree.size().width().max(tree.size().height());
        resvg::render(
            tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        let png = match pixmap.encode_png() {
            Ok(png) => png,
            Err(e) => {
                log::warn!(error:% = e; "the app icon cannot be drawn: {{error}}");
                return false;
            }
        };
        if std::fs::read(file).is_ok_and(|old| old == png) {
            return false;
        }
        let written = file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(file, &png));
        match written {
            Ok(()) => true,
            Err(e) => {
                log::warn!(path:% = file.display(), error:% = e; "could not write the app icon to {{path}}: {{error}}");
                false
            }
        }
    }

    /// Tells the desktop the icon theme changed: its caches are checked against the theme
    /// folder's time, and GTK's own cache file is rebuilt when there is one.
    fn refresh_theme(hicolor: &Path) {
        touch(hicolor);
        if !hicolor.join("icon-theme.cache").exists() {
            return;
        }
        let Some(program) = crate::find_program("gtk-update-icon-cache") else {
            return;
        };
        let ran = crate::host_command(program)
            .args(["-f", "-t", "-q"])
            .arg(hicolor)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if let Err(e) = ran {
            log::warn!(error:% = e; "could not refresh the icon cache: {{error}}");
        }
    }

    /// Sets `path`'s modified time to now, so what caches it reads it again.
    fn touch(path: &Path) {
        let now = std::time::SystemTime::now();
        if let Err(e) = std::fs::File::open(path).and_then(|f| f.set_modified(now)) {
            log::warn!(path:% = path.display(), error:% = e; "could not mark {{path}} changed: {{error}}");
        }
    }
}
