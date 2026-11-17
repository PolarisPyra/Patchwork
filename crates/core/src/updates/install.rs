//! Puts a checked download in place of the running copy, as each system allows: the AppImage
//! file on Linux, the exe on Windows, the app bundle on macOS. The running copy keeps going on
//! the files it opened, and the new one runs from the next start.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};

use crate::Error;

/// Whether this copy can replace itself: a release build that runs from an AppImage, an app
/// bundle, or (on Windows) the exe.
pub(super) fn can_install() -> bool {
    if cfg!(debug_assertions) {
        return false;
    }
    if cfg!(windows) {
        true
    } else if cfg!(target_os = "macos") {
        app_bundle().is_some()
    } else {
        std::env::var_os("APPIMAGE").is_some()
    }
}

/// What to start after an update, where it is not the running executable: the AppImage, whose
/// running copy is a file inside its mount.
#[must_use]
pub fn restart_path() -> Option<PathBuf> {
    if cfg!(any(windows, target_os = "macos")) {
        return None;
    }
    std::env::var_os("APPIMAGE").map(PathBuf::from)
}

/// The `.app` bundle this copy runs from, on macOS.
fn app_bundle() -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    // Patchwork.app/Contents/MacOS/patchwork
    let bundle = exe.parent()?.parent()?.parent()?;
    (bundle.extension()? == "app").then(|| bundle.to_path_buf())
}

/// Moves the checked AppImage over the running one, which keeps its open file until the restart.
#[cfg(all(unix, not(target_os = "macos")))]
pub(super) fn install(file: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;
    let target = PathBuf::from(std::env::var_os("APPIMAGE").ok_or("Not running as an AppImage.")?);
    // Copied beside the AppImage first, so the move into place is a rename on one filesystem.
    let staged = target.with_extension("download");
    let result = std::fs::copy(file, &staged)
        .and_then(|_| std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)))
        .and_then(|()| std::fs::rename(&staged, &target))
        .map_err(|e| format!("Could not replace {}: {e}", target.display()));
    if result.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    let _ = std::fs::remove_file(file);
    result.map_err(Error::from)
}

/// Where the exe an update replaced waits to be removed: Windows lets a running exe be renamed,
/// not deleted.
#[cfg(windows)]
fn replaced(exe: &Path) -> PathBuf {
    exe.with_extension("old")
}

/// Removes the exe the last update replaced.
#[cfg(windows)]
pub(super) fn sweep() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(replaced(&exe));
    }
}

/// Renames the running exe aside and puts the checked one in its place.
#[cfg(windows)]
pub(super) fn install(file: &Path) -> Result<(), Error> {
    let exe = std::env::current_exe().map_err(|e| format!("Failed to find Patchwork: {e}"))?;
    let old = replaced(&exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("Could not replace {}: {e}", exe.display()))?;
    let result = std::fs::copy(file, &exe)
        .map(|_| ())
        .map_err(|e| format!("Could not replace {}: {e}", exe.display()));
    if result.is_err() {
        let _ = std::fs::rename(&old, &exe);
    }
    let _ = std::fs::remove_file(file);
    result.map_err(Error::from)
}

/// Swaps the app bundle for the one in the checked disk image.
#[cfg(target_os = "macos")]
pub(super) fn install(file: &Path) -> Result<(), Error> {
    let result = install_bundle(file);
    let _ = std::fs::remove_file(file);
    result
}

/// Runs `program` and waits for it; an error names `doing`.
#[cfg(target_os = "macos")]
fn run(program: &str, args: &[&std::ffi::OsStr], doing: &str) -> Result<(), Error> {
    let done = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("Could not {doing}: {e}"))?;
    if done.status.success() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&done.stderr);
    Err(format!("Could not {doing}: {}", said.trim()).into())
}

/// Copies the new bundle out of the disk image to beside the running one, then swaps them. A
/// bundle in a folder the user cannot write to is not replaced.
#[cfg(target_os = "macos")]
fn install_bundle(image: &Path) -> Result<(), Error> {
    let bundle = app_bundle().ok_or("Not running from an app bundle.")?;
    let parent = bundle.parent().ok_or("The app bundle has no folder.")?;
    let staging = parent.join(".patchwork-update");
    let _ = std::fs::remove_dir_all(&staging);
    let mount = staging.join("image");
    std::fs::create_dir_all(&mount).map_err(|e| {
        format!(
            "Patchwork cannot write to {}, so download the update from the releases page: {e}",
            parent.display()
        )
    })?;
    let result = (|| {
        let new = staging.join("Patchwork.app");
        run(
            "hdiutil",
            &[
                "attach".as_ref(),
                "-nobrowse".as_ref(),
                "-readonly".as_ref(),
                "-noautoopen".as_ref(),
                "-mountpoint".as_ref(),
                mount.as_os_str(),
                image.as_os_str(),
            ],
            "open the update",
        )?;
        let copied = run(
            "ditto",
            &[mount.join("Patchwork.app").as_os_str(), new.as_os_str()],
            "unpack the update",
        );
        let _ = run(
            "hdiutil",
            &["detach".as_ref(), mount.as_os_str(), "-force".as_ref()],
            "close the update",
        );
        copied?;
        let old = staging.join("previous.app");
        std::fs::rename(&bundle, &old)
            .map_err(|e| format!("Could not replace {}: {e}", bundle.display()))?;
        if let Err(e) = std::fs::rename(&new, &bundle) {
            let _ = std::fs::rename(&old, &bundle);
            return Err(format!("Could not replace {}: {e}", bundle.display()).into());
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}
