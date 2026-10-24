#!/usr/bin/env bash
# Builds Patchwork. `appimage` writes Patchwork_amd64.AppImage. `install` builds the release binary
# and installs it under $PREFIX (default ~/.local) with its icons and a launcher. The launcher has
# its own id, so it leaves the launcher of an AppImage you already run untouched.
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
  install   Build the release binary and install it under $PREFIX (default ~/.local):
            bin/patchwork, the icons, and share/applications/dev.patchwork.ade.desktop
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
  pnpm --filter patchwork-desktop exec tauri build --bundles appimage
  # Tauri names the file after the version. Rename it so the name stays the same across releases.
  local built="target/release/bundle/appimage/Patchwork_${version}_amd64.AppImage"
  local out="target/release/bundle/appimage/Patchwork_amd64.AppImage"
  [[ -f "$built" ]] || die "Expected $built after the build."
  mv -f "$built" "$out"
  echo "AppImage: $repo_root/$out"
}

stop_running_instances() {
  local dev_bin="$repo_root/target/debug/patchwork"
  local pids=() pid exe
  for pid in $(pgrep -x patchwork || true); do
    exe="$(readlink "/proc/$pid/exe" 2>/dev/null || true)"
    # The dev session runs the debug build from this repo. Leave it running.
    [[ "$exe" == "$dev_bin" ]] && continue
    pids+=("$pid")
  done
  (( ${#pids[@]} )) || return 0

  echo "Stopping running Patchwork: ${pids[*]}"
  kill -TERM "${pids[@]}" 2>/dev/null || true
  local _ alive
  for _ in {1..50}; do
    alive=0
    for pid in "${pids[@]}"; do
      kill -0 "$pid" 2>/dev/null && alive=1
    done
    (( alive )) || return 0
    sleep 0.2
  done
  echo "Some instances ignored SIGTERM; killing them."
  kill -KILL "${pids[@]}" 2>/dev/null || true
}

install_app() {
  require_tools
  local prefix="${PREFIX:-$HOME/.local}"
  local bin_path="$prefix/bin/patchwork"
  local apps_dir="$prefix/share/applications"
  local icons_dir="$prefix/share/icons/hicolor"
  local launcher="$apps_dir/$app_id.desktop"

  stop_running_instances
  pnpm --filter patchwork-desktop exec tauri build --no-bundle
  [[ -x target/release/patchwork ]] || die "Expected target/release/patchwork after the build."

  install -Dm755 target/release/patchwork "$bin_path"
  echo "Binary: $bin_path"

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
