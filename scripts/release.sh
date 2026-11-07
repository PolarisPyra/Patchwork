#!/usr/bin/env bash
# Everything that releases Patchwork, on your machine and in the Release workflow
# (.github/workflows/release.yml). docs/release.md explains how it all fits together.
#
# After each push to main that passes the checks, the Release workflow keeps a "Release vYY.M.N"
# pull request up to date with what users would get; merging it releases. These are for releasing
# from your machine instead.
#
# On your machine:
#   scripts/release.sh bump             Cut the next release now, with no questions asked: check,
#                                       write the notes from the commits' Changelog: paragraphs,
#                                       set the version, add the notes to CHANGELOG.md, commit,
#                                       tag, and push, then print the build's link and end. The
#                                       version is CalVer, YY.M.N, counted from the newest tag:
#                                       the next N this month (26.11.4 -> 26.11.5), or N 0 in a
#                                       new month (26.11.5 -> 26.12.0).
#   scripts/release.sh YY.M.N           The same, for exactly that version.
#   scripts/release.sh next             Print the version that bump gives, and change nothing.
#   scripts/release.sh watch YY.M.N     Follow YY.M.N's build to the end, then check that
#                                       latest.json names it.
#   scripts/release.sh version YY.M.N   Write a version into every file that names it. `next`
#                                       works here too.
#   scripts/release.sh notes YY.M.N     Publish YY.M.N's section of CHANGELOG.md again, after a
#                                       fix: to its GitHub release and its latest.json.
#   scripts/release.sh pull YY.M.N      Withdraw YY.M.N when it went wrong: make the release before
#                                       it the latest again, and send copies already on YY.M.N
#                                       back to it unless YY.M.N changed the database.
#
# In the Release workflow, where $RELEASE_TAG names the tag being published:
#   scripts/release.sh ci-plan          Write the version main carries to the step's `version`
#                                       output when it has no tag yet, because a release pull
#                                       request was merged, and nothing when it has one.
#   scripts/release.sh ci-pr            When a commit since the last tag has a Changelog:
#                                       paragraph for users, write the notes, set the next version,
#                                       add the notes to CHANGELOG.md, commit that on top of the
#                                       checked commit, force-push it to the release branch, and
#                                       open or update its pull request. Close it when none has.
#   scripts/release.sh ci-tag YY.M.N    Tag the commit that set YY.M.N, push the tag, and write it
#                                       to the step's `tag` output.
#   scripts/release.sh ci-build linux|windows|macos
#                                       Build this platform's files, unsigned, with the tag's
#                                       version when there is one. For a tag, gather them in
#                                       release-files/ for the job's artifact, and write a JSON
#                                       list of them (name, SHA-256) to the step's `assets` output.
#   scripts/release.sh ci-release       Check every file the build jobs' artifacts brought into
#                                       release-check/ against their lists ($LINUX, $WINDOWS,
#                                       $MACOS), sign it and check the signature against the app's
#                                       key, then create the GitHub release with the files and
#                                       latest.json.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# The one place that takes the version out of a bundle's name, shared with scripts/build.sh.
# shellcheck source=scripts/bundle-names.sh
source "$repo_root/scripts/bundle-names.sh"

REPO_URL=https://github.com/PolarisPyra/Patchwork
# What the updater reads: the newest release's own latest.json.
LATEST_JSON=$REPO_URL/releases/latest/download/latest.json
WORKFLOW=release.yml
# The branch the Release workflow keeps the next release on, for its pull request. Only it writes
# there.
RELEASE_BRANCH=release
# What the Windows installer is built for, on Linux; its files land in target/<this>/release.
WINDOWS_TARGET=x86_64-pc-windows-msvc
# A release that changed these can't be rolled back to the one before it: see withdrawn_tags.
MIGRATIONS=crates/server/src/persistence/migrations
SECRETS=(TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD)

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
# A version is CalVer, YY.M.N: the two-digit year, the month, and which release of that month it
# is, from 0. The year is two digits, which tells a CalVer version apart from the old
# MAJOR.MINOR.PATCH scheme (`0.11.3` has a one-digit first part). The YY.N.P releases before it
# (26.1.0 to 26.3.9) read as January to March 2026, so they still sort below every new one.
is_release_version() {
  [[ "$1" =~ ^[0-9]{2}\.([1-9]|1[0-2])\.(0|[1-9][0-9]*)$ ]]
}

check_version() {
  is_release_version "$1" \
    || die "Not a version: $1 (expected YY.M.N, for example $(date -u +%y.%-m).0)."
}

# The newest release, from the tags. A release follows the latest tag, so the next version comes
# from there rather than from the version files, which a release writes to match it.
latest_version() {
  local tag
  tag=$(git tag -l 'v[0-9]*' --sort=-v:refname | head -n 1)
  [[ -n "$tag" ]] || die "This repo has no release tag to count from."
  echo "${tag#v}"
}

# Prints the version after the latest release: the next N in the same month, or N 0 in a new one.
# The month is UTC's, so a release cut here and one cut by the workflow count the same way.
next_version() {
  local current year month count
  current=$(latest_version)
  read -r year month <<< "$(date -u '+%y %-m')"
  # The newest tag from the old scheme has no month to count from.
  if ! is_release_version "$current"; then
    echo "$year.$month.0"
    return
  fi
  IFS=. read -r _ _ count <<< "$current"
  local was=$((10#${current%%.*} * 100 + $(cut -d. -f2 <<< "$current")))
  local now=$((10#$year * 100 + month))
  if ((was > now)); then
    die "The newest release, $current, is from a later month than this machine's clock ($year.$month)."
  elif ((was < now)); then
    echo "$year.$month.0"
  else
    echo "$year.$month.$((count + 1))"
  fi
}

# The version to release: `next`, or `bump`, counted from the latest tag, or the YY.M.N given. A
# leading `v` is accepted, as in the tags themselves.
resolve_version() {
  local version="${1:-}"
  [[ -n "$version" ]] || die "Usage: scripts/release.sh bump | YY.M.N"
  version="${version#v}"
  case "$version" in
    next | bump) next_version ;;
    *)
      check_version "$version"
      echo "$version"
      ;;
  esac
}

# Writes the version into the package.json files and the Tauri config, which the app shows and the
# installers use; the Rust crates read it from the Tauri config (crates/server/build.rs), so a release
# leaves Cargo.toml and Cargo.lock alone.
set_version() {
  local version file
  version="${1#v}"
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

# Writes and checks the release notes, sets the version, adds the notes to CHANGELOG.md, and
# commits, all on the current commit. Pushing is up to the caller.
commit_release() {
  local version="$1" tag="v$1"
  # Kept until the release is pushed, so an edit to it, or a second run, is used as it is.
  local notes=".release-notes/$tag.md"
  step "Release notes for $tag"
  scripts/changelog.sh draft "$tag" "$notes"
  echo
  cat "$notes"
  echo
  scripts/changelog.sh check "$notes" "$version"

  step "Committing $tag"
  set_version "$version"
  scripts/changelog.sh insert "$notes" "$version"
  git add CHANGELOG.md package.json apps/web/package.json apps/desktop/package.json \
    apps/desktop/src-tauri/tauri.conf.json
  git commit -m "Release $tag"
}

# commit_release, then the annotated tag on that commit.
tag_release() {
  commit_release "$1"
  git tag -a "v$1" -m "Patchwork $1"
}

cut_release() {
  local asked="${1:-bump}" version previous
  version=$(resolve_version "$asked")
  check_version "$version"
  previous=$(latest_version)
  [[ "$version" != "$previous" ]] || die "v$version is already the latest release; nothing to cut."
  local tag="v$version"
  echo "Cutting $tag, up from v$previous."

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

  # Commits not on origin yet go up on their own first: GitHub matches the Release workflow's paths
  # against every commit in a push, so one that changes a Cargo file would otherwise start a cache
  # build beside the tag's run.
  if [[ -n "$(git rev-list origin/main..HEAD)" ]]; then
    git push origin main
  fi
  tag_release "$version"
  git push origin main "$tag"
  rm -f ".release-notes/$tag.md"

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

  step "Checking $LATEST_JSON"
  local manifest published
  manifest=$(curl -fsSL "$LATEST_JSON") || die "Could not fetch $LATEST_JSON."
  published=$(jq -r .tag_name <<< "$manifest")
  [[ "$published" == "$tag" ]] || die "latest.json names $published, not $tag."
  echo "Installed copies on 0.10.10 or newer will update to $version. Downloads:"
  jq -r '.assets[] | "  \(.browser_download_url)"' <<< "$manifest"
}

# Builds with the Tauri CLI. Nothing is signed here: the release job signs the published files, so
# the signing key is never on a machine that runs the dependencies' build scripts.
tauri_build() {
  pnpm --filter patchwork-desktop exec tauri build --bundles "$@"
}

# Dies unless the argument names a platform, before anything is built.
check_platform() {
  case "$1" in
    linux | windows | macos) ;;
    *) die "Usage: scripts/release.sh ci-build linux|windows|macos" ;;
  esac
}

# Sets `files` to the files a platform's build publishes. Each is found by pattern, since Tauri puts
# the version in the name, and the newest match wins.
platform_files() {
  case "$1" in
    linux) files=($(ls -t target/release/bundle/appimage/*.AppImage 2> /dev/null | head -n 1 || true)) ;;
    windows) files=($(ls -t "target/$WINDOWS_TARGET/release/bundle/nsis/"*-setup.exe 2> /dev/null | head -n 1 || true)) ;;
    macos)
      # The disk image is for new installs; the updater downloads the app tarball.
      files=($(ls -t target/release/bundle/dmg/*.dmg 2> /dev/null | head -n 1 || true)
        target/release/bundle/macos/Patchwork.app.tar.gz)
      ;;
    *) die "Usage: scripts/release.sh ci-build linux|windows|macos" ;;
  esac
  (( ${#files[@]} )) || die "The $1 build has no files: nothing was built yet."
  [[ -f "${files[0]}" ]] || die "The $1 build has no file at ${files[0]}."
}

# A release run's first step, after a push to main passed the checks. When main carries a version
# with no tag yet, a release pull request was merged, and this run tags and publishes it; otherwise
# it updates the release pull request.
ci_plan() {
  local version
  version=$(node -p 'require("./package.json").version')
  if git rev-parse --quiet --verify "refs/tags/v$version" > /dev/null; then
    echo "v$version is released already, so this run updates the release pull request."
    version=""
  else
    echo "main carries v$version, which has no tag yet, so this run releases it."
  fi
  echo "version=$version" >> "${GITHUB_OUTPUT:-/dev/stdout}"
}

# Puts the next release on the release branch, one commit on top of the checked commit, and opens
# or updates its pull request, so it always shows what merging would release. A commit pushed while
# it is open joins it once its own checks pass. Merged with a merge commit, one that had not yet
# keeps its notes for the next release.
ci_pr() {
  local pr version body
  pr=$(gh pr list --head "$RELEASE_BRANCH" --base main --state open --json number -q '.[0].number // empty')
  if ! scripts/changelog.sh pending; then
    echo "No commit since $(latest_version) has notes for users, so there is nothing to release."
    if [[ -n "$pr" ]]; then
      gh pr close "$pr" --delete-branch --comment "Nothing for users is waiting any more."
    fi
    return
  fi
  version=$(next_version)
  commit_release "$version"
  git push --force origin "HEAD:refs/heads/$RELEASE_BRANCH"
  body="$(cat ".release-notes/v$version.md")

---

Merging this releases v$version: these notes and the version, on top of $(git rev-parse --short HEAD^), which passed the checks. Merge it with **Create a merge commit**, so commits pushed since keep their notes for the next release."
  if [[ -n "$pr" ]]; then
    gh pr edit "$pr" --title "Release v$version" --body "$body"
  else
    gh pr create --base main --head "$RELEASE_BRANCH" --title "Release v$version" --body "$body"
  fi
}

# Tags the commit that set VERSION: the release pull request's own commit when it was merged with
# a merge commit, else the one the merge made. A tag pushed with the workflow's token starts no
# second run, so this run builds it.
ci_tag() {
  local version="${1#v}" tag commit set
  check_version "$version"
  tag="v$version"
  ! git rev-parse --quiet --verify "refs/tags/$tag" > /dev/null || die "$tag already exists."
  commit=$(git log -1 --format=%H -G '"version": ' "v$(latest_version)..HEAD" -- package.json)
  [[ -n "$commit" ]] || die "No commit since v$(latest_version) changes the version."
  set=$(git show "$commit:package.json" | node -p 'JSON.parse(require("fs").readFileSync(0, "utf8")).version')
  [[ "$set" == "$version" ]] || die "The newest commit that changes the version sets $set, not $version."
  git tag -a "$tag" -m "Patchwork $version" "$commit"
  git push origin "refs/tags/$tag"
  echo "Tagged $tag on $(git log -1 --format='%h %s' "$commit")."
  echo "tag=$tag" >> "${GITHUB_OUTPUT:-/dev/stdout}"
}

ci_build() {
  local platform="${1:-}" files=() file version stable was i dir
  check_platform "$platform"
  # A tag pushed without `scripts/release.sh` still builds with its own version.
  if [[ -n "${RELEASE_TAG:-}" ]]; then set_version "${RELEASE_TAG#v}"; fi
  case "$platform" in
    # The AppImage bundles GStreamer, which WebKit plays video and audio with, so it carries the
    # runner's plugins too: the user's own, built for a newer GStreamer, crash the web process.
    linux)
      tauri_build appimage --config '{"bundle":{"linux":{"appimage":{"bundleMediaFramework":true}}}}'
      ;;
    # Built on Linux with cargo-xwin, which compiles it in half the time a Windows runner takes.
    windows) tauri_build nsis --runner cargo-xwin --target "$WINDOWS_TARGET" ;;
    macos)
      # The disk image is for new installs; the updater downloads Patchwork.app.tar.gz and unpacks
      # it with tar, expecting Patchwork.app at its top. COPYFILE_DISABLE keeps macOS's ._ files out.
      tauri_build app,dmg
      COPYFILE_DISABLE=1 tar -czf target/release/bundle/macos/Patchwork.app.tar.gz \
        -C target/release/bundle/macos Patchwork.app
      ;;
  esac
  platform_files "$platform"
  # Tauri names each bundle after its version. The version comes out of it here, before anything is
  # uploaded or signed, so every release publishes the same file names.
  version=$(jq -r .version apps/desktop/src-tauri/tauri.conf.json)
  for i in "${!files[@]}"; do
    dir=$(dirname "${files[$i]}")
    was=$(basename "${files[$i]}")
    stable=$(stable_bundle_name "${files[$i]}" "$version")
    [[ "$stable" != "$was" ]] || continue
    mv -f "${files[$i]}" "$dir/$stable"
    files[i]="$dir/$stable"
    echo "Named $was as $stable."
  done
  for file in "${files[@]}"; do
    [[ -f "$file" ]] || die "Expected $file after the build."
  done
  echo "Built: ${files[*]}"
  [[ -n "${RELEASE_TAG:-}" ]] || return 0

  # For a release, the files go in one folder for the job's artifact, which the release job checks
  # and publishes, and their list (name, SHA-256) goes to the step's `assets` output.
  local assets='[]' name
  mkdir -p release-files
  for file in "${files[@]}"; do
    name=$(basename "$file")
    cp "$file" "release-files/$name"
    assets=$(jq -c --arg name "$name" --arg digest "sha256:$(file_sha256 "$file")" '. + [{name: $name, digest: $digest}]' <<< "$assets")
  done
  echo "assets=$assets" >> "${GITHUB_OUTPUT:-/dev/stdout}"
}

file_sha256() {
  # macOS has shasum but not always sha256sum.
  (sha256sum "$1" 2> /dev/null || shasum -a 256 "$1") | cut -d' ' -f1
}

# The key the app checks updates against, as built into it. It lives with the release code.
app_signing_key() {
  sed -n 's/^const SIGNING_KEY: &str = "\(.*\)";$/\1/p' \
    apps/desktop/src-tauri/src/updates/release.rs
}

# The Tauri CLI version the lockfile pins, for signing without installing the project.
tauri_cli_version() {
  awk '/^      .@tauri-apps\/cli.:$/ { found = 1; next } found && /version:/ { print $2; exit }' pnpm-lock.yaml
}

ci_release() {
  local tag="${RELEASE_TAG:?Run ci-release from the Release workflow.}"
  local version="${tag#v}" key cli dir=release-check name digest asset assets='[]'
  local built="${LINUX:?} ${WINDOWS:?} ${MACOS:?}"
  [[ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]] || die "TAURI_SIGNING_PRIVATE_KEY is not set."
  key=$(app_signing_key)
  [[ -n "$key" ]] || die "Could not read SIGNING_KEY from apps/desktop/src-tauri/src/updates/release.rs."
  cli=$(tauri_cli_version)
  [[ -n "$cli" ]] || die "Could not read the @tauri-apps/cli version from pnpm-lock.yaml."

  # Each file came from its build job's artifact. What is signed and published must match the
  # SHA-256 its build job reported, and its signature must check out against the key built into the
  # app, as the updater will check it.
  while IFS= read -r asset; do
    name=$(jq -r .name <<< "$asset")
    digest=$(jq -r .digest <<< "$asset")
    [[ -f "$dir/$name" ]] || die "The build jobs' artifacts have no $name."
    [[ "sha256:$(file_sha256 "$dir/$name")" == "$digest" ]] \
      || die "$name does not match the file its build job built."
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
  # reads it.
  jq -n --arg tag "$tag" --arg repo "$REPO_URL" --rawfile body RELEASE_NOTES.md \
    --argjson built "$(jq -s 'add' <<< "$built")" --argjson signed "$assets" \
    '{tag_name: $tag, body: $body, assets: ($built | map(. as $file | {
      name,
      browser_download_url: "\($repo)/releases/download/\($tag)/\(.name)",
      digest,
      signature: ($signed[] | select(.name == $file.name) | .sig)
    }))}' > "$dir/latest.json"
  cat "$dir/latest.json"

  # GitHub serves the newest release's copy of an asset at /releases/latest/download/<name>, so the
  # updater, the README, and the website reach every release at the same links. The release is a
  # draft until every file is up, so it never becomes the latest with latest.json naming a file
  # that is not there yet. Each release keeps its own latest.json, which lets
  # `scripts/release.sh pull` make it the latest again.
  local files_to_upload=("$dir/latest.json")
  while IFS= read -r asset; do
    files_to_upload+=("$dir/$(jq -r .name <<< "$asset")")
  done < <(jq -c '.[]' <<< "$built")
  gh release create "$tag" --draft --title "Patchwork $tag" --notes-file RELEASE_NOTES.md \
    "${files_to_upload[@]}"
  gh release edit "$tag" --draft=false --latest > /dev/null
  echo "Published $tag and $LATEST_JSON"
}

# The tags latest.json withdraws once it names $previous again: those it withdrew already, and
# $tag. A tag that changed the database schema since $previous is left out, since $previous can't
# open a database it migrated, so copies on that tag keep it until a fix ships.
withdrawn_tags() {
  local manifest="$1" previous="$2" tag="$3" t
  for t in $(jq -r '.withdrawn // [] | .[]' <<< "$manifest") "$tag"; do
    if git diff --quiet "$previous" "$t" -- "$MIGRATIONS"; then
      echo "$t"
    else
      echo "$t changed the database since $previous, which can't open it, so copies on $t keep it until a fix ships." >&2
    fi
  done
}

# Withdraws a release that went wrong: latest.json goes back to the release before it, so copies
# that have not updated yet stop being offered it, and lists it as withdrawn, so copies already on
# it go back to the release before it at their next check (unless its database changes rule it out).
pull_release() {
  local version="${1#v}"
  check_version "$version"
  local tag="v$version" manifest previous="" candidate found=""
  manifest=$(curl -fsSL "$LATEST_JSON") || die "Could not fetch $LATEST_JSON."
  [[ "$(jq -r .tag_name <<< "$manifest")" == "$tag" ]] \
    || die "latest.json names $(jq -r .tag_name <<< "$manifest"), not $tag, so there is nothing to pull."
  git fetch --quiet --tags origin
  mkdir -p .release-notes
  for candidate in $(git tag -l 'v*' --sort=-v:refname); do
    if [[ -n "$found" ]] && curl -fsSL -o .release-notes/latest.json "$REPO_URL/releases/download/$candidate/latest.json" 2> /dev/null; then
      previous="$candidate"
      break
    fi
    if [[ "$candidate" == "$tag" ]]; then found=1; fi
  done
  [[ -n "$previous" ]] || die "No release before $tag has its own copy of latest.json to go back to."
  local withdrawn
  withdrawn=$(withdrawn_tags "$manifest" "$previous" "$tag")
  jq --arg tags "$withdrawn" '.withdrawn = ($tags | split("\n") | map(select(. != "")))' \
    .release-notes/latest.json > .release-notes/latest.json.new
  mv .release-notes/latest.json.new .release-notes/latest.json
  gh release upload "$previous" .release-notes/latest.json --clobber
  # $tag becomes a prerelease, so GitHub never counts it as the latest again.
  gh release edit "$previous" --latest > /dev/null
  gh release edit "$tag" --prerelease > /dev/null
  rm -f .release-notes/latest.json
  echo "latest.json names $previous again, so $tag is no longer offered as an update."
  if grep -qx "$tag" <<< "$withdrawn"; then
    echo "Copies already on $tag go back to $previous at their next update check."
  fi
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
  # The release's own latest.json, which the app reads while it is the latest.
  local manifest
  if ! manifest=$(curl -fsSL "$REPO_URL/releases/download/$tag/latest.json" 2> /dev/null); then
    echo "$tag has no latest.json of its own, so the app's update notes are unchanged."
  else
    jq --rawfile body "$notes" '.body = $body' <<< "$manifest" > .release-notes/latest.json
    gh release upload "$tag" .release-notes/latest.json --clobber
    echo "Updated the notes in $tag's latest.json."
    rm -f .release-notes/latest.json
  fi
  rm -f "$notes"
}

case "${1:-}" in
  bump) cut_release bump ;;
  next) next_version ;;
  version)
    requested=$(resolve_version "${2:-}") || exit 1
    set_version "$requested"
    ;;
  notes) republish_notes "${2:-}" ;;
  pull) pull_release "${2:-}" ;;
  watch) watch_release "${2:-}" ;;
  ci-plan) ci_plan ;;
  ci-pr) ci_pr ;;
  ci-tag) ci_tag "${2:-}" ;;
  ci-build) ci_build "${2:-}" ;;
  ci-release) ci_release ;;
  -h | --help) usage ;;
  "") usage >&2 && exit 1 ;;
  # A YY.M.N.
  *) cut_release "$1" ;;
esac
