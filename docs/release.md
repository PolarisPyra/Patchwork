# Releasing

How a version of Patchwork is cut, built and published.

## What a release is

A GitHub release tagged `vYY.M.N` (CalVer: the next N this month, or 0 in a new month) with:

| File | What it is |
| --- | --- |
| `Patchwork_amd64.AppImage` | Linux, built by `scripts/build.sh` |
| `Patchwork_x64.exe` | Windows, the release build of the app |
| `Patchwork_aarch64.dmg` | macOS on Apple silicon, `Patchwork.app` signed ad hoc |
| `latest.json` | The release's tag and notes, and each file's address, SHA-256 and signature, which the app's updater reads |
| `latest.json.minisig` | The release key's signature over `latest.json` |

Its notes are the version's section of `CHANGELOG.md`, which the app also shows in Settings →
Updates.

## Cutting one

1. Add the version's section to the top of `CHANGELOG.md`, `## [YY.M.N] - YYYY-MM-DD`, made from
   the `Changelog:` paragraphs of the commits since the last tag (`git log vLAST..`), grouped by
   Added, Changed, Fixed, Removed and Security, by the rules in
   [Changelog style](changelog-style.md).
2. Set `[workspace.package] version` in the root `Cargo.toml` to `YY.M.N`, and run `cargo check`
   so `Cargo.lock` follows.
3. Commit both and run `git pull --rebase origin main`, since the last release's workflow pushed a
   commit of its own to `main`. Push `main`, then tag the release commit
   `vYY.M.N` and push the tag. Tagging after the branch is up keeps a tag from landing on a
   commit that is not on `main`.

The tag starts the Release workflow (`.github/workflows/release.yml`). It stops before building if
the tag and `Cargo.toml` disagree or the changelog has no section for the version, then builds
the three files and publishes the release.
Once it is published, the workflow points the website's download links at it and pushes that
change to `main` as "Point the website's downloads at vYY.M.N", so pull before the next release.

## Signing

The app installs a release only when its file matches the SHA-256 in `latest.json` and is signed
with the release key. The publish job signs each file with the `TAURI_SIGNING_PRIVATE_KEY` secret
(and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`), then checks the signature with `minisign` against
`SIGNING_KEY` in `crates/server/src/updates/release.rs`, the key the app carries, and stops
before publishing if it does not check out. Changing the key means changing both, and copies
already installed keep trusting the old one.

The signer and the website's deploy tool are each installed from a lockfile, `.github/signer` and
`.github/deploy`, in a step of their own before the key or the deploy token is in any step's
environment. `npm ci` takes only the packages a lockfile records, each checked against its hash.
To move either to a new version, change it in that folder's `package.json` and run
`npm install --package-lock-only --ignore-scripts` there. `scripts/build.sh` checks the
`appimagetool` it downloads against a SHA-256 written in the script, and
`.github/workflows/audit.yml` checks `Cargo.lock` against the RustSec advisories each week.

`latest.json` is signed too, as `latest.json.minisig` beside it, and the app reads a release only
when that signature checks out. Its version and its `withdrawn` list are then the release key's
word, so an older signed build cannot be passed off as the newest. Editing `latest.json` on a
published release, to withdraw a version, means signing it again with the release key and
replacing `latest.json.minisig` with the new signature.

Run the workflow by hand (Actions → Release → Run workflow) to build all three without publishing.

## Building the AppImage locally

```sh
scripts/build.sh
```

It writes `target/release/Patchwork_amd64.AppImage`. appimagetool comes from PATH, or is
downloaded to `target/` the first time.
