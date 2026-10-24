#!/usr/bin/env bash
# Writes one version into every file that names the app's version: the package.json files, the
# Tauri config (which the app shows and the installers use), and the Rust crates. scripts/release.sh
# runs it before tagging, and the release workflow runs it again from the tag before building.
#
# Usage: scripts/set-version.sh 0.2.0
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

version="${1:-}"
version="${version#v}"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Usage: scripts/set-version.sh X.Y.Z (for example 0.2.0)" >&2
  exit 1
fi

for file in package.json apps/web/package.json apps/desktop/package.json \
  apps/desktop/src-tauri/tauri.conf.json; do
  node -e '
    const fs = require("fs");
    const [file, version] = process.argv.slice(1);
    const json = JSON.parse(fs.readFileSync(file, "utf8"));
    json.version = version;
    fs.writeFileSync(file, JSON.stringify(json, null, 2) + "\n");
  ' "$file" "$version"
done

# Perl rather than sed: it replaces only the package's own version (the first match) the same way
# on Linux, macOS, and Git Bash on Windows.
for file in apps/desktop/src-tauri/Cargo.toml apps/server/Cargo.toml; do
  perl -0pi -e "s/^version = .*/version = \"$version\"/m" "$file"
done

echo "Set the version to $version."
