//! App icon color ways, shown on the window, the tray, and the installed launcher.

use std::sync::atomic::AtomicU64;

use tauri::image::Image;
use tauri::{AppHandle, Manager};

/// Id of the tray icon, so its image can change after the tray is built.
pub const TRAY_ID: &str = "main";

/// Color way used when the setting is unset or names one that no longer exists. Matches the
/// `default` in `assets/logos/manifest.json`.
const DEFAULT: &str = "flare";

macro_rules! logos {
    ($($id:literal),* $(,)?) => {
        /// Every color way in `assets/logos/manifest.json`, with its 512 pixel PNG.
        const LOGOS: &[(&str, &[u8])] = &[
            $(($id, include_bytes!(concat!("../../../../assets/logos/", $id, ".png")))),*
        ];
    };
}

logos![
    "original",
    "sunrise",
    "meadow",
    "lagoon",
    "peach",
    "lilac",
    "electric",
    "tropic",
    "popsicle",
    "arcade",
    "zest",
    "neon-mint",
    "flare",
    "lime-pop",
    "sherbet",
    "mono-pink",
    "mono-blue",
    "mono-violet",
    "mono-green",
    "mono-orange",
    "gunmetal",
    "midnight",
    "gold",
    "snow",
];

/// Counts launcher icon changes, so only the latest one remaps the window.
#[derive(Debug, Default)]
pub struct LogoChanges(AtomicU64);

/// Find the color way named `id`, falling back to the default.
///
/// Returns the color way's id and PNG.
fn resolve(id: Option<&str>) -> (&'static str, &'static [u8]) {
    let find = |id: &str| LOGOS.iter().find(|(logo, _)| *logo == id).copied();
    id.and_then(find)
        .or_else(|| find(DEFAULT))
        .expect("the default color way is in LOGOS")
}

/// Show the color way named `id` on the window, the tray, and the installed launcher.
///
/// `id` of None, or one that no longer exists, shows the default color way.
pub fn apply(app: &AppHandle, id: Option<&str>) {
    let (id, png) = resolve(id);
    match Image::from_bytes(png) {
        Ok(icon) => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_icon(icon.clone());
            }
            if let Some(tray) = app.tray_by_id(TRAY_ID) {
                let _ = tray.set_icon(Some(icon));
            }
        }
        Err(e) => log::warn!("app icon {id} could not be decoded: {e}"),
    }
    #[cfg(target_os = "linux")]
    launcher::apply(app, id, png);
}

/// HACK: the desktop shows a Wayland window's icon from its launcher, because tao cannot set a
/// window icon there yet (the `xdg-toplevel-icon` protocol). Until it can, the color way is
/// installed as the launcher's icon, and the window is remapped so the window manager and taskbar
/// read it again. Once the window icon can be set directly, remove this module and the icon
/// handling in `scripts/build.sh`.
#[cfg(target_os = "linux")]
mod launcher {
    use std::fs;
    use std::io;
    use std::path::Path;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use tauri::{AppHandle, Manager};

    use super::LogoChanges;

    /// Plasma's taskbar loaded a rebuilt application cache in about half a second, and keeps the
    /// old icon if the window is remapped before then.
    const TASKBAR_RELOAD: Duration = Duration::from_millis(1500);

    /// Install `png` as the launcher icon, then remap the window once the desktop has loaded it.
    pub fn apply(app: &AppHandle, id: &str, png: &[u8]) {
        let installed = app
            .path()
            .data_dir()
            .map_err(io::Error::other)
            .and_then(|data_home| install(&data_home, &app.config().identifier, id, png));
        match installed {
            Ok(true) => {
                let changes = app.state::<LogoChanges>();
                let change = changes.0.fetch_add(1, Ordering::Relaxed) + 1;
                let app = app.clone();
                std::thread::spawn(move || {
                    refresh_cache();
                    std::thread::sleep(TASKBAR_RELOAD);
                    // A later pick remaps the window itself, so this one would only interrupt it.
                    if app.state::<LogoChanges>().0.load(Ordering::Relaxed) == change {
                        remap(&app);
                    }
                });
            }
            Ok(false) => {}
            Err(e) => log::warn!("launcher icon could not change: {e}"),
        }
    }

    /// Point the installed launcher at `png`.
    ///
    /// Every change installs the icon under a name never used before, because the window manager
    /// and taskbar keep icons, and failed lookups, cached by name. Runs without an installed
    /// launcher, such as the AppImage, leave the desktop files alone.
    ///
    /// Returns whether the launcher now names a different icon.
    fn install(data_home: &Path, app_id: &str, id: &str, png: &[u8]) -> io::Result<bool> {
        let launcher = data_home.join(format!("applications/{app_id}.desktop"));
        let entry = match fs::read_to_string(&launcher) {
            Ok(entry) => entry,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        };
        let theme = data_home.join("icons/hicolor");
        let apps = theme.join("512x512/apps");
        let family = format!("{app_id}-logo-");
        let prefix = format!("{family}{id}-");
        let current = entry.lines().find_map(|line| line.strip_prefix("Icon="));
        if let Some(current) = current.filter(|current| current.starts_with(&prefix))
            && fs::read(apps.join(format!("{current}.png")))
                .ok()
                .as_deref()
                == Some(png)
        {
            return Ok(false);
        }

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis());
        let name = format!("{prefix}{stamp}");
        let icon = apps.join(format!("{name}.png"));
        fs::create_dir_all(&apps)?;
        let staged = icon.with_extension("png.tmp");
        fs::write(&staged, png)?;
        fs::rename(&staged, &icon)?;
        // Icon caches compare the theme directory's modification time to notice new icons.
        fs::File::open(&theme)?.set_modified(SystemTime::now())?;

        let icon_line = format!("Icon={name}");
        let updated: String = entry
            .lines()
            .map(|line| {
                if line.starts_with("Icon=") {
                    icon_line.as_str()
                } else {
                    line
                }
            })
            .flat_map(|line| [line, "\n"])
            .collect();
        let staged = launcher.with_extension("desktop.tmp");
        fs::write(&staged, updated)?;
        fs::rename(&staged, &launcher)?;

        // Earlier picks are unused once the launcher names this one.
        let installed = format!("{name}.png");
        for file in fs::read_dir(&apps)? {
            let file_name = file?.file_name();
            let file_name = file_name.to_string_lossy();
            if file_name.starts_with(&family) && file_name != installed {
                match fs::remove_file(apps.join(&*file_name)) {
                    Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                    _ => {}
                }
            }
        }
        Ok(true)
    }

    /// Rebuild KDE's application cache, which the taskbar reads the launcher icon from. The
    /// desktop otherwise rebuilds it on its own schedule. Desktops without the tool skip it.
    fn refresh_cache() {
        let _ = std::process::Command::new("kbuildsycoca6").output();
    }

    /// Unmap and map the window, which the window manager and taskbar treat as a new window and
    /// read the launcher icon for again. A window hidden to the tray is left hidden.
    fn remap(app: &AppHandle) {
        if let Some(window) = app.get_webview_window("main")
            && window.is_visible().unwrap_or(false)
        {
            let _ = window.hide();
            let _ = window.show();
        }
    }
}
