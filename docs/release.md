# Releasing

How a version of Patchwork is cut, built and published.

## What a release is

A GitHub release tagged `vYY.M.N` (CalVer: the next N this month, or 0 in a new month, counted by
the workflow) with:

| File | What it is |
| --- | --- |
| `Patchwork_amd64.AppImage` | Linux, built by `scripts/build.sh` |
| `Patchwork_x64.exe` | Windows, the release build of the app |
| `Patchwork_aarch64.dmg` | macOS on Apple silicon, `Patchwork.app` signed ad hoc |
| `latest.json` | The release's tag and notes, and each file's address, SHA-256 and signature, which the app's updater reads |
| `latest.json.minisig` | The release key's signature over `latest.json` |
| `patchwork-remote-*` (five) | The helpers the app copies to SSH hosts: Linux and macOS on x86_64 and aarch64, and Windows on x86_64 |
| `patchwork-remote-*.minisig` | The release key's signature over each helper, which the app checks before copying it |

Its notes are the version's section of `CHANGELOG.md`, which the app also shows in Settings →
Updates.

## Cutting one

1. Push to `main`. The Release workflow (`.github/workflows/release.yml`) opens a pull request
   named "Release vYY.M.N", or brings the open one up to date. It is one commit on the `release`
   branch, on top of `main`: the next version in `Cargo.toml` and `Cargo.lock`, and the version's
   section at the top of `CHANGELOG.md`, `## [YY.M.N] - YYYY-MM-DD`, made from the `Changelog:`
   paragraphs of the commits since the last tag, grouped by Added, Changed, Fixed, Removed and
   Security. Every later push makes it again, so the pull request always holds everything since
   the last release. Pushes with nothing for users (`Changelog: none`) open none.
2. Merge the pull request when the release should go out.

The merge puts a version on `main` that has no tag, which is what the workflow takes as a release:
it tags that commit `vYY.M.N`, builds the three apps and the helpers and publishes the release. Setting the
version and writing the section by hand, and pushing that to `main`, releases the same way.
The entries are the commits' own words, so they are right when the commit is written, by the rules
in [Changelog style](changelog-style.md); to reword one, edit `CHANGELOG.md` on the `release`
branch before merging, and merge before the next push to `main` makes the branch again.
Once the release is published, the workflow points the website's download links at it and pushes
that change to `main` as "Point the website's downloads at vYY.M.N", so pull before the next push.

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
