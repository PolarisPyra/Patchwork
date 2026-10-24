#!/usr/bin/env bash
# Cuts a release: checks the repo and the build, writes the new version into every package file,
# regenerates CHANGELOG.md with git-cliff, commits both, tags the version, and pushes. Pushing the
# tag starts the Release workflow, which builds the installers and creates the GitHub release.
#
# Usage: scripts/release.sh 0.2.0
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

die() {
  echo "$*" >&2
  exit 1
}

version="${1:-}"
version="${version#v}"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  die "Usage: scripts/release.sh X.Y.Z (for example 0.2.0)"
fi
tag="v$version"

for tool in git git-cliff pnpm cargo; do
  command -v "$tool" > /dev/null || die "$tool is required."
done

if [[ "$(git branch --show-current)" != main ]]; then
  die "Release from main."
fi
if [[ -n "$(git status --porcelain)" ]]; then
  die "The working tree has uncommitted changes. Commit them before releasing."
fi

git fetch --quiet --tags origin
if git rev-parse --quiet --verify "refs/tags/$tag" > /dev/null; then
  die "$tag already exists."
fi
if ! git merge-base --is-ancestor origin/main HEAD; then
  die "main is behind or has diverged from origin/main. Pull or rebase first."
fi

echo "Checking the web app and the Rust crates..."
pnpm typecheck
pnpm format:check
cargo clippy --workspace --all-targets
cargo test --workspace

echo
echo "Release notes for $tag:"
git cliff --config cliff.toml --unreleased --tag "$tag"
echo

read -r -p "Set the version to $version, commit it with CHANGELOG.md, tag $tag, and push main and the tag to origin? [y/N] " answer
if [[ "$answer" != y && "$answer" != Y ]]; then
  die "Release cancelled. Nothing was changed."
fi

scripts/set-version.sh "$version"
# Writes the crates' new version into Cargo.lock.
cargo update --workspace --quiet
git cliff --config cliff.toml --tag "$tag" --output CHANGELOG.md
git add CHANGELOG.md Cargo.lock package.json apps/web/package.json apps/desktop/package.json \
  apps/desktop/src-tauri/tauri.conf.json apps/desktop/src-tauri/Cargo.toml apps/server/Cargo.toml
git commit -m "Release $tag"
git tag -a "$tag" -m "Patchwork $version"
git push origin main "$tag"

echo "Pushed $tag. The Release workflow is building the installers and will create the GitHub"
echo "release. Follow it in the Actions tab."
