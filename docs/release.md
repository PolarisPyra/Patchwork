# Releasing

How a version of Patchwork is cut, built and published.

## What a release is

A GitHub release tagged `vYY.M.N` (CalVer: the next N this month, or 0 in a new month) with:

| File | What it is |
| --- | --- |
| `Patchwork_amd64.AppImage` | Linux, built by `scripts/build.sh` |
| `Patchwork_x64.exe` | Windows, the release build of the app |
| `Patchwork_aarch64.dmg` | macOS on Apple silicon, `Patchwork.app` signed ad hoc |
| `latest.json` | The release's tag, notes and files, which Settings → Updates reads |

Its notes are the version's section of `CHANGELOG.md`, which the app also shows in Settings →
Updates.

## Cutting one

1. Add the version's section to the top of `CHANGELOG.md`, `## [YY.M.N] - YYYY-MM-DD`, made from
   the `Changelog:` paragraphs of the commits since the last tag (`git log vLAST..`), grouped by
   Added, Changed, Fixed, Removed and Security, by the rules in
   [Changelog style](changelog-style.md).
2. Set `[workspace.package] version` in the root `Cargo.toml` to `YY.M.N`, and run `cargo check`
   so `Cargo.lock` follows.
3. Commit both, tag the commit `vYY.M.N`, and push the branch and the tag.

The tag starts the Release workflow (`.github/workflows/release.yml`). It stops before building if
the tag and `Cargo.toml` disagree or the changelog has no section for the version, then builds
the three files and publishes the release.

Run the workflow by hand (Actions → Release → Run workflow) to build all three without publishing.

## Building the AppImage locally

```sh
scripts/build.sh
```

It writes `target/release/Patchwork_amd64.AppImage`. appimagetool comes from PATH, or is
downloaded to `target/` the first time.
