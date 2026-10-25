#!/usr/bin/env bash
# Builds Patchwork. `appimage` writes Patchwork_amd64.AppImage. `install` builds the AppImage and
# installs it as $PREFIX/bin/patchwork (default ~/.local) with its icons and a launcher. The app
# updates that file in place from the GitHub releases.
#
# Usage: scripts/build.sh appimage | install
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

die() {
  echo "$*" >&2
  exit 1
}

usage() {
  cat <<'EOF'
Usage: scripts/build.sh appimage | install

  appimage  Build target/release/bundle/appimage/Patchwork_amd64.AppImage
  install   Build the AppImage and install it under $PREFIX (default ~/.local): bin/patchwork,
            the icons, and share/applications/dev.patchwork.ade.desktop
EOF
}

version="$(jq -r .version apps/desktop/src-tauri/tauri.conf.json)"
app_id="dev.patchwork.ade"

require_tools() {
  local tool
  for tool in pnpm jq; do
    command -v "$tool" > /dev/null || die "$tool is required."
  done
}

build_appimage() {
  require_tools
  # The updater signature needs the release signing key: CI passes it in the environment, and a
  # local build reads it from ~/.tauri. Without the key, the build has no signature.
  local key="$HOME/.tauri/patchwork.key" args=()
  if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" && -f "$key" ]]; then
    export TAURI_SIGNING_PRIVATE_KEY="$key"
    export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat "$key.password" 2>/dev/null || true)"
  fi
  if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
    args=(--config '{"bundle":{"createUpdaterArtifacts":false}}')
  fi
  pnpm --filter patchwork-desktop exec tauri build --bundles appimage "${args[@]}"
  # Tauri names the file after the version. Rename it so the name stays the same across releases.
  local built="target/release/bundle/appimage/Patchwork_${version}_amd64.AppImage"
  local out="target/release/bundle/appimage/Patchwork_amd64.AppImage"
  [[ -f "$built" ]] || die "Expected $built after the build."
  mv -f "$built" "$out"
  [[ -f "$built.sig" ]] && mv -f "$built.sig" "$out.sig"
  echo "AppImage: $repo_root/$out"
}

install_app() {
  require_tools
  local prefix="${PREFIX:-$HOME/.local}"
  local bin_path="$prefix/bin/patchwork"
  local apps_dir="$prefix/share/applications"
  local icons_dir="$prefix/share/icons/hicolor"
  local launcher="$apps_dir/$app_id.desktop"

  build_appimage
  # Moved into place rather than written over, so a running Patchwork keeps its file and the new
  # version starts the next time it opens.
  install -Dm755 target/release/bundle/appimage/Patchwork_amd64.AppImage "$bin_path.new"
  mv -f "$bin_path.new" "$bin_path"
  echo "AppImage: $bin_path"

  install -Dm644 apps/desktop/src-tauri/icons/logo.png "$icons_dir/512x512/apps/$app_id.png"
  # Earlier installs wrote smaller sizes of the icon. The shell picks one of those for notifications,
  # so remove them and let the theme scale the 512px copy.
  find "$icons_dir" -path "*/apps/$app_id.png" ! -path "$icons_dir/512x512/apps/$app_id.png" -delete
  echo "Icons: $icons_dir"

  # The launcher's Exec is the absolute binary path, so it goes stale when the prefix or the
  # template changes. Compare the rendered launcher with the installed copy before rewriting it.
  local rendered
  rendered="$(sed "s|@EXEC@|$bin_path|" packaging/patchwork.desktop)"
  # HACK: see the launcher module in apps/desktop/src-tauri/src/logo.rs. The app points the
  # launcher at the icon of the chosen color way, so keep that choice.
  local chosen_icon
  chosen_icon="$(sed -n "s/^Icon=\($app_id-logo-.*\)$/\1/p" "$launcher" 2> /dev/null || true)"
  if [[ -n "$chosen_icon" && -f "$icons_dir/512x512/apps/$chosen_icon.png" ]]; then
    rendered="$(sed "s|^Icon=.*|Icon=$chosen_icon|" <<< "$rendered")"
  fi
  mkdir -p "$apps_dir"
  if [[ ! -f "$launcher" ]]; then
    echo "Launcher: new install, writing $launcher"
  elif [[ "$rendered" == "$(cat "$launcher")" ]]; then
    echo "Launcher: up to date"
  else
    echo "Launcher: stale, rewriting $launcher"
  fi
  printf '%s\n' "$rendered" > "$launcher"
  if command -v desktop-file-validate > /dev/null; then
    desktop-file-validate "$launcher"
  fi

  refresh_caches "$apps_dir" "$icons_dir"

  case ":$PATH:" in
    *":$prefix/bin:"*) ;;
    *) echo "Note: $prefix/bin is not on PATH, so the 'patchwork' command won't resolve." ;;
  esac
  echo "Installed Patchwork $version. Open it from the application menu."
}

refresh_caches() {
  local apps_dir="$1" icons_dir="$2"
  if command -v update-desktop-database > /dev/null; then
    update-desktop-database -q "$apps_dir"
  fi
  if command -v gtk-update-icon-cache > /dev/null && [[ -f "$icons_dir/index.theme" ]]; then
    gtk-update-icon-cache -q -t -f "$icons_dir"
  fi
  # Plasma 6 keeps its own application cache, which is what the KDE menu reads.
  if command -v kbuildsycoca6 > /dev/null; then
    kbuildsycoca6 --noincremental > /dev/null 2>&1 \
      || echo "Warning: kbuildsycoca6 failed; the KDE menu may update after you log in again."
  fi
}

case "${1:-}" in
  appimage) build_appimage ;;
  install) install_app ;;
  -h | --help) usage ;;
  *)
    usage >&2
    exit 1
    ;;
esac
