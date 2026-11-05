#!/usr/bin/env bash
# One name per platform, for every version.
#
# Tauri names each bundle after the version it built: Patchwork_26.1.0_x64-setup.exe,
# Patchwork_26.1.0_aarch64.dmg, Patchwork_26.1.0_amd64.AppImage. A download link that carried that
# name would break at every release, so the version comes out of it right after the build and
# before anything is published or signed. The release, latest.json, and the README then name
# the same file from one release to the next.
#
# The updater does not care: it picks its file by suffix (`-setup.exe`, `.AppImage`) or by its fixed
# name (`Patchwork.app.tar.gz`), so no version is part of what it looks for. See `is_own_asset` in
# apps/desktop/src-tauri/src/updates/release.rs.
#
# Sourced by release.sh and build.sh. Both call it with the path Tauri produced.

# The name `file` has once its version is out of it: Patchwork_26.1.0_x64-setup.exe comes back as
# Patchwork_x64-setup.exe. The version sits between two underscores in every bundle the release
# publishes (the AppImage, the Windows installer, and the disk image), and a name that does not
# carry it is returned unchanged.
stable_bundle_name() {
  local file="$1" version="$2" name
  name=$(basename "$file")
  name=${name//_${version}_/_}
  printf '%s\n' "$name"
}
