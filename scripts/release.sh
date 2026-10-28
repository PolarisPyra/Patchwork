#!/usr/bin/env bash
# Everything that releases Patchwork, on your machine and in the Release workflow
# (.github/workflows/release.yml). docs/release.md explains how it all fits together.
#
# On your machine:
#   scripts/release.sh X.Y.Z            Cut a release, with no questions asked: check, write the
#                                       notes from the commits' Changelog: paragraphs, set the
#                                       version, add the notes to CHANGELOG.md, commit, tag, and
#                                       push, then print the build's link and end. Run it only
#                                       when a release was asked for.
#   scripts/release.sh watch X.Y.Z      Follow X.Y.Z's build to the end, then check that
#                                       latest.json names it.
#   scripts/release.sh version X.Y.Z    Write a version into every file that names it.
#   scripts/release.sh notes X.Y.Z      Publish X.Y.Z's section of CHANGELOG.md again, after a
#                                       fix: to its GitHub release, and to latest.json when X.Y.Z
#                                       is the latest release and the Bunny keys are set.
#   scripts/release.sh pull X.Y.Z       Withdraw X.Y.Z when it went wrong: point latest.json back
#                                       at the release before it. Needs the Bunny keys.
#
# In the Release workflow:
#   scripts/release.sh ci-build linux|windows|macos
#                                       Build this platform's files, unsigned, with the tag's
#                                       version on a tag.
#   scripts/release.sh ci-upload linux|windows|macos
#                                       Upload them under the tag's folder, and write a JSON list
#                                       of them (name, SHA-256) to the step's `assets` output.
#   scripts/release.sh ci-release       Fetch every file back from the update server, check it
#                                       against the build jobs' lists ($LINUX, $WINDOWS, $MACOS),
#                                       sign it and check the signature against the app's key,
#                                       then publish latest.json (also under the tag's folder),
#                                       purge its cached copy, and create the GitHub release.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

PUBLIC=https://updates.patchworkeditor.com
STORAGE=https://ny.storage.bunnycdn.com/patchwork-releases
WORKFLOW=release.yml
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

# Writes the version into the package.json files and the Tauri config, which the app shows and the
# installers use; the Rust crates read it from the Tauri config (apps/server/build.rs), so a release
# leaves Cargo.toml and Cargo.lock alone.
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
  echo "Set the version to $version."
}

cut_release() {
  local version="${1#v}"
  check_version "$version"
  local tag="v$version"

  step "Checking the repo"
  local tool secret
  for tool in git pnpm cargo gh jq curl; do
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
  # The same checks as the CI workflow. They regenerate the wire types, which must already be committed.
  pnpm check
  cargo test --workspace
  [[ -z "$(git status --porcelain)" ]] || die "pnpm check changed files. Commit them, then release again."

  step "Release notes for $tag"
  # Kept until the release is pushed, so an edit to it, or a second run, is used as it is.
  local notes=".release-notes/$tag.md"
  scripts/changelog.sh draft "$tag" "$notes"
  echo
  cat "$notes"
  echo
  scripts/changelog.sh check "$notes" "$version"

  step "Tagging $tag"
  # Commits not on origin yet go up on their own first: GitHub matches the Release workflow's paths
  # against every commit in a push, so one that changes a Cargo file would otherwise start a cache
  # build beside the tag's run.
  if [[ -n "$(git rev-list origin/main..HEAD)" ]]; then
    git push origin main
  fi
  set_version "$version"
  scripts/changelog.sh insert "$notes" "$version"
  git add CHANGELOG.md package.json apps/web/package.json apps/desktop/package.json \
    apps/desktop/src-tauri/tauri.conf.json
  git commit -m "Release $tag"
  git tag -a "$tag" -m "Patchwork $version"
  git push origin main "$tag"
  rm -f "$notes"

  # The build runs on GitHub from here; `scripts/release.sh watch` follows it.
  local run=""
  for _ in $(seq 30); do
    run=$(gh run list --workflow "$WORKFLOW" --branch "$tag" --limit 1 --json databaseId -q '.[0].databaseId // empty')
    [[ -n "$run" ]] && break
    sleep 2
  done
  [[ -n "$run" ]] || die "No Release run started for $tag. Check the Actions tab."
  echo "Released $tag. The build: $(gh run view "$run" --json url -q .url)"
}

# Follows a release's build to the end, then checks that latest.json names it. Ctrl-C stops only
# the watching, not the build.
watch_release() {
  local version="${1#v}"
  check_version "$version"
  local tag="v$version" run url
  run=$(gh run list --workflow "$WORKFLOW" --branch "$tag" --limit 1 --json databaseId -q '.[0].databaseId // empty')
  [[ -n "$run" ]] || die "No Release run for $tag. Check the Actions tab."
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

# Builds with the Tauri CLI. Nothing is signed here: the release job signs the published files, so
# the signing key is never on a machine that runs the dependencies' build scripts.
tauri_build() {
  pnpm --filter patchwork-desktop exec tauri build --bundles "$1"
}

# Sets `files` to the files a platform's build publishes.
platform_files() {
  case "$1" in
    linux) files=(target/release/bundle/appimage/Patchwork_amd64.AppImage) ;;
    windows) files=(target/release/bundle/nsis/*-setup.exe) ;;
    macos) files=(target/release/bundle/dmg/*.dmg target/release/bundle/macos/Patchwork.app.tar.gz) ;;
    *) die "Usage: scripts/release.sh ci-build|ci-upload linux|windows|macos" ;;
  esac
}

ci_build() {
  local platform="${1:-}" files=() file
  platform_files "$platform"
  # A tag pushed without `scripts/release.sh X.Y.Z` still builds with its own version.
  if [[ "${GITHUB_REF:-}" == refs/tags/v* ]]; then set_version "${GITHUB_REF_NAME#v}"; fi
  case "$platform" in
    linux)
      tauri_build appimage
      # Tauri names the file after the version. The release keeps one name across versions,
      # which the updater looks for.
      local version built
      version=$(jq -r .version apps/desktop/src-tauri/tauri.conf.json)
      built="target/release/bundle/appimage/Patchwork_${version}_amd64.AppImage"
      [[ -f "$built" ]] || die "Expected $built after the build."
      mv -f "$built" target/release/bundle/appimage/Patchwork_amd64.AppImage
      ;;
    windows) tauri_build nsis ;;
    macos)
      # The disk image is for new installs; the updater downloads Patchwork.app.tar.gz and unpacks
      # it with tar, expecting Patchwork.app at its top. COPYFILE_DISABLE keeps macOS's ._ files out.
      tauri_build app,dmg
      COPYFILE_DISABLE=1 tar -czf target/release/bundle/macos/Patchwork.app.tar.gz \
        -C target/release/bundle/macos Patchwork.app
      ;;
  esac
  platform_files "$platform"
  for file in "${files[@]}"; do
    [[ -f "$file" ]] || die "Expected $file after the build."
  done
  echo "Built: ${files[*]}"
}

# Uploads a platform's files under the tag's folder, and writes a JSON list of them (name, SHA-256)
# to the step's `assets` output. A separate step from the build, so the storage password is not in
# the environment while the dependencies' build scripts run.
ci_upload() {
  local platform="${1:-}" tag="${GITHUB_REF_NAME:?Run ci-upload from the Release workflow.}"
  local files=() assets='[]' file name digest
  [[ "${GITHUB_REF:-}" == refs/tags/v* ]] || die "ci-upload only runs on a version tag."
  platform_files "$platform"
  for file in "${files[@]}"; do
    [[ -f "$file" ]] || die "Expected $file after the build."
    name=$(basename "$file")
    upload "$file" "$tag/$name"
    digest=$(file_sha256 "$file")
    assets=$(jq -c --arg name "$name" --arg digest "sha256:$digest" '. + [{name: $name, digest: $digest}]' <<< "$assets")
    echo "Uploaded $PUBLIC/$tag/$name"
  done
  echo "assets=$assets" >> "${GITHUB_OUTPUT:-/dev/stdout}"
}

file_sha256() {
  # macOS has shasum but not always sha256sum.
  (sha256sum "$1" 2> /dev/null || shasum -a 256 "$1") | cut -d' ' -f1
}

# The key the app checks updates against, as built into it.
app_signing_key() {
  sed -n 's/^const SIGNING_KEY: &str = "\(.*\)";$/\1/p' apps/desktop/src-tauri/src/updates.rs
}

# The Tauri CLI version the lockfile pins, for signing without installing the project.
tauri_cli_version() {
  awk '/^      .@tauri-apps\/cli.:$/ { found = 1; next } found && /version:/ { print $2; exit }' pnpm-lock.yaml
}

ci_release() {
  local tag="${GITHUB_REF_NAME:?Run ci-release from the Release workflow.}"
  local version="${tag#v}" key cli dir=release-check name digest asset assets='[]'
  local built="${LINUX:?} ${WINDOWS:?} ${MACOS:?}"
  [[ -n "${BUNNY_API_KEY:-}" ]] || die "BUNNY_API_KEY is not set."
  [[ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]] || die "TAURI_SIGNING_PRIVATE_KEY is not set."
  key=$(app_signing_key)
  [[ -n "$key" ]] || die "Could not read SIGNING_KEY from apps/desktop/src-tauri/src/updates.rs."
  cli=$(tauri_cli_version)
  [[ -n "$cli" ]] || die "Could not read the @tauri-apps/cli version from pnpm-lock.yaml."
  mkdir -p "$dir"

  # Each file is fetched back from the update server, so what is checked and signed is what users
  # download: it must match the SHA-256 its build job reported, and its signature must check out
  # against the key built into the app, as the updater will check it.
  while IFS= read -r asset; do
    name=$(jq -r .name <<< "$asset")
    digest=$(jq -r .digest <<< "$asset")
    curl -fsS --retry 3 -o "$dir/$name" "$PUBLIC/$tag/$name" || die "Could not download $PUBLIC/$tag/$name."
    [[ "sha256:$(file_sha256 "$dir/$name")" == "$digest" ]] \
      || die "$PUBLIC/$tag/$name does not match the file its build job uploaded."
    npx --yes "@tauri-apps/cli@$cli" signer sign --app-version "$version" "$dir/$name" > /dev/null
    base64 -d "$dir/$name.sig" > "$dir/$name.minisig"
    minisign -Vq -P "$key" -m "$dir/$name" -x "$dir/$name.minisig" \
      || die "The signature of $name does not check out against the app's key. Is TAURI_SIGNING_PRIVATE_KEY the right key?"
    assets=$(jq -c --arg name "$name" --rawfile sig "$dir/$name.minisig" '. + [{name: $name, sig: $sig}]' <<< "$assets")
    echo "Checked and signed $name"
  done < <(jq -c '.[]' <<< "$built")

  # The tag's section of CHANGELOG.md, which release.sh committed before tagging.
  scripts/changelog.sh section "$tag" > RELEASE_NOTES.md
  [[ -s RELEASE_NOTES.md ]] || echo "CHANGELOG.md has no section for $tag, so the release has no notes." >&2
  # In the shape of GitHub's release API, plus each file's signature and the notes, as the updater
  # reads it. It goes up after the build jobs uploaded every file, so it never names a missing one.
  jq -n --arg tag "$tag" --arg public "$PUBLIC" --rawfile body RELEASE_NOTES.md \
    --argjson built "$(jq -s 'add' <<< "$built")" --argjson signed "$assets" \
    '{tag_name: $tag, body: $body, assets: ($built | map(. as $file | {
      name,
      browser_download_url: "\($public)/\($tag)/\(.name)",
      digest,
      signature: ($signed[] | select(.name == $file.name) | .sig)
    }))}' > latest.json
  cat latest.json
  # The tag's own copy lets `scripts/release.sh pull` put this release back as the latest.
  upload latest.json "$tag/latest.json"
  upload latest.json latest.json
  curl -fsS -X POST -H "AccessKey: $BUNNY_API_KEY" "https://api.bunny.net/purge?url=$PUBLIC/latest.json"
  echo "Published $PUBLIC/latest.json"

  # The installers stay on the update server; the GitHub release keeps the tag's notes.
  gh release create "$tag" --title "Patchwork $tag" --notes-file RELEASE_NOTES.md
}

# Withdraws a release that went wrong: latest.json goes back to the release before it, so copies
# that have not updated yet stop being offered it. Copies already on it keep it.
pull_release() {
  local version="${1#v}"
  check_version "$version"
  local tag="v$version" manifest previous="" candidate found=""
  [[ -n "${BUNNY_STORAGE_PASSWORD:-}" && -n "${BUNNY_API_KEY:-}" ]] \
    || die "Set BUNNY_STORAGE_PASSWORD and BUNNY_API_KEY to pull a release."
  manifest=$(curl -fsS "$PUBLIC/latest.json") || die "Could not fetch $PUBLIC/latest.json."
  [[ "$(jq -r .tag_name <<< "$manifest")" == "$tag" ]] \
    || die "latest.json names $(jq -r .tag_name <<< "$manifest"), not $tag, so there is nothing to pull."
  git fetch --quiet --tags origin
  mkdir -p .release-notes
  for candidate in $(git tag -l 'v*' --sort=-v:refname); do
    if [[ -n "$found" ]] && curl -fsS -o .release-notes/latest.json "$PUBLIC/$candidate/latest.json" 2> /dev/null; then
      previous="$candidate"
      break
    fi
    if [[ "$candidate" == "$tag" ]]; then found=1; fi
  done
  [[ -n "$previous" ]] || die "No release before $tag has its own copy of latest.json to go back to."
  upload .release-notes/latest.json latest.json
  curl -fsS -X POST -H "AccessKey: $BUNNY_API_KEY" "https://api.bunny.net/purge?url=$PUBLIC/latest.json"
  rm -f .release-notes/latest.json
  echo "latest.json names $previous again, so $tag is no longer offered as an update."
}

# Publishes a release's notes again from CHANGELOG.md, for a fix made after the release.
republish_notes() {
  local version="${1#v}"
  check_version "$version"
  local tag="v$version"
  local notes=".release-notes/$tag.published.md"
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
  pull) pull_release "${2:-}" ;;
  watch) watch_release "${2:-}" ;;
  ci-build) ci_build "${2:-}" ;;
  ci-upload) ci_upload "${2:-}" ;;
  ci-release) ci_release ;;
  -h | --help) usage ;;
  "") usage >&2 && exit 1 ;;
  *) cut_release "$1" ;;
esac
