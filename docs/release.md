# Releasing

Everything about shipping Patchwork: versions, the changelog, the release script, the GitHub
Actions workflow, the update server on Bunny, signing, and what to do when a release goes wrong.
For local builds, see [Development](development.md).

## Overview

```
pnpm release 0.2.0                      your machine: checks, notes, version, CHANGELOG.md, commit, tag, push
        │
        ▼
Release workflow (GitHub Actions)       on the v0.2.0 tag
  ├─ appimage  ─┐
  ├─ windows   ─┼─ build (no secrets), upload → updates.patchworkeditor.com/v0.2.0/<file>
  ├─ macos     ─┘
  └─ release      fetch back, check, sign, check signature → latest.json, GitHub release with notes
        │
        ▼
Installed copies (0.10.10 and newer)    read latest.json, download, check SHA-256 and signature, install
```

The repo is private. The installers are not attached to GitHub releases; they live only on the
update server.

These files do all of it:

| File | What it does |
| --- | --- |
| `scripts/release.sh` | Cuts the release on your machine, and does the build and publish steps in CI |
| `.github/workflows/release.yml` | Builds each platform with `scripts/release.sh`, and publishes a tag |
| `.github/workflows/ci.yml` | Checks every pull request, and runs by hand |
| `.github/actions/setup` | The runner setup both workflows share: Linux packages, Rust, pnpm, Node |
| `scripts/changelog.sh` | Checks commit messages, writes each release's notes from them, and reads and writes `CHANGELOG.md` |
| `docs/changelog-style.md` | The rules for changelog entries |

`scripts/release.sh --help` lists its commands.

## Versions

Versions are `MAJOR.MINOR.PATCH` with no leading zeros and no suffixes; the script only accepts
`X.Y.Z`. Read the current one with `git describe --tags --abbrev=0`, then raise one part by one:

- **Patch** by default, for every release: `0.8.0` → `0.8.1`.
- **Minor** for a major update, resetting the patch: `0.8.1` → `0.9.0`.
- **Major** only when asked for.

Hotfixes are ordinary patch releases: a hotfix after `0.4.9` is `0.4.10`, not `0.4.91`. Each part
is compared as a whole number, so `0.4.91` would sort above every release from `0.4.10` to `0.4.90`
and break update ordering.

The repo's version always matches the latest tag. The script writes it into `package.json`,
`apps/web/package.json`, `apps/desktop/package.json`, and `apps/desktop/src-tauri/tauri.conf.json`.
The Rust crates read it from `tauri.conf.json` when they build (`apps/server/build.rs`); their own
versions stay at `0.0.0`, so a release never changes a Cargo file. The app shows it next to the
project name in the top left corner. To fix a mismatch, run
`scripts/release.sh version X.Y.Z`.

## Changelog

`CHANGELOG.md` is for users. The app shows the notes in **Settings → Updates**: those of the
update it is installing, from `latest.json`, and those of the running version, as built in. Each
GitHub release has the same text. It follows [Keep a Changelog](https://keepachangelog.com) with
the groups Added, Changed, Fixed, Removed, and Security, and the rules in
[changelog-style.md](changelog-style.md).

Notes are written in two steps.

**1. At commit.** Every commit that changes the app has a paragraph for users in its message,
starting with its group, or `Changelog: none` when users can't notice the change:

```
Use the custom spinner everywhere

Changelog: Changed: Every loading indicator now uses the steady spinner from the thread list: plan steps,
background tasks, the tasks badge, steps in a reply, plans loading, and the setup screens for
agents and imports. The old one rotated and looked like it shook.
```

The `commit-msg` hook in `scripts/hooks/` turns down a commit that changes the app without one,
one without a group, and one that breaks the style rules, so the author fixes it while the change is fresh. A commit
that only touches `.github/`, `scripts/`, `docs/`, or `*.md` is internal and needs none.
`pnpm install` turns the hook on (`git config core.hooksPath scripts/hooks`).

No hook stops a push. Pushing is the user's call, and only their asking for it is what pushes:
an agent commits and says the work is unpushed. `scripts/release.sh` pushes the release and its tag
in one step, because a release was asked for.

**2. At release.** `scripts/release.sh` writes the release's section from the `Changelog:`
paragraphs of the app commits since the last tag, each under its group, each entry once. A
paragraph from before groups were required goes under Changed. A release with only internal
changes says so in one line. The notes are exactly what the commits say, so write a feature's
follow-up commits' paragraphs to read as one change, or as `Changelog: none` for a fix to a bug
that never shipped.

A tag whose build failed never reached users, yet its section is in `CHANGELOG.md`. The next
release merges every such section, the ones above the newest published release, into its own and
drops them, so its notes cover everything since the last version users have. The newest published
release is the newest GitHub release, which the Release workflow creates only once every file is up.

The notes wait in `.release-notes/<tag>.md` (ignored by git) until the release is pushed. To change
them before releasing, edit that file and run the release again; it is used as it is, once it
passes the style checks.

**Fixing notes after a release.** `CHANGELOG.md` is the source. Edit the release's section, commit
it, and run `scripts/release.sh notes 0.2.0` to publish it to the GitHub release, and to
`latest.json` when it is the latest release and `BUNNY_STORAGE_PASSWORD` and `BUNNY_API_KEY` are
set. Copies already installed show the fix once they update past it.

## Cut a release

From a clean `main` that is up to date with `origin/main`:

```sh
pnpm release 0.2.0
```

It needs `git`, `pnpm`, `cargo`, `gh` (signed in), `jq`, and `curl`. The script:

1. Checks the version, that `main` is clean and up to date, that the tag is new, and that the repo
   has the four secrets the workflow needs (see [Secrets](#secrets)).
2. Runs the CI workflow's checks, `pnpm check` and `cargo test --workspace`, and stops if they
   change a file, such as wire types that were not regenerated.
3. Writes the release notes (see [Changelog](#changelog)), checks them, and prints them.
4. Pushes commits not on `origin` yet, writes the version, adds the notes to the top of
   `CHANGELOG.md`, commits both as `Release v0.2.0`, creates the annotated tag, and pushes `main` and
   the tag. It asks no questions.
5. Prints the link to the Release workflow's run and ends. The build carries on on GitHub.

To follow the build, run `scripts/release.sh watch 0.2.0`. It waits for the run to end, stops with
the failed jobs' errors if it fails, then checks that `latest.json` names the new version and
prints the download links. Ctrl-C stops only the watching, not the build.

## The CI workflow

`.github/workflows/ci.yml` checks every pull request, and runs when started by hand, on the
`ubuntu-22.04` runner the AppImage is built on: it builds the web app, runs `pnpm check` and
`cargo test --workspace`, and fails if the generated wire types differ from the committed ones.
These are the checks `scripts/release.sh` runs before a release. Pushes to `main` start no run, so a
plain push costs no Actions minutes. Its Rust cache is saved only from `main`. A newer run on the
same branch cancels an older one.

## The Release workflow

`.github/workflows/release.yml` runs on a `v*` tag, and when started by hand (see
[Caches](#caches)). Pushes to `main` build nothing.

| Job | Runner | Commands | Publishes |
| --- | --- | --- | --- |
| appimage | `ubuntu-22.04` (older glibc, so the AppImage runs on newer distributions) | `ci-build linux`, `ci-upload linux` | `Patchwork_amd64.AppImage` |
| windows | `windows-2025`, building on a Dev Drive | `ci-build windows`, `ci-upload windows` | `Patchwork_<version>_x64-setup.exe` |
| macos | `macos-14` (Apple silicon) | `ci-build macos`, `ci-upload macos` | `Patchwork_<version>_aarch64.dmg`, `Patchwork.app.tar.gz` |
| release | `ubuntu-latest`, after the three above | `ci-release` | `latest.json`, the GitHub release |

Each build job sets the tag's version first, so a tag pushed without the script still builds with
its own version. It builds with no secrets in its environment, then a separate step uploads the
files to `updates.patchworkeditor.com/<tag>/` and passes each file's name and SHA-256 to the release
job as a job output.

The release job is the only one with the signing key or write access to the repo, and it runs none
of the project's dependencies. For each file, it downloads it back from the update server, checks
it matches the SHA-256 its build job reported, signs it with the Tauri CLI version `pnpm-lock.yaml`
pins, and checks the signature with `minisign` against the key built into the app (`SIGNING_KEY` in
`updates.rs`), the way the updater will. Any mismatch stops the release before `latest.json`
changes, so installed copies are never offered a file that won't install. It then uploads
`latest.json` to `<tag>/latest.json` and to `latest.json` (last, so it never names a file that
isn't there yet), purges Bunny's cached copy, and creates a GitHub release named `Patchwork v0.2.0`.
Both carry the tag's section of `CHANGELOG.md`.

Every action is pinned to a commit, with the version it was in a comment, so a moved tag upstream
can't change what runs. To update one, look up the commit of its new release and replace both.

To check the build without releasing, run the workflow by hand from the Actions tab
(**Run workflow**). It builds all three platforms and publishes nothing, even when run on a tag:
only a pushed tag uploads files or creates a release.

### Caches

Most of each build is compiling the Rust dependencies. A tag's run can restore caches saved on
`main`, but not ones saved by another tag, so the dependency caches are saved only on `main`:

- A run by hand on `main` builds Linux and Windows without publishing anything, and saves their
  caches. Run it after a change to a Cargo file, before the next tag. Pushes to `main` start no run.
- Tag runs restore those caches and save none.
- The Rust toolchain is pinned (`RUST_TOOLCHAIN` in the workflow), because a cache is only reused by
  the same compiler. Raising it rebuilds the caches on that push.
- macOS has no cache. Its build is short, and its minutes count ten times.

### Private-repo limits

A private repo gets limited Actions resources on the free plan, so the workflow stays inside them:

- **No artifacts.** Builds upload straight to Bunny. Artifact storage is counted across the
  billing month, so deleted artifacts keep counting until it resets.
- **Caches only from `main`.** See [Caches](#caches). The repo's cache storage is 10 GB; the Linux
  and Windows dependency caches fit in it, and older ones are evicted on their own.
- **Minutes.** The free plan has 2,000 a month. Windows minutes count twice and macOS minutes ten
  times. Without caches, v0.10.16 took about 13 minutes on Linux, 17 on Windows, and 2 on macOS,
  so about 70 counted minutes. A change to the Rust dependencies costs one cache build of Linux and
  Windows. Each job has a timeout (macOS 30 minutes, Linux and Windows 45, release 10). A second
  run for the same ref waits for the first, so a release is never stopped halfway through
  publishing.

Check usage at github.com → Settings → Billing and plans.

## Update server

Release files are served from `https://updates.patchworkeditor.com`:

```
/latest.json                              the newest release; the only file that changes
/v0.2.0/Patchwork_amd64.AppImage          one folder per tag, never overwritten
/v0.2.0/Patchwork_0.2.0_x64-setup.exe
/v0.2.0/Patchwork_0.2.0_aarch64.dmg
/v0.2.0/Patchwork.app.tar.gz
```

`latest.json` has the shape of GitHub's release API, plus each file's signature. `body` is the
release's section of `CHANGELOG.md`:

```json
{
  "tag_name": "v0.2.0",
  "body": "## [0.2.0] - 2026-01-02\n\n### Added\n\n- …",
  "assets": [
    {
      "name": "Patchwork_amd64.AppImage",
      "browser_download_url": "https://updates.patchworkeditor.com/v0.2.0/Patchwork_amd64.AppImage",
      "digest": "sha256:…",
      "signature": "untrusted comment: signature from tauri secret key\n…"
    }
  ]
}
```

### How copies update

`apps/desktop/src-tauri/src/updates.rs` checks `latest.json` 30 seconds after launch and then as
often as **Settings → Updates** says. When the tag is newer than the running version, it picks its
platform's file (the AppImage, the `-setup.exe`, or `Patchwork.app.tar.gz`), downloads it, and
installs it only if both its SHA-256 and its minisign signature check out. The signature is checked
against the public key built into the app, so a changed file on the server, or a changed
`latest.json`, can't install anything the signing key didn't sign. Dev builds and copies not
installed from a release report new versions but don't install them.

Patchwork 0.10.9 and older checked GitHub's API instead, which a private repo blocks. Those copies
need a one-time manual download of 0.10.10 or newer.

### Bunny setup

| Piece | Value |
| --- | --- |
| Storage zone | `patchwork-releases`, New York (`https://ny.storage.bunnycdn.com/patchwork-releases`) |
| Pull zone | `patchwork-releases` (`patchwork-releases.b-cdn.net`), origin: the storage zone |
| Custom hostname | `updates.patchworkeditor.com`, free SSL, Force SSL on |
| DNS (Cloudflare) | `CNAME updates → patchwork-releases.b-cdn.net`, **DNS only** (grey cloud) |

Keep the DNS record unproxied. Bunny issues and renews the certificate itself, and needs to see the
traffic to do it. Proxying through Cloudflare would also put a second cache in front of
`latest.json` that the release job doesn't purge.

### Secrets

The workflow reads four repository secrets. The release script stops if any is missing.

| Secret | Where it comes from | Used for |
| --- | --- | --- |
| `BUNNY_STORAGE_PASSWORD` | Bunny → Storage → patchwork-releases → Access → Password (not the read-only one) | Uploading files |
| `BUNNY_API_KEY` | Bunny → Account settings → API key | Purging `latest.json` |
| `TAURI_SIGNING_PRIVATE_KEY` | The updater signing key | Signing the update files, in the release job only |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | That key's password | Signing the update files, in the release job only |

Set one with `gh secret set <NAME>` and paste the value at the prompt. If a Bunny key leaks,
regenerate it in Bunny and set it again.

The signing key's public half is in `updates.rs` (`SIGNING_KEY`), which the app and the release job
check signatures against. If `TAURI_SIGNING_PRIVATE_KEY` is ever set to a different key, the
release job stops before publishing. Losing the private key means no installed copy can update
again, so keep a copy outside GitHub. Changing it means every copy needs a manual reinstall.

## Share a build

Every release's files stay on the server, so send a link:

```
https://updates.patchworkeditor.com/v0.2.0/Patchwork_amd64.AppImage
```

On Linux, the AppImage runs after `chmod +x Patchwork_amd64.AppImage`. Windows uses the
`-setup.exe`, macOS the `.dmg`. The release script prints the links for the release it cut, and
`latest.json` always lists the newest ones.

## When a release goes wrong

- **The workflow failed.** Read the errors the script printed, or `gh run view <id> --log-failed`.
  Fix the cause on `main` and release the next patch version. Never reuse, move, or delete a pushed
  tag. The next release's notes take in the failed one's section (see [Changelog](#changelog)), so
  nothing it said is lost. To try a fix to the build first, run the workflow by hand.
- **A release went out broken.** Run `scripts/release.sh pull X.Y.Z` with `BUNNY_STORAGE_PASSWORD`
  and `BUNNY_API_KEY` set. It points `latest.json` back at the release before X.Y.Z, using the copy
  each release keeps in its own folder, so copies that haven't updated stop being offered it. Copies
  already on it keep it until the fix ships as the next patch. Releases before v0.10.18 have no
  copy, so they can't be gone back to.
- **"Artifact storage quota has been hit."** Something is still uploading GitHub artifacts. The
  workflow above uses none; check that no step calls `actions/upload-artifact`.
- **A job hit its timeout or the minutes ran out.** Wait for the monthly reset, or rerun the
  failed run with `gh run rerun <id>` once there are minutes again.
- **`latest.json` still shows the old version.** Purge it in Bunny (pull zone → Purge Cache) or
  run `curl -X POST -H "AccessKey: <api key>" "https://api.bunny.net/purge?url=https://updates.patchworkeditor.com/latest.json"`.
- **A copy won't install an update.** Its Settings → Updates shows why. "Not signed with
  Patchwork's release key" means the file and the key don't match.

## Rules

- Releases add their notes to `CHANGELOG.md`; edit it only to fix published notes, then run
  `scripts/release.sh notes <version>`.
- Don't change version numbers by hand.
- Tags start with `v` followed by a digit, such as `v0.2.0`, because the scripts and the workflow
  match that pattern.
- Never reuse, move, or delete a pushed tag. A broken release is followed by the next patch.

### For agents

- Push only when the user asks. A push to `main` builds only when it changes the Rust
  dependencies or the workflow.
- Cut a release only when the user asks, with `scripts/release.sh <version>` in the foreground. It
  asks nothing and ends once the tag is pushed and the run's link is printed. Run `watch` only when
  the user asks to follow the build.
- If the user asks for a version like `0.4.91` after `0.4.9`, explain the ordering problem above
  and confirm before tagging.
- **Discord changelog**: every time the version is bumped and pushed, write Discord release notes
  and end the reply with them in one fenced `markdown` code block, so the formatting survives
  copying. They come in addition to `CHANGELOG.md`, not instead of it. Write them from the
  release's section of `CHANGELOG.md` (`scripts/changelog.sh section <version>`), in this shape:
  - A `## Patchwork v<version>` heading, then `**Fixed**`, `**Added**`, `**Changed**`, and
    `**Note**` sections, in that order, leaving out any that are empty. No emoji.
  - Plain `-` bullets, one per user-visible change. Say what the user sees and does, in everyday
    words: "Queued messages now send automatically. If you send a message while the agent is still
    replying, it goes out by itself as soon as the reply is done." Not "Messages queued during a
    turn are sent once it finishes."
  - When a change has an exception, give it its own bullet in the same plain words, such as "If you
    stop a reply, or it fails with an error, your queued messages stay put until you send them
    yourself."
  - Name settings by their path as the user sees it: "Settings → AI Tokenomics → Generate chat and
    task titles".
  - Use `**Note**` for anything the user should know, such as an extra cost or a default worth
    changing.
  - No file paths, function names, commit hashes, pixel sizes, or other apps' names.
  - Stay well under Discord's 2,000-character message limit.
