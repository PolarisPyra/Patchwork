# Agent instructions

Rules for coding agents working in the Rust code in this repo (`apps/server` and `apps/desktop/src-tauri`).

## Error handling

- **NEVER** use `.unwrap()` in production code paths. Use `.expect()` only for invariant violations, with a descriptive message.
- **MUST** use `Result<T, E>` for fallible operations.
- **MUST** use `thiserror` for defining error types and `anyhow` for application-level errors.
- **MUST** propagate errors with the `?` operator where appropriate.
- Provide meaningful error messages with context using `.context()` from `anyhow`.
- **MUST** use meaningful custom error types with `thiserror`.

## Types

- **MUST** keep types focused on a single responsibility.
- **MUST** derive common traits: `Debug`, `Clone`, `PartialEq` where appropriate.
- Use `#[derive(Default)]` when a sensible default exists.
- Prefer composition over inheritance-like patterns.
- Use the builder pattern for complex struct construction.
- Make fields private by default; provide accessor methods when needed.
- Use newtypes to distinguish semantically different values of the same underlying type.
- Prefer `Option<T>` over sentinel values.

## Functions

- **MUST** keep functions focused on a single responsibility.
- **MUST** prefer borrowing (`&T`, `&mut T`) over ownership when possible.
- Limit function parameters to 5 or fewer; use a config struct for more.
- Return early to reduce nesting.
- Use iterators and combinators over explicit loops where clearer.

## Type system

- **MUST** leverage Rust's type system to prevent bugs at compile time.

## Imports and dependencies

- **MUST** avoid wildcard imports (`use module::*`) except for preludes, test modules (`use super::*`), and prelude re-exports.
- **MUST** document dependencies in `Cargo.toml` with version constraints.
- Use `cargo` for dependency management.
- Organize imports in three groups: standard library, external crates, local modules.
- Use `rustfmt` to automate import formatting.

## Idioms and patterns

- **NEVER** use `unsafe` unless absolutely necessary; document safety invariants when used.
- **MUST** call `.clone()` explicitly on non-`Copy` types; avoid hidden clones in closures and iterators.
- **MUST** use pattern matching exhaustively; avoid catch-all `_` patterns when possible.
- **MUST** use the `format!` macro for string formatting.
- Use iterators and iterator adapters over manual loops.
- Use `enumerate()` instead of manual counter variables.
- Prefer `if let` and `while let` for single-pattern matching.

## Benchmarks

- **NEVER** run benchmarks in parallel, as the benchmarks will compete for resources and the results will be invalid.
- **NEVER** game the benchmarks. Do not manipulate the benchmarks themselves to satisfy any required performance constraints.
- **NEVER** run benchmarks with `target-cpu=native` or any other `RUSTFLAGS`.
- If benchmarking against another crate or library, ensure the benchmarks are apples-to-apples comparisons.
- Ensure benchmark tests are independent. If the tests are dependent due to a feature (e.g. caching), ensure the feature is disabled.

## Pushing and releasing

- **Pushing to `main`** (no tag) is a plain `git push origin main`. It builds nothing. Only push when the user asks.
- **Tagged builds** are releases. Cut one only when the user asks, with `pnpm release <version>` (for example `pnpm release 0.2.0`, which runs `scripts/release.sh`) from a clean `main` that matches `origin/main`. The script runs the checks, shows the release notes, and waits for confirmation before it writes the version into every package file with `scripts/set-version.sh`, commits `Release v<version>`, creates the annotated `v<version>` tag, and pushes `main` and the tag. The repo's version always matches the latest tag.
- Pushing the tag starts the **Release** workflow (`.github/workflows/release-appimage.yml`). It runs `scripts/set-version.sh` with the tag's version, builds the Linux, Windows, and macOS installers, and creates a GitHub release named `Patchwork v<version>`.
- To check the build without a release, run the workflow by hand from the Actions tab (**Run workflow**). It builds with the last released version and creates no release.
- Never edit `CHANGELOG.md` by hand, and never set version numbers by hand. Tags must start with `v` followed by a digit.
- **Version numbers** are `MAJOR.MINOR.PATCH` with no leading zeros and no suffixes (the release script only accepts `X.Y.Z`). Read the current version with `git describe --tags --abbrev=0`, then raise one part by one:
  - **Patch** for fixes and small changes: `0.4.11` → `0.4.12`.
  - **Minor** for larger features, resetting the patch: `0.4.11` → `0.5.0`.
  - **Major** only when the user asks for it.
- **Hotfixes** are ordinary patch releases. Fix the bug on `main`, then release the next patch number: a hotfix after `0.4.9` is `0.4.10`, not `0.4.91`. Each part is compared as a whole number, so `0.4.91` would sort above every release from `0.4.10` to `0.4.90` and break update and release ordering. If the user asks for a version like that, explain this and confirm before tagging.
- Never reuse, move, or delete a pushed tag. A broken release is followed by the next patch release.
- Full details are in [docs/releasing.md](docs/releasing.md).

## Release and build setup for a Tauri project

Use this to give a new pnpm + Cargo Tauri project the same layout, release, and build setup. Everything it needs is below, so nothing has to be copied from another repo.

### Repo layout

Every Tauri app uses this pnpm + Cargo monorepo layout, the one Patchwork uses. Run every command from the repo root.

```
.
├── Cargo.toml              # Cargo workspace: members = ["apps/server", "apps/desktop/src-tauri"]
├── package.json            # root package `@<name>/monorepo`; its scripts forward to the apps
├── pnpm-workspace.yaml     # packages: ["apps/*"]
├── scripts/                # release.sh, build.sh, and other repo scripts
├── packaging/              # <bin_name>.desktop
└── apps/
    ├── desktop/            # <desktop_pkg>: owns @tauri-apps/cli; scripts dev, build, tauri
    │   └── src-tauri/      # Tauri crate <bin_name>: window, plugins, and commands
    ├── web/                # <name>-web: Vite frontend; scripts dev, build, typecheck
    └── server/             # <name>-server: Rust library crate with the app's logic
```

- The frontend lives in `apps/web`, not inside `apps/desktop`. In `tauri.conf.json`, `frontendDist` is `../../web/dist`, `beforeDevCommand` is `pnpm --filter <name>-web dev`, and `beforeBuildCommand` is `pnpm --filter <name>-web build`.
- The app's logic lives in the `apps/server` library crate. The Tauri crate depends on it by path (`<name>-server = { path = "../../server" }`) and only exposes it as Tauri commands.
- Root scripts call the apps with `pnpm --filter`: `dev`, `dev:web`, `build`, `typecheck`, `tauri`, and `test` (`cargo test --workspace`). Run the app with `pnpm tauri dev`.
- A new package or crate goes under `apps/`. Add each new crate to `members` in the root `Cargo.toml`.

### Values to set

Set these before you write the files. The examples are placeholders.

| Value | Where it comes from | Example |
| --- | --- | --- |
| `app_name` | `productName` in `tauri.conf.json` | `My App` |
| `app_id` | `identifier` in `tauri.conf.json` (reverse-DNS) | `com.example.myapp` |
| `bin_name` | `name` in the desktop crate's `Cargo.toml` | `myapp` |
| `desktop_pkg` | `name` in the desktop package's `package.json` | `my-app-desktop` |
| `tauri_dir` | Folder with `tauri.conf.json` and `Cargo.toml` | `apps/desktop/src-tauri` |

The repo must follow the layout above. The Tauri crate must have `icons/` with `32x32.png`, `128x128.png`, `128x128@2x.png`, and `icon.png`, which `pnpm tauri icon` generates.

### Files

- `scripts/release.sh`: runs the checks, shows the release notes, and on confirmation commits `CHANGELOG.md`, tags, and pushes.
- `scripts/build.sh`: `appimage` builds a fixed-name AppImage. `install` builds the release binary and installs it with icons and a launcher under `$PREFIX` (default `~/.local`).
- `cliff.toml`: git-cliff config for `CHANGELOG.md` and the release notes.
- `.github/workflows/release.yml`: on a `v*` tag, builds each platform and creates the GitHub release.
- `packaging/<bin_name>.desktop`: launcher template. `@EXEC@` is replaced with the binary path at install time.

Make the scripts executable with `chmod +x scripts/*.sh`.

### scripts/release.sh

```bash
#!/usr/bin/env bash
# Cuts a release: checks the repo and the build, regenerates CHANGELOG.md with git-cliff, commits
# it, tags the version, and pushes both. Pushing the tag starts the release workflow.
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

# Replace these with the project's own checks. Keep them to commands that already exist.
echo "Checking the web app and the Rust crates..."
pnpm typecheck
pnpm format:check
cargo clippy --workspace --all-targets
cargo test --workspace

echo
echo "Release notes for $tag:"
git cliff --config cliff.toml --unreleased --tag "$tag"
echo

read -r -p "Commit CHANGELOG.md, tag $tag, and push main and the tag to origin? [y/N] " answer
if [[ "$answer" != y && "$answer" != Y ]]; then
  die "Release cancelled. Nothing was changed."
fi

git cliff --config cliff.toml --tag "$tag" --output CHANGELOG.md
git add CHANGELOG.md
git commit -m "Release $tag"
git tag -a "$tag" -m "$version"
git push origin main "$tag"

echo "Pushed $tag. The release workflow is building the installers and will create the"
echo "GitHub release. Follow it in the Actions tab."
```

### scripts/build.sh

```bash
#!/usr/bin/env bash
# Builds the app. `appimage` writes <bin_name>_amd64.AppImage. `install` builds the release binary
# and installs it under $PREFIX (default ~/.local) with its icons and a launcher. The launcher has
# the app id, so it leaves the launcher of an AppImage you already run untouched.
#
# Usage: scripts/build.sh appimage | install
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# Set these for the project. See "Values to set" in AGENTS.md.
app_name="My App"
app_id="com.example.myapp"
bin_name="myapp"
desktop_pkg="my-app-desktop"
tauri_dir="apps/desktop/src-tauri"

die() {
  echo "$*" >&2
  exit 1
}

usage() {
  cat <<EOF
Usage: scripts/build.sh appimage | install

  appimage  Build target/release/bundle/appimage/${bin_name}_amd64.AppImage
  install   Build the release binary and install it under \$PREFIX (default ~/.local):
            bin/${bin_name}, the icons, and share/applications/${app_id}.desktop
EOF
}

version="$(jq -r .version "$tauri_dir/tauri.conf.json")"

require_tools() {
  local tool
  for tool in pnpm jq; do
    command -v "$tool" > /dev/null || die "$tool is required."
  done
}

build_appimage() {
  require_tools
  pnpm --filter "$desktop_pkg" exec tauri build --bundles appimage
  # Tauri names the file after the product and version. Take the newest one and rename it, so the
  # name stays the same across releases.
  local built out="target/release/bundle/appimage/${bin_name}_amd64.AppImage"
  built="$(ls -t target/release/bundle/appimage/*.AppImage 2> /dev/null | head -n 1 || true)"
  [[ -n "$built" ]] || die "No AppImage found after the build."
  mv -f "$built" "$out"
  echo "AppImage: $repo_root/$out"
}

stop_running_instances() {
  local dev_bin="$repo_root/target/debug/$bin_name"
  local pids=() pid exe
  # pgrep -x matches up to 15 characters of the process name on Linux, which is enough for most names.
  for pid in $(pgrep -x "$bin_name" || true); do
    exe="$(readlink "/proc/$pid/exe" 2>/dev/null || true)"
    # The dev session runs the debug build from this repo. Leave it running.
    [[ "$exe" == "$dev_bin" ]] && continue
    pids+=("$pid")
  done
  (( ${#pids[@]} )) || return 0

  echo "Stopping running $app_name: ${pids[*]}"
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
  local bin_path="$prefix/bin/$bin_name"
  local apps_dir="$prefix/share/applications"
  local icons_dir="$prefix/share/icons/hicolor"
  local launcher="$apps_dir/$app_id.desktop"

  stop_running_instances
  pnpm --filter "$desktop_pkg" exec tauri build --no-bundle
  [[ -x "target/release/$bin_name" ]] || die "Expected target/release/$bin_name after the build."

  install -Dm755 "target/release/$bin_name" "$bin_path"
  echo "Binary: $bin_path"

  local pair
  for pair in 32x32.png:32x32 128x128.png:128x128 128x128@2x.png:256x256 icon.png:512x512; do
    install -Dm644 "$tauri_dir/icons/${pair%%:*}" \
      "$icons_dir/${pair##*:}/apps/$app_id.png"
  done
  echo "Icons: $icons_dir"

  # The launcher's Exec is the absolute binary path, so it goes stale when the prefix or the
  # template changes. Compare the rendered launcher with the installed copy before rewriting it.
  local rendered
  rendered="$(sed "s|@EXEC@|$bin_path|" "packaging/$bin_name.desktop")"
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
    *) echo "Note: $prefix/bin is not on PATH, so the '$bin_name' command won't resolve." ;;
  esac
  echo "Installed $app_name $version. Open it from the application menu."
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
```

### packaging/<bin_name>.desktop

```ini
[Desktop Entry]
Type=Application
Name=My App
Comment=One-line description of the app
Exec=@EXEC@
Icon=com.example.myapp
Terminal=false
Categories=Development;
StartupWMClass=myapp
```

Set `Icon` to `app_id` and `StartupWMClass` to `bin_name`.

### cliff.toml

```toml
# git-cliff configuration for CHANGELOG.md and the release notes.

[changelog]
header = """
# Changelog

All notable changes to My App are documented here.
This file is generated by git-cliff from the commit history.

"""
body = """
{% if version %}\
    ## [{{ version | trim_start_matches(pat="v") }}] - {{ timestamp | date(format="%Y-%m-%d") }}
{% else %}\
    ## [Unreleased]
{% endif %}\
{% for group, commits in commits | group_by(attribute="group") %}
    ### {{ group | upper_first }}
    {% for commit in commits %}
        - {{ commit.message | upper_first | split(pat="\n") | first }}\
    {% endfor %}
{% endfor %}\n
"""
footer = ""
trim = true

[git]
conventional_commits = false
filter_unconventional = false
split_commits = false
commit_parsers = [
    { message = "^(Fix|Fixed|Repair|Stop|Prevent|Correct)", group = "Fixed" },
    { message = "^(Add|Show|Support|Allow|Introduce|Let|Remember|Reopen|Animate|Draw|Track|List|Render|Enable|Create)", group = "Added" },
    { message = "^(Remove|Drop|Delete|Deprecate)", group = "Removed" },
    { message = "^(Update|Change|Polish|Tighten|Refactor|Rename|Move|Align|Make|Improve|Describe|Sidebar|Choice|Use|Replace)", group = "Changed" },
    { message = "^(Release|Bump|Merge|Chore)", skip = true },
    { message = ".*", group = "Other" },
]
protect_breaking_commits = false
filter_commits = false
tag_pattern = "v[0-9].*"
skip_tags = ""
ignore_tags = ""
topo_order = false
topo_order_commits = true
sort_commits = "oldest"
```

### .github/workflows/release.yml

Runs on a `v*` tag, or by hand from the Actions tab to test the build. The release job only runs for tags, so a manual run builds the installers and creates no release.

```yaml
name: Release

on:
  push:
    tags:
      - "v*"
  workflow_dispatch:

permissions:
  contents: write

jobs:
  appimage:
    # Build on the oldest Ubuntu runner still available so the AppImage links against an older glibc.
    runs-on: ubuntu-22.04
    env:
      # linuxdeploy is itself an AppImage; this lets it run without FUSE.
      APPIMAGE_EXTRACT_AND_RUN: 1
    steps:
      - uses: actions/checkout@v4

      - name: Install Tauri Linux dependencies
        uses: awalsh128/cache-apt-pkgs-action@v1
        with:
          packages: >-
            build-essential file libayatana-appindicator3-dev libfuse2 librsvg2-dev libssl-dev
            libwebkit2gtk-4.1-dev libxdo-dev patchelf
          version: 1

      - uses: dtolnay/rust-toolchain@stable

      - uses: Swatinem/rust-cache@v2

      - uses: pnpm/action-setup@v4
        with:
          version: 10

      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: pnpm

      - name: Install JavaScript dependencies
        run: pnpm install --frozen-lockfile

      - name: Set version from tag
        # The tag is the source of truth. The repo keeps a placeholder version.
        if: startsWith(github.ref, 'refs/tags/v')
        run: |
          version="${GITHUB_REF_NAME#v}"
          for file in package.json apps/desktop/package.json apps/desktop/src-tauri/tauri.conf.json; do
            jq --arg v "$version" '.version = $v' "$file" > "$file.tmp" && mv "$file.tmp" "$file"
          done
          for file in apps/desktop/src-tauri/Cargo.toml; do
            perl -0pi -e "s/^version = .*/version = \"$version\"/m" "$file"
          done

      - name: Build AppImage
        run: scripts/build.sh appimage

      - name: Find AppImage
        id: find
        run: echo "path=$(ls target/release/bundle/appimage/*.AppImage | head -n 1)" >> "$GITHUB_OUTPUT"

      - uses: actions/upload-artifact@v4
        with:
          name: app-appimage
          path: ${{ steps.find.outputs.path }}

  windows:
    runs-on: windows-latest
    defaults:
      run:
        shell: bash

    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable

      - uses: Swatinem/rust-cache@v2

      - uses: pnpm/action-setup@v4
        with:
          version: 10

      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: pnpm

      - name: Install JavaScript dependencies
        run: pnpm install --frozen-lockfile

      - name: Set version from tag
        if: startsWith(github.ref, 'refs/tags/v')
        run: |
          version="${GITHUB_REF_NAME#v}"
          for file in package.json apps/desktop/package.json apps/desktop/src-tauri/tauri.conf.json; do
            jq --arg v "$version" '.version = $v' "$file" > "$file.tmp" && mv "$file.tmp" "$file"
          done
          for file in apps/desktop/src-tauri/Cargo.toml; do
            perl -0pi -e "s/^version = .*/version = \"$version\"/m" "$file"
          done

      - name: Build installer
        run: pnpm --filter my-app-desktop exec tauri build --bundles nsis

      - name: Find installer
        id: find
        run: echo "path=$(ls target/release/bundle/nsis/*-setup.exe | head -n 1)" >> "$GITHUB_OUTPUT"

      - uses: actions/upload-artifact@v4
        with:
          name: app-windows
          path: ${{ steps.find.outputs.path }}

  macos:
    # macos-14 runs on Apple silicon, so the build is native arm64.
    runs-on: macos-14

    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable

      - uses: Swatinem/rust-cache@v2

      - uses: pnpm/action-setup@v4
        with:
          version: 10

      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: pnpm

      - name: Install JavaScript dependencies
        run: pnpm install --frozen-lockfile

      - name: Set version from tag
        if: startsWith(github.ref, 'refs/tags/v')
        run: |
          version="${GITHUB_REF_NAME#v}"
          for file in package.json apps/desktop/package.json apps/desktop/src-tauri/tauri.conf.json; do
            jq --arg v "$version" '.version = $v' "$file" > "$file.tmp" && mv "$file.tmp" "$file"
          done
          for file in apps/desktop/src-tauri/Cargo.toml; do
            perl -0pi -e "s/^version = .*/version = \"$version\"/m" "$file"
          done

      - name: Build disk image
        run: pnpm --filter my-app-desktop exec tauri build --bundles dmg

      - name: Find disk image
        id: find
        run: echo "path=$(ls target/release/bundle/dmg/*.dmg | head -n 1)" >> "$GITHUB_OUTPUT"

      - uses: actions/upload-artifact@v4
        with:
          name: app-macos-arm64
          path: ${{ steps.find.outputs.path }}

  release:
    needs: [appimage, windows, macos]
    if: startsWith(github.ref, 'refs/tags/v')
    runs-on: ubuntu-latest

    steps:
      - uses: actions/checkout@v4
        with:
          # git-cliff needs the full history and tags to build the release notes.
          fetch-depth: 0

      - uses: actions/download-artifact@v4
        with:
          path: dist
          merge-multiple: true

      - name: Generate release notes
        uses: orhun/git-cliff-action@v4
        with:
          args: --latest --strip header --output RELEASE_NOTES.md

      - name: Create GitHub release
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          gh release create "$GITHUB_REF_NAME" dist/* \
            --title "My App $GITHUB_REF_NAME" \
            --notes-file RELEASE_NOTES.md
```

In the workflow, replace the `apps/desktop` paths and `my-app-desktop` with the project's `tauri_dir` and `desktop_pkg`. Drop any platform job the project doesn't ship. Add the `Cargo.toml` of each crate that the app ships to the version loops.

### Rules to keep

- The tag sets the version. Keep the placeholder version (`0.1.0`) in the repo, and never set version numbers by hand.
- Never edit `CHANGELOG.md` by hand. `release.sh` regenerates it.
- Tags must start with `v` followed by a digit, such as `v0.2.0`.
- Commit subjects start with a verb such as `Add`, `Fix`, or `Change`. git-cliff sorts commits by that word, and it skips `Release`, `Bump`, `Merge`, and `Chore` commits.
- Run `release.sh` only when the user asks for a release. It pushes `main` and the tag. To test the build without a release, run the workflow by hand with **Run workflow**.
- Install `git-cliff` on the machine that runs `release.sh` (for example `cargo install git-cliff --locked`). Also install `jq`.
