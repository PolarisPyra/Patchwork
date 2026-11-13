# Releasing

How a version of Patchwork is cut, built and published.

## What a release is

A GitHub release tagged `YY.M.N` (CalVer: the next N this month, or 0 in a new month) with:

| File | What it is |
| --- | --- |
| `Patchwork_amd64.AppImage` | Linux, built by `scripts/build.sh` |
| `Patchwork_x64.exe` | Windows, the release build of the app |
| `Patchwork_aarch64.dmg` | macOS on Apple silicon, `Patchwork.app` signed ad hoc |
| `latest.json` | The release's tag and notes, and each file's address, SHA-256 and signature, which the app's updater reads |
| `latest.json.minisig` | The release key's signature over `latest.json` |
| `patchwork-remote-*` (five) | The helpers the app copies to SSH hosts: Linux and macOS on x86_64 and aarch64, and Windows on x86_64 |
| `patchwork-remote-*.minisig` | The release key's signature over each helper, which the app checks before copying it |

Its notes are written when it is published, from the changes since the previous tag. Each change
is a squash-merged PR, one commit whose subject ends in "(#N)", and its entry is the `Changelog:`
line in the PR body, or the commit message when the body has none. A commit pushed straight to
`main` uses its message. The notes have:

- A line with the version and how many changes and contributors it has.
- Up to seven highlights, each a bold name and one sentence. DeepSeek (`deepseek-v4-pro`, through
  the `DEEPSEEK_API_KEY` repository secret) picks them from the entries.
- Every entry, grouped by Added, Changed, Fixed, Removed and Security, with "by @login in #N" for
  its author and PR, folded under "Full changelog".

The app's update notes and Settings → Updates stop at "Full changelog". Without the secret, or
when the request fails, the notes are the full list alone and the run shows a warning. The prompt
and its rules are in the workflow's "Write the highlights" step.

## Cutting one

Tag the commit on `main` and push the tag:

```sh
git tag 26.11.66
git push origin 26.11.66
```

The Release workflow (`.github/workflows/release.yml`) then:

1. Writes the notes, sets the tag's version in `Cargo.toml` and `Cargo.lock`, and puts the notes
   at the top of `CHANGELOG.md` as `## [YY.M.N] - YYYY-MM-DD`, for the builds only.
2. Builds the three apps and the helpers at that version, signs them and publishes the release.
3. Commits the version and the notes to `main` as "Release YY.M.N", points the website's
   download links at the release, and announces it on Discord. Pull before the next push.

The entries are the PRs' own words, so they are right when the PR is written, by the rules in
[Changelog style](changelog-style.md), which the PR Changelog check enforces, and the highlights use only what the entries say. To
reword the notes after publishing, edit the release on GitHub and its section in `CHANGELOG.md`;
the update notes in `latest.json` are signed, so they keep the published words.

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
