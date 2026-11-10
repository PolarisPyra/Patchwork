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

tool=$(command -v appimagetool || echo target/appimagetool)
if [[ ! -x $tool ]]; then
  curl -fsSL -o "$tool" \
    https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
  chmod +x "$tool"
fi
ARCH=x86_64 "$tool" --no-appstream "$app" target/release/Patchwork_amd64.AppImage
