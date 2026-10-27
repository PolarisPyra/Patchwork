# Releasing

Changelog entries and release notes come from the commit history with
[git-cliff](https://git-cliff.org/), configured in `cliff.toml`. Each commit is sorted into
a group by the start of its subject line, and commits that start with `Release`, `Bump`,
`Merge`, or `Chore` are left out. Words that don't match a group go under **Other**, so write
commit subjects with a leading verb, for example `Add ...`, `Fix ...`, or `Change ...`.

The repo's version always matches the latest release tag. The release script writes it into
every file that names the version, so you never bump version numbers by hand, and a local build
reports the version it was built from. The app shows it next to the project name in the top left
corner.

## Cut a release

Run the script from a clean `main` that is up to date with `origin/main`:

```sh
pnpm release 0.2.0
```

`pnpm release` runs `scripts/release.sh`. Pick the next version from the last tag
(`git describe --tags --abbrev=0`): raise the last number for fixes and small changes, the middle
one for larger features.

The script checks the version and the repo state, runs the typecheck, format check, Clippy, and
the Rust tests, and prints the release notes for the new version. After you confirm, it
writes the version with `scripts/set-version.sh` into `package.json`, `apps/web/package.json`,
`apps/desktop/package.json`, `apps/desktop/src-tauri/tauri.conf.json`,
`apps/desktop/src-tauri/Cargo.toml`, `apps/server/Cargo.toml`, and `Cargo.lock`. It then
regenerates `CHANGELOG.md` with a full rewrite (not `--prepend`, because the file already has an
Unreleased section), commits it all as `Release v0.2.0`, creates the annotated tag, and pushes
`main` and the tag. Nothing is written until you confirm.

Pushing the tag starts the **Release** workflow (`.github/workflows/release-appimage.yml`). Each
build job runs `scripts/set-version.sh` with the tag's version before it builds, so a tag pushed
without the script still builds with its own version. The workflow builds the Linux AppImage on
Ubuntu 22.04, the Windows installer, and the macOS disk image, runs `git cliff --latest` for the
release notes, and creates a GitHub release named `Patchwork v0.2.0` with all three attached.
Pushes to `main` do not build anything.

## Check the build without releasing

Run the workflow by hand from the Actions tab (**Run workflow**). It builds the files as they are,
so it uses the last released version, and creates no release.

## Rules

- Don't edit `CHANGELOG.md` by hand, since the next regeneration overwrites it.
- Don't change version numbers by hand; run `pnpm release` (or `scripts/set-version.sh` to fix a
  mismatch).
- Tags must start with `v` followed by a digit, such as `v0.2.0`, because that is the pattern
  git-cliff and the workflow both look for.
