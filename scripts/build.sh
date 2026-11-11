#!/usr/bin/env bash
# Builds the Linux AppImage at target/release/Patchwork_amd64.AppImage.
#
# It holds only the app: the libraries it links against (X11, Wayland keyboard, D-Bus) are on every
# Linux desktop. appimagetool comes from PATH, or is downloaded to target/ the first time.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release -p patchwork

app=target/release/Patchwork.AppDir
rm -rf "$app"
mkdir -p "$app/usr/bin"
cp target/release/patchwork "$app/usr/bin/"
ln -s usr/bin/patchwork "$app/AppRun"
sed 's|@EXEC@|patchwork|' packaging/patchwork.desktop > "$app/dev.patchwork.ade.desktop"
cp assets/logo.png "$app/dev.patchwork.ade.png"

# appimagetool 1.9.1 for x86_64, by its SHA-256. A new version needs its hash here too: download
# it and run sha256sum on it.
tool_sha256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
tool=$(command -v appimagetool || echo target/appimagetool)
if [[ ! -x $tool ]]; then
  curl -fsSL -o "$tool" \
    https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
  chmod +x "$tool"
fi
# The downloaded copy builds what the release then signs, so it is run only when it is the file
# that was pinned, each time. One found on PATH is the system's own to vouch for.
if [[ $tool == target/appimagetool ]] &&
  ! echo "$tool_sha256  $tool" | sha256sum --check --status; then
  echo "$tool is not appimagetool 1.9.1 as pinned in scripts/build.sh; delete it and run again" >&2
  exit 1
fi
ARCH=x86_64 "$tool" --no-appstream "$app" target/release/Patchwork_amd64.AppImage
