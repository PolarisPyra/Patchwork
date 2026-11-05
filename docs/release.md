# Releasing

Everything about shipping Patchwork: versions, the changelog, the release script, the GitHub
Actions workflow, the GitHub releases the updater reads, signing, and what to do when a release goes wrong.
For local builds, see [Development](development.md).

## Overview

```
CI workflow (GitHub Actions)            every push to main: the checks
        │ passed
        ▼
Release workflow                        after each push to main that passed, and on the merge of the release pull request
  ├─ plan         does main carry a version with no tag yet, from a merged release pull request?
  ├─ pr           no: notes, version, CHANGELOG.md as one commit on the release branch → "Release v26.11.0" pull request
  │                 (you merge it with a merge commit; that push starts this workflow at once, without the checks)
  ├─ cut          yes: tag v26.11.0 on the release commit, push the tag
  │                 (or: pnpm release bump on your machine, which pushes a tag and starts the steps below)
  ├─ appimage  ─┐
  ├─ windows   ─┼─ build (no secrets) → one artifact each
  ├─ macos     ─┘
  └─ release      check, sign, check signature → GitHub release with the files, latest.json, notes
        │
        ▼
Installed copies (0.10.10 and newer)    read latest.json, download, check SHA-256 and signature, install
```

The repo is public. Each GitHub release carries every installer and the `latest.json` the app's
updater reads. The updater and the README's download links reach the newest one through
`github.com/PolarisPyra/Patchwork/releases/latest/download/<name>`, which needed the names to stop
carrying a version (see [Versions](#versions) and `scripts/bundle-names.sh`).

These files do all of it:

| File | What it does |
| --- | --- |
| `scripts/release.sh` | Cuts a release, in CI or on your machine, and does the build and publish steps in CI |
| `.github/workflows/release.yml` | Keeps the release pull request up to date after a push that passed, tags it once merged, builds each platform with `scripts/release.sh`, and publishes a tag |
| `.github/workflows/ci.yml` | Checks every push to `main` and every pull request, and runs by hand |
| `.github/actions/setup` | The runner setup both workflows share: Linux packages, Rust, pnpm, Node |
| `scripts/changelog.sh` | Checks commit messages, writes each release's notes from them, and reads and writes `CHANGELOG.md` |
| `docs/changelog-style.md` | The rules for changelog entries |

`scripts/release.sh --help` lists its commands.

## Versions

Versions are CalVer, `YY.M.N`: the two-digit calendar year, the month, and which release of that
month it is, counting from 0. Nobody works the next one out by hand: the Release workflow, and
`scripts/release.sh bump`, count it from the newest tag and the UTC calendar.

| Part | Meaning | Starts at |
| --- | --- | --- |
| `YY` | The two-digit calendar year of the release. `26` is 2026. | The clock |
| `M` | The month of the release, `1` to `12`, with no leading zero. | The clock |
| `N` | Which release of that month this is. A fix is just the next one. | `0` |

So `26.11.4` is "the fifth release of November 2026". The next release in the same month counts `N`
up (`26.11.4` → `26.11.5`); the first in a new month starts at 0 (`26.11.5` → `26.12.0`), and a new
year rolls over the same way (`26.12.3` → `27.1.0`). There are no levels to pick: a fix and a
feature both take the next number.

A release's tag is `v` and the whole version, such as `v26.11.0`. An explicit version, as in
`pnpm release 26.11.7`, cuts exactly that one. `scripts/release.sh next` prints what a bump gives
and changes nothing.

Each part is compared as a whole number, so `26.11.10` sorts above `26.11.9`, and `26.12.0` above
both. `N` has no practical ceiling: Windows installers allow up to 65535 for it. The releases before
this scheme, `26.1.0` to `26.3.9`, were `YY.N.P` (year, release number, patch); they read as January
to March 2026 and sort below every `26.11.x`, so copies on them update as usual.

Three parts are also what the tools read. `tauri-codegen` parses `tauri.conf.json` with
`semver::Version` and stops the build on anything else, and the updater compares a release's tag
against the running version with `semver`, so a two-part tag would read as no version at all and no
copy would ever be offered it. A version from the old scheme, `0.11.3`, sorts below every CalVer
release, so the first `26.x` release reaches every installed copy.

The repo's version always matches the latest tag. The script writes it into `package.json`,
`apps/web/package.json`, `apps/desktop/package.json`, and `apps/desktop/src-tauri/tauri.conf.json`.
The Rust crates read it from `tauri.conf.json` when they build (`crates/server/build.rs`); their own
versions stay at `0.0.0`, so a release never changes a Cargo file. The app shows it next to the
project name in the top left corner. To fix a mismatch, run
`scripts/release.sh version 26.1.0`.

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
an agent commits and says the work is unpushed. Once pushed, a commit with a paragraph for users
joins the release pull request once its push passes the checks, and goes out when that is merged.

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

On your machine, the notes wait in `.release-notes/<tag>.md` (ignored by git) until the release is
pushed. To change them before releasing, edit that file and run the release again; it is used as it
is, once it passes the style checks. The release pull request's notes are its `CHANGELOG.md`
section; the next push that passes writes them again from the commits, so fix a paragraph in a new
commit rather than in the pull request.

**Fixing notes after a release.** `CHANGELOG.md` is the source. Edit the release's section, commit
it, and run `scripts/release.sh notes 26.1.0` to publish it to the GitHub release and its
`latest.json`. Copies already installed show the fix once they update past it.

## Cut a release

A release is a pull request you merge. Every push to `main` runs the CI workflow's checks (see
[The CI workflow](#the-ci-workflow)). When they pass, the Release workflow starts and looks at the
commits since the last tag, up to the commit the checks passed on. If any has a `Changelog:`
paragraph other than `none`, it (`scripts/release.sh ci-pr`):

1. Writes the release notes (see [Changelog](#changelog)) and checks them, writes the next version,
   adds the notes to the top of `CHANGELOG.md`, and commits both as `Release v26.11.0` on top of the
   checked commit.
2. Force-pushes that one commit to the `release` branch, and opens the **Release v26.11.0** pull
   request into `main`, or updates its title and notes when it is open already.

So the pull request always shows what merging would release: every push that passes rebuilds it on
top of `main`. A push with only internal commits, or `Changelog: none`, changes nothing, and when
nothing for users is waiting the pull request is closed. A push that fails the checks changes
nothing either; the next push that passes brings the pull request up to date.

**To release**, merge the pull request with **Create a merge commit**, or run
`gh pr merge release --merge`. Squash and rebase merges are turned off for the repo, because the
release would then also cover commits pushed after the pull request was last updated, without their
notes. The merge is a push to `main` that changes `package.json`, which starts the Release workflow
straight away, without waiting for the checks. It sees that `main` carries a version with no tag
yet (`ci-plan`) and:

1. Tags the commit that set that version, the release pull request's own commit, and pushes the tag
   (`scripts/release.sh ci-tag`).
2. Builds, uploads, signs, and publishes, as below.

The checks still run on the merge, and when they pass, the run after them opens a new release pull
request for anything pushed before the merge that the release did not include.

The release need not wait for them, because what it tags is the pull request's commit: the commit
that passed the checks plus the version and notes, not the merge, which can also hold pushes that
had not passed yet. Those go out in the next release. If tagging fails, the run after the checks
on the merge tags it instead. A merge made while a release builds waits for it.

The repo lets the workflow open pull requests (Settings → Actions → General → **Allow GitHub
Actions to create and approve pull requests**) and allows only merge commits (Settings → General →
Pull Requests).

**From your machine**, on a clean `main` that is up to date with `origin/main`, for a release that
skips the pull request:

```sh
pnpm release bump          # the next version, 26.11.4 -> 26.11.5
pnpm release 26.11.7       # exactly this version
```

It needs `git`, `pnpm`, `cargo`, `gh` (signed in), `jq`, and `curl`. The script:

1. Checks the version, that `main` is clean and up to date, that the tag is new, and that the repo
   has the four secrets the workflow needs (see [Secrets](#secrets)).
2. Runs the CI workflow's checks, `pnpm check` and `cargo test --workspace`, and stops if they
   change a file, such as wire types that were not regenerated.
3. Writes the release notes (see [Changelog](#changelog)), checks them, and prints them.
4. Pushes commits not on `origin` yet, writes the version, adds the notes to the top of
   `CHANGELOG.md`, commits both as `Release v26.11.0`, creates the annotated tag, and pushes `main`
   and the tag. It asks no questions.
5. Prints the link to the Release workflow's run and ends. The build carries on on GitHub.

To follow the build, run `scripts/release.sh watch 26.11.0`. It waits for the run to end, stops with
the failed jobs' errors if it fails, then checks that `latest.json` names the new version and
prints the download links. Ctrl-C stops only the watching, not the build.

## The CI workflow

`.github/workflows/ci.yml` checks every push to `main` and every pull request, and runs when
started by hand, on the `ubuntu-22.04` runner the AppImage is built on: it builds the web app, runs
`pnpm check` and `cargo test --workspace`, and fails if the generated wire types differ from the
committed ones. These are the checks `scripts/release.sh` runs before a release. A push to `main`
that passes starts the Release workflow (the merge of the release pull request starts it before
the checks; see [Cut a release](#cut-a-release)). Its Rust cache is saved only from `main`. A newer run on
the same branch cancels an older one. What the workflows push or open with their own token, such as
the release pull request or the website's links, starts no run.

## The Release workflow

`.github/workflows/release.yml` runs after each run of the CI workflow on `main`, on a push to
`main` that changes `package.json`, on a `v*` tag, and when started by hand (see
[Cut a release](#cut-a-release) and [Caches](#caches)). Of the CI runs, only a push that passed the
checks does anything; of the pushes to `main`, only the merge of the release pull request, whose
message GitHub writes as `Merge pull request #N from PolarisPyra/release`.

| Job | Runner | Commands | Publishes |
| --- | --- | --- | --- |
| plan | `ubuntu-latest`, after a push passed or on the release merge | `ci-plan` | Nothing: the merged version to tag, or none |
| cut | `ubuntu-latest`, with write access, when plan found a version | `ci-tag` | The tag |
| pr | `ubuntu-latest`, with write access to the repo and pull requests, after a push passed: after cut or in its place | `ci-pr` | The `release` branch and its pull request |
| appimage | `ubuntu-22.04` (older glibc, so the AppImage runs on newer distributions) | `ci-build linux` | `Patchwork_amd64.AppImage` |
| windows | `ubuntu-24.04`, cross-compiled with `cargo-xwin` (its NSIS is new enough for Tauri's installer) | `ci-build windows` | `Patchwork_x64-setup.exe` |
| macos | `macos-15` (Apple silicon) | `ci-build macos` | `Patchwork_aarch64.dmg`, `Patchwork.app.tar.gz` |
| release | `ubuntu-latest`, after the three above | `ci-release` | `latest.json`, the GitHub release and its installers |

The build jobs and the release job take the tag from `$RELEASE_TAG`: the one cut made, or the one
pushed. cut pushes with the workflow's own token, which starts no second run, so the same run builds
the tag it made. Each build job sets the tag's version first, so a tag pushed without the script
still builds with its own version. It builds with no secrets and no write access, keeps its files
as a one-day artifact (`release-linux`, `release-windows`, `release-macos`), and passes each file's
name and SHA-256 to the release job as a job output.

The release job is the only one with the signing key. It, cut, pr, and site are the only ones with
write access to the repo, and none of them runs the project's dependencies. It downloads the
artifacts, and for each file checks it matches the SHA-256 its build job reported, signs it with the
Tauri CLI version `pnpm-lock.yaml` pins, and checks the signature with `minisign` against the key
built into the app (`SIGNING_KEY` in `updates/release.rs`), the way the updater will. Any mismatch
stops the release before anything is published, so installed copies are never offered a file that
won't install. It then creates the GitHub release `Patchwork v26.1.0` as a draft with the files and
`latest.json`, and publishes it as the latest once every file is up, so `latest.json` never names a
file that isn't there yet. Both carry the tag's section of `CHANGELOG.md`.

Every action is pinned to a commit, with the version it was in a comment, so a moved tag upstream
can't change what runs. To update one, look up the commit of its new release and replace both.

To check the build without releasing, run the workflow by hand from the Actions tab
(**Run workflow**). It builds all three platforms and publishes nothing, even when run on a tag.

### Caches

Most of each build is compiling the Rust dependencies. A tag's run can restore caches saved on
`main`, but not ones saved by another tag, so the dependency caches are saved only on `main`:

- A release from a merge runs on `main`, so it saves the Linux and Windows caches as it builds. A
  run by hand on `main` builds them without publishing anything, to save them before a tag from
  your machine after a change to a Cargo file.
- Tag runs restore those caches and save none.
- The Rust toolchain is pinned (`RUST_TOOLCHAIN` in the workflow), because a cache is only reused by
  the same compiler. Raising it rebuilds the caches on that push.
- The Windows job also caches the Windows SDK and C runtime that `cargo-xwin` downloads from
  Microsoft, for x86_64 only (`XWIN_ARCH`), keyed on the `cargo-xwin` version. Without it the
  download alone took 15 minutes on a runner.
- macOS has no cache. Its build is short, and its minutes count ten times.

### Actions limits

The workflow is built to stay inside the free plan's Actions limits, whichever plan the repo is on:

- **Artifacts for a day.** The build jobs' artifacts only carry the files to the release job, so
  they are kept for one day and not compressed.
- **Caches only from `main`.** See [Caches](#caches). The repo's cache storage is 10 GB; the Linux
  and Windows dependency caches fit in it, and older ones are evicted on their own.
- **Minutes.** The free plan has 2,000 a month. macOS minutes count ten times; the Windows
  installer is built on Linux, so its minutes count once (on a Windows runner they counted twice,
  and its build took twice as long). Without caches, v0.10.16 took about 13 minutes on Linux, 17 on
  Windows, and 2 on macOS, about 70 counted minutes; with Windows built on Linux that is about 50.
  A change to the Rust dependencies costs one cache build of Linux and Windows. Each job has a
  timeout (macOS and Windows 30 minutes, Linux 45, release 10). A second
  run for the same ref waits for the first, so a release is never stopped halfway through
  publishing. Each push to `main` costs its CI run, about 5 minutes with the cache, plus a minute
  for the Release run's first job and another to update the release pull request. Each release
  adds its builds.

Check usage at github.com → Settings → Billing and plans.

## Release files

Every GitHub release carries the same files:

```
latest.json                  what the updater reads, from the release GitHub marks as the latest
Patchwork_amd64.AppImage
Patchwork_x64-setup.exe
Patchwork_aarch64.dmg
Patchwork.app.tar.gz
```

`/releases/latest/download/<name>` answers the latest release's copy, and
`/releases/download/<tag>/<name>` one release's, which never changes. Tauri names each bundle after
the version it built, and `ci-build` takes the version out of the name before anything is published
or signed (`scripts/bundle-names.sh`), so a file keeps one name from release to release. The updater
does not read the name for a version: it picks its file by suffix, so it finds its download either
way.

`latest.json` has the shape of GitHub's release API, plus each file's signature. `body` is the
release's section of `CHANGELOG.md`:

```json
{
  "tag_name": "v26.1.0",
  "body": "## [26.1.0] - 2026-01-02\n\n### Added\n\n- …",
  "assets": [
    {
      "name": "Patchwork_amd64.AppImage",
      "browser_download_url": "https://github.com/PolarisPyra/Patchwork/releases/download/v26.1.0/Patchwork_amd64.AppImage",
      "digest": "sha256:…",
      "signature": "untrusted comment: signature from tauri secret key\n…"
    }
  ]
}
```

### How copies update

`apps/desktop/src-tauri/src/updates/` checks `latest.json` 30 seconds after launch and then as
often as **Settings → Updates** says. When the tag is newer than the running version, or
`withdrawn` lists the running version (see [When a release goes wrong](#when-a-release-goes-wrong)),
it picks its
platform's file (the AppImage, the `-setup.exe`, or `Patchwork.app.tar.gz`), downloads it, and
installs it only if both its SHA-256 and its minisign signature check out. The signature is checked
against the public key built into the app, so a changed file on the server, or a changed
`latest.json`, can't install anything the signing key didn't sign. Dev builds and copies not
installed from a release report new versions but don't install them.

Patchwork 0.10.9 and older checked GitHub's API instead, which the repo being private at the time
blocked. Those copies need a one-time manual download of 0.10.10 or newer.

Patchwork 26.11.12 and older read `https://updates.patchworkeditor.com/latest.json`, which was on
Bunny. A Cloudflare redirect rule on that hostname sends it to
`https://github.com/PolarisPyra/Patchwork/releases/latest/download/latest.json`, so they update to
the first GitHub-only release like any other. Keep the rule while copies that old may still be
around.

### Secrets

The workflow reads two repository secrets. The release script stops if either is missing.

| Secret | Where it comes from | Used for |
| --- | --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | The updater signing key | Signing the update files, in the release job only |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | That key's password | Signing the update files, in the release job only |

Set one with `gh secret set <NAME>` and paste the value at the prompt.

The signing key's public half is in `updates/release.rs` (`SIGNING_KEY`), which the app and the release job
check signatures against. If `TAURI_SIGNING_PRIVATE_KEY` is ever set to a different key, the
release job stops before publishing. Losing the private key means no installed copy can update
again, so keep a copy outside GitHub. Changing it means every copy needs a manual reinstall.

## Share a build

Every release's files stay on its GitHub release, so send a link:

```
https://github.com/PolarisPyra/Patchwork/releases/download/v26.1.0/Patchwork_amd64.AppImage
```

On Linux, the AppImage runs after `chmod +x Patchwork_amd64.AppImage`. Windows uses the
`-setup.exe`, macOS the `.dmg`. Only the tag in that link changes from one release to the next; the
file name stays the same. The release script prints the links for the release it cut, and
`latest.json` always lists the newest ones.

## When a release goes wrong

- **The workflow failed.** Read the errors the script printed, or `gh run view <id> --log-failed`.
  Fix the cause on `main`; the next release carries it. Never reuse, move, or delete a pushed
  tag. The next release's notes take in the failed one's section (see [Changelog](#changelog)), so
  nothing it said is lost. To try a fix to the build first, run the workflow by hand.
- **A release went out broken.** Run `scripts/release.sh pull YY.M.N`. It makes the release before
  YY.M.N the latest on GitHub again and YY.M.N a prerelease, so copies that haven't updated stop
  being offered it. It also lists YY.M.N under `withdrawn` in that release's own `latest.json`, and
  copies already on it go back to the release before it at their next update check, like any
  update. Pulling a second release in a row keeps the first on the list. Releases before the move to
  GitHub (v26.11.12 and older) have no `latest.json` on GitHub, so they can't be gone back to.

  A release that changed the database schema (`crates/server/src/persistence/migrations/`) is left
  off the list, because the release before it refuses a database a newer build migrated. The script
  says so, and copies on it keep it until the fix ships; merge the release pull request as
  soon as it carries the fix. Copies running a release from before rollbacks existed never
  read `withdrawn`, so they too wait for the fix.
- **A job hit its timeout or the minutes ran out.** Wait for the monthly reset, or rerun the
  failed run with `gh run rerun <id>` once there are minutes again.
- **`latest.json` still shows the old version.** The new release is still a draft, or another one
  is marked as the latest. Check with `gh release list`, and fix it with
  `gh release edit vYY.M.N --draft=false --latest`.
- **A copy won't install an update.** Its Settings → Updates shows why. "Not signed with
  Patchwork's release key" means the file and the key don't match.

## Rules

- Releases add their notes to `CHANGELOG.md`; edit it only to fix published notes, then run
  `scripts/release.sh notes <version>`.
- Don't change version numbers by hand.
- Tags start with `v` followed by a digit, such as `v26.1.0`, because the scripts and the workflow
  match that pattern.
- Never reuse, move, or delete a pushed tag. A broken release is followed by the next one.

### For agents

- Push only when the user asks. A push to `main` with a paragraph for users joins the release pull
  request once it passes the checks; it ships when that pull request is merged.
- "push" is only ever a plain push. Merge the release pull request (`gh pr merge release --merge`,
  never squash or rebase) only when the user asks to release. Never run `scripts/release.sh bump`
  or cut a version yourself unless the user asks for a release that skips the pull request, and
  then run it in the foreground. It asks nothing and ends once the tag is pushed and the run's link is printed. Run
  `watch` only when the user asks to follow the build.
- A version is `YY.M.N`, always with all three parts: `26.11.0`, `26.11.1`. Never work one out by
  hand; `scripts/release.sh next` prints it.
- **Discord changelog**: every time you cut a release, write Discord release notes
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
