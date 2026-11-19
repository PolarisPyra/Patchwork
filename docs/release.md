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

Its notes are written when it is published, from the commits since the previous tag. Each change
is a squash-merged PR, one commit whose subject ends in "(#N)", or a commit pushed straight to
`main`. The notes have:

- A line with the version and how many commits and contributors it has. Contributors are the PR
  authors, without bots, plus the authors of direct pushes who have no PR in the range.
- Up to seven highlights, each a bold name and one sentence. Claude Haiku 5.5 (`claude-haiku-5-5`
  at low effort, through the `ANTHROPIC_API_KEY` repository secret) picks them from the commit
  subjects, each with its PR's size and the first 300 characters of its description, and may pick
  none when no change qualifies. A line after them counts the `fix` and `perf` commits they leave
  out, by the type at the start of the subject.
- GitHub's generated notes for the range, folded under "Full changelog".

The app's update notes and Settings → Updates show what comes before the fold, with a "Read more"
link to the release's page. Without the secret, or when the request fails, the notes are GitHub's
generated notes alone, the app shows their first ten entries, and the run shows a warning. The
prompt and its rules are in the workflow's "Write the highlights" step.

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
   The commit changes `site/index.html`, so the Site workflow deploys the website
   ([CI and deploys](ci.md)).

The highlights use only the commit subjects and PR descriptions, so they are right when the PR is
written. To reword the notes after publishing, edit the release on GitHub and its section in `CHANGELOG.md`;
the update notes in `latest.json` are signed, so they keep the published words.

## Signing

The app installs a release only when its file matches the SHA-256 in `latest.json` and is signed
with the release key. The publish job signs each file with the `TAURI_SIGNING_PRIVATE_KEY` secret
(and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`), then checks the signature with `minisign` against
`SIGNING_KEY` in `crates/core/src/updates/release.rs`, the key the app carries, and stops
before publishing if it does not check out. Changing the key means changing both, and copies
already installed keep trusting the old one.

The signer, in the Release workflow, and the website's deploy tool, in the Site workflow, are each
installed from a lockfile, `.github/signer` and `.github/deploy`, in a step of their own before
the key or the deploy token is in any step's environment. `npm ci` takes only the packages a lockfile records, each checked against its hash.
To move either to a new version, change it in that folder's `package.json` and run
`npm install --package-lock-only --ignore-scripts` there. `scripts/build.sh` checks the
`appimagetool` it downloads against a SHA-256 written in the script, and
`.github/workflows/audit.yml` checks `Cargo.lock` against the RustSec advisories each week.

`latest.json` is signed too, as `latest.json.minisig` beside it, and the app reads a release only
when that signature checks out. Its version and its `withdrawn` list are then the release key's
word, so an older signed build cannot be passed off as the newest. Editing `latest.json` on a
published release, to withdraw a version, means signing it again with the release key and
replacing `latest.json.minisig` with the new signature.

Only a tag releases, and its run is named "Release YY.M.N". Every other run of the workflow is a
"Release build check", which builds all three and publishes nothing: one started by hand (Actions →
Release → Run workflow), by a push to `main` that changes `Cargo.lock`, a `Cargo.toml` or the
workflow, or by the daily schedule. They save the Rust caches that tag runs restore. The release
commit changes only the workspace's own version, which the caches do not hold, so it builds nothing
([CI and deploys](ci.md)).

## Building the AppImage locally

```sh
scripts/build.sh
```

It writes `target/release/Patchwork_amd64.AppImage`. appimagetool comes from PATH, or is
downloaded to `target/` the first time.
