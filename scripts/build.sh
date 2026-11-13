#!/usr/bin/env bash
# Builds the Linux AppImage at target/release/Patchwork_amd64.AppImage.
#
# It holds the app, whose libraries (X11, Wayland keyboard, D-Bus) are on every Linux desktop, and
# the helpers Patchwork copies to SSH hosts: static Linux ones for x86_64 and aarch64, and the
# Windows one. They are built with cargo-zigbuild, which needs zig and cargo-zigbuild on PATH and
# the two musl targets and x86_64-pc-windows-gnu in rustup.
# appimagetool comes from PATH, or is downloaded to target/ the first time.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release -p patchwork

app=target/release/Patchwork.AppDir
rm -rf "$app"
mkdir -p "$app/usr/bin"
cp target/release/patchwork "$app/usr/bin/"
for arch in x86_64 aarch64; do
  cargo zigbuild --release -p patchwork-remote --target "$arch-unknown-linux-musl"
  cp "target/$arch-unknown-linux-musl/release/patchwork-remote" "$app/usr/bin/patchwork-remote-linux-$arch"
done
cargo zigbuild --release -p patchwork-remote --target x86_64-pc-windows-gnu
cp target/x86_64-pc-windows-gnu/release/patchwork-remote.exe "$app/usr/bin/patchwork-remote-windows-x86_64.exe"
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
