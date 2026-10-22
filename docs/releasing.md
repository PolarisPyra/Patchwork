# Releasing

Changelog entries and release notes come from the commit history with
[git-cliff](https://git-cliff.org/), configured in `cliff.toml`. Each commit is sorted into
a group by the start of its subject line, and commits that start with `Release`, `Bump`,
`Merge`, or `Chore` are left out. Words that don't match a group go under **Other**, so write
commit subjects with a leading verb, for example `Add ...`, `Fix ...`, or `Change ...`.

The tag sets the app version, so you never bump version numbers by hand. The version in the
repo stays at `0.1.0` as a placeholder, which means a local build reports that version.

## Cut a release

Run the script from a clean `main` that is up to date with `origin/main`:

```sh
scripts/release.sh 0.2.0
```

The script checks the version and the repo state, runs the typecheck, format check, Clippy, and
the Rust tests, and prints the release notes for the new version. After you confirm, it
regenerates `CHANGELOG.md` with a full rewrite (not `--prepend`, because the file already has an
Unreleased section), commits it as `Release v0.2.0`, creates the annotated tag, and pushes `main`
and the tag. Nothing is written until you confirm.

Pushing the tag starts the **Release AppImage** workflow (`.github/workflows/release-appimage.yml`).
Before it builds, the workflow writes the tag's version (`0.2.0`) into `package.json`,
`apps/desktop/package.json`, `apps/desktop/src-tauri/tauri.conf.json`,
`apps/desktop/src-tauri/Cargo.toml`, and `apps/server/Cargo.toml`. It then builds the AppImage on
Ubuntu 22.04, runs `git cliff --latest` for the release notes, and creates a GitHub release named
`Patchwork v0.2.0` with the AppImage attached. Pushes to `main` do not build anything.

## Check the build without releasing

Run the workflow by hand from the Actions tab (**Run workflow**). It builds the files as they are,
so it uses the placeholder version, and creates no release.

## Rules

- Don't edit `CHANGELOG.md` by hand, since the next regeneration overwrites it.
- Tags must start with `v` followed by a digit, such as `v0.2.0`, because that is the pattern
  git-cliff and the workflow both look for.
