#!/usr/bin/env bash
# Everything that releases Patchwork, on your machine and in the Release workflow
# (.github/workflows/release-appimage.yml). docs/release.md explains how it all fits together.
#
# On your machine:
#   scripts/release.sh X.Y.Z            Cut a release: check, draft the notes with Claude for you
#                                       to approve, set the version, add the notes to
#                                       CHANGELOG.md, commit, tag, push, follow the workflow, and
#                                       check that latest.json names the new version.
#   scripts/release.sh version X.Y.Z    Write a version into every file that names it.
#   scripts/release.sh notes X.Y.Z      Publish X.Y.Z's section of CHANGELOG.md again, after a
#                                       fix: to its GitHub release, and to latest.json when X.Y.Z
#                                       is the latest release and the Bunny keys are set.
#
# In the Release workflow:
#   scripts/release.sh ci-build linux|windows|macos
#                                       On a tag: set the tag's version, build this platform's
#                                       files, upload them under the tag's folder, and write a
#                                       JSON list of them (name, SHA-256, signature) to the step's
#                                       `assets` output. Without a tag: build only.
#   scripts/release.sh ci-release       Write latest.json from the build jobs' lists ($LINUX,
#                                       $WINDOWS, $MACOS) and the tag's notes, upload it, purge
#                                       its cached copy, and create the GitHub release.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

PUBLIC=https://updates.patchworkeditor.com
STORAGE=https://ny.storage.bunnycdn.com/patchwork-releases
WORKFLOW=release-appimage.yml
SECRETS=(BUNNY_STORAGE_PASSWORD BUNNY_API_KEY TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD)

die() {
  echo "$*" >&2
  exit 1
}
step() {
  echo
  echo "== $*"
}
usage() {
  sed -n '2,/^set -euo/p' "${BASH_SOURCE[0]}" | sed '$d' | sed 's/^# \{0,1\}//'
}
check_version() {
  [[ "$1" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "Not a version: $1 (expected X.Y.Z, for example 0.2.0)."
}

# Writes the version into the package.json files, the Tauri config (which the app shows and the
# installers use), and the Rust crates.
set_version() {
  local version="$1" file
  check_version "$version"
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
}

cut_release() {
  local version="${1#v}"
  check_version "$version"
  local tag="v$version"

  step "Checking the repo"
  local tool secret
  for tool in git pnpm cargo gh jq curl claude; do
    command -v "$tool" > /dev/null || die "$tool is required."
  done
  gh auth status > /dev/null 2>&1 || die "gh is not signed in. Run gh auth login."
  [[ "$(git branch --show-current)" == main ]] || die "Release from main."
  [[ -z "$(git status --porcelain)" ]] || die "The working tree has uncommitted changes. Commit them before releasing."
  git fetch --quiet --tags origin
  ! git rev-parse --quiet --verify "refs/tags/$tag" > /dev/null || die "$tag already exists."
  git merge-base --is-ancestor origin/main HEAD \
    || die "main is behind or has diverged from origin/main. Pull or rebase first."
  local secrets
  secrets=$(gh secret list --json name -q '.[].name')
  for secret in "${SECRETS[@]}"; do
    grep -qx "$secret" <<< "$secrets" || die "The repo has no $secret secret. Set it with gh secret set $secret."
  done
  echo "main is clean and up to date, and the workflow's secrets are set."

  step "Checking the web app and the Rust crates"
  pnpm typecheck
  pnpm format:check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace

  step "Release notes for $tag"
  # Kept until the release is pushed, so running it again reuses them rather than drafting anew.
  local notes=".release-notes/$tag.md" answer
  scripts/changelog.sh draft "$tag" "$notes"
  while true; do
    echo
    cat "$notes"
    echo
    read -r -p "Release $version with these notes: set the version, add them to CHANGELOG.md, commit, tag $tag, and push main and the tag? [y]es, [e]dit the notes, [r]edraft them, or [N]o: " answer
    case "$answer" in
      y | Y) break ;;
      # Unquoted, so an editor with arguments, such as "code --wait", runs.
      e | E) ${VISUAL:-${EDITOR:-vi}} "$notes" && scripts/changelog.sh check "$notes" "$version" || true ;;
      r | R)
        rm -f "$notes"
        scripts/changelog.sh draft "$tag" "$notes"
        ;;
      *) die "Release cancelled. Nothing was changed; the notes stay in $notes." ;;
    esac
  done
  scripts/changelog.sh check "$notes" "$version"

  step "Tagging $tag"
  set_version "$version"
  # Writes the crates' new version into Cargo.lock.
  cargo update --workspace --quiet
  scripts/changelog.sh insert "$notes" "$version"
  git add CHANGELOG.md Cargo.lock package.json apps/web/package.json apps/desktop/package.json \
    apps/desktop/src-tauri/tauri.conf.json apps/desktop/src-tauri/Cargo.toml apps/server/Cargo.toml
  git commit -m "Release $tag"
  git tag -a "$tag" -m "Patchwork $version"
  git push origin main "$tag"
  rm -f "$notes" "$notes.json"

  # Stopping the script from here on (Ctrl-C) does not stop the build.
  step "Following the Release workflow"
  local run="" url
  for _ in $(seq 30); do
    run=$(gh run list --workflow "$WORKFLOW" --branch "$tag" --limit 1 --json databaseId -q '.[0].databaseId // empty')
    [[ -n "$run" ]] && break
    sleep 2
  done
  [[ -n "$run" ]] || die "No Release run started for $tag. Check the Actions tab."
  url=$(gh run view "$run" --json url -q .url)
  echo "$url"
  if ! gh run watch "$run" --exit-status --interval 30 > /dev/null; then
    gh run view "$run" --json jobs -q '.jobs[] | "  \(.name): \(.conclusion)"'
    gh run view "$run" --log-failed 2> /dev/null | grep '##\[error\]' | head -n 10 || true
    die "The Release workflow failed: $url"
  fi
  gh run view "$run" --json jobs -q '.jobs[] | "  \(.name): \(.conclusion)"'

  step "Checking $PUBLIC/latest.json"
  local manifest published
  manifest=$(curl -fsS "$PUBLIC/latest.json") || die "Could not fetch $PUBLIC/latest.json."
  published=$(jq -r .tag_name <<< "$manifest")
  [[ "$published" == "$tag" ]] || die "latest.json names $published, not $tag."
  echo "Installed copies on 0.10.10 or newer will update to $version. Downloads:"
  jq -r '.assets[] | "  \(.browser_download_url)"' <<< "$manifest"
}

# Uploads a file to the storage zone at the given path.
upload() {
  [[ -n "${BUNNY_STORAGE_PASSWORD:-}" ]] || die "BUNNY_STORAGE_PASSWORD is not set."
  curl -fsS --retry 3 -T "$1" -H "AccessKey: $BUNNY_STORAGE_PASSWORD" \
    -H "Content-Type: application/octet-stream" "$STORAGE/$2" > /dev/null
}

# Builds with the Tauri CLI, signing the updater files when CI passes the signing key.
tauri_build() {
  local args=()
  if [[ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
    # The bundler signs with the updater's public key, which only this config carries.
    args=(--config "${GITHUB_WORKSPACE:-$repo_root}/apps/desktop/src-tauri/tauri.signing.conf.json")
  fi
  pnpm --filter patchwork-desktop exec tauri build --bundles "$1" ${args[@]+"${args[@]}"}
}

ci_build() {
  local platform="${1:-}" tag="" files=()
  [[ "${GITHUB_REF:-}" == refs/tags/v* ]] && tag="$GITHUB_REF_NAME"
  # A tag pushed without `scripts/release.sh X.Y.Z` still builds with its own version.
  [[ -n "$tag" ]] && set_version "${tag#v}"
  case "$platform" in
    linux)
      tauri_build appimage
      # Tauri names the file after the version. The release keeps one name across versions,
      # which the updater looks for.
      local version built out=target/release/bundle/appimage/Patchwork_amd64.AppImage
      version=$(jq -r .version apps/desktop/src-tauri/tauri.conf.json)
      built="target/release/bundle/appimage/Patchwork_${version}_amd64.AppImage"
      [[ -f "$built" ]] || die "Expected $built after the build."
      mv -f "$built" "$out"
      [[ -f "$built.sig" ]] && mv -f "$built.sig" "$out.sig"
      files=("$out")
      ;;
    windows)
      tauri_build nsis
      files=(target/release/bundle/nsis/*-setup.exe)
      ;;
    macos)
      # The disk image is for new installs; the updater downloads Patchwork.app.tar.gz, which a
      # signed build packs itself.
      tauri_build app,dmg
      local bundle=target/release/bundle/macos
      [[ -f "$bundle/Patchwork.app.tar.gz" ]] || tar -czf "$bundle/Patchwork.app.tar.gz" -C "$bundle" Patchwork.app
      files=(target/release/bundle/dmg/*.dmg "$bundle/Patchwork.app.tar.gz")
      ;;
    *) die "Usage: scripts/release.sh ci-build linux|windows|macos" ;;
  esac
  if [[ -z "$tag" ]]; then
    echo "Not a tag, so nothing is published. Built: ${files[*]}"
    return
  fi

  local assets='[]' file name sig digest
  for file in "${files[@]}"; do
    [[ -f "$file" ]] || die "Expected $file after the build."
    name=$(basename "$file")
    upload "$file" "$tag/$name"
    sig=null
    [[ -f "$file.sig" ]] && sig=$(jq -Rs . < "$file.sig")
    # macOS has shasum but not always sha256sum.
    digest=$( (sha256sum "$file" 2> /dev/null || shasum -a 256 "$file") | cut -d' ' -f1)
    assets=$(jq -c --arg name "$name" --arg digest "sha256:$digest" --argjson sig "$sig" \
      '. + [{name: $name, digest: $digest, sig: $sig}]' <<< "$assets")
    echo "Uploaded $PUBLIC/$tag/$name"
  done
  echo "assets=$assets" >> "${GITHUB_OUTPUT:-/dev/stdout}"
}

ci_release() {
  local tag="${GITHUB_REF_NAME:?Run ci-release from the Release workflow.}"
  [[ -n "${BUNNY_API_KEY:-}" ]] || die "BUNNY_API_KEY is not set."
  # The tag's section of CHANGELOG.md, which release.sh committed before tagging.
  scripts/changelog.sh section "$tag" > RELEASE_NOTES.md
  [[ -s RELEASE_NOTES.md ]] || echo "CHANGELOG.md has no section for $tag, so the release has no notes." >&2
  # In the shape of GitHub's release API, plus each file's signature and the notes, as the updater
  # reads it. It goes up after the build jobs uploaded every file, so it never names a missing one.
  jq -n --arg tag "$tag" --arg public "$PUBLIC" --rawfile body RELEASE_NOTES.md \
    --argjson linux "${LINUX:?}" --argjson windows "${WINDOWS:?}" --argjson macos "${MACOS:?}" \
    '{tag_name: $tag, body: $body, assets: ($linux + $windows + $macos | map({
      name,
      browser_download_url: "\($public)/\($tag)/\(.name)",
      digest,
      signature: (.sig | if . then @base64d else null end)
    }))}' > latest.json
  cat latest.json
  upload latest.json latest.json
  curl -fsS -X POST -H "AccessKey: $BUNNY_API_KEY" "https://api.bunny.net/purge?url=$PUBLIC/latest.json"
  echo "Published $PUBLIC/latest.json"

  # The installers stay on the update server; the GitHub release keeps the tag's notes.
  gh release create "$tag" --title "Patchwork $tag" --notes-file RELEASE_NOTES.md
}

# Publishes a release's notes again from CHANGELOG.md, for a fix made after the release.
republish_notes() {
  local version="${1#v}"
  check_version "$version"
  local tag="v$version" notes=".release-notes/$tag.published.md"
  mkdir -p .release-notes
  scripts/changelog.sh section "$tag" > "$notes"
  [[ -s "$notes" ]] || die "CHANGELOG.md has no section for $version."
  scripts/changelog.sh check "$notes" "$version"
  gh release edit "$tag" --notes-file "$notes" > /dev/null
  echo "Updated the notes of the GitHub release $tag."
  local manifest
  manifest=$(curl -fsS "$PUBLIC/latest.json") || die "Could not fetch $PUBLIC/latest.json."
  if [[ "$(jq -r .tag_name <<< "$manifest")" != "$tag" ]]; then
    echo "latest.json is for another release, so the app's update notes are unchanged."
  elif [[ -z "${BUNNY_STORAGE_PASSWORD:-}" || -z "${BUNNY_API_KEY:-}" ]]; then
    echo "Set BUNNY_STORAGE_PASSWORD and BUNNY_API_KEY to update the notes the app shows for this update too."
  else
    jq --rawfile body "$notes" '.body = $body' <<< "$manifest" > .release-notes/latest.json
    upload .release-notes/latest.json latest.json
    curl -fsS -X POST -H "AccessKey: $BUNNY_API_KEY" "https://api.bunny.net/purge?url=$PUBLIC/latest.json"
    echo "Updated the notes in $PUBLIC/latest.json."
    rm -f .release-notes/latest.json
  fi
  rm -f "$notes"
}

case "${1:-}" in
  version) set_version "${2:-}" ;;
  notes) republish_notes "${2:-}" ;;
  ci-build) ci_build "${2:-}" ;;
  ci-release) ci_release ;;
  -h | --help) usage ;;
  "") usage >&2 && exit 1 ;;
  *) cut_release "$1" ;;
esac
