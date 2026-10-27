# Releasing

Everything about shipping Patchwork: versions, the changelog, the release script, the GitHub
Actions workflow, the update server on Bunny, signing, and what to do when a release goes wrong.
For local builds, see [Development](development.md).

## Overview

```
pnpm release 0.2.0                      your machine: checks, version, CHANGELOG.md, commit, tag, push
        │
        ▼
Release workflow (GitHub Actions)       on the v0.2.0 tag
  ├─ appimage  ─┐
  ├─ windows   ─┼─ build, sign, upload → updates.patchworkeditor.com/v0.2.0/<file>
  ├─ macos     ─┘
  └─ release      latest.json → updates.patchworkeditor.com/latest.json, GitHub release with notes
        │
        ▼
Installed copies (0.10.10 and newer)    read latest.json, download, check SHA-256 and signature, install
```

The repo is private. The installers are not attached to GitHub releases; they live only on the
update server.

Three files do all of it:

| File | What it does |
| --- | --- |
| `scripts/release.sh` | Cuts the release on your machine, and does the build and publish steps in CI |
| `.github/workflows/release-appimage.yml` | Sets up each runner's tools and calls `scripts/release.sh` |
| `cliff.toml` | git-cliff config for `CHANGELOG.md` and the release notes |

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
`apps/web/package.json`, `apps/desktop/package.json`, `apps/desktop/src-tauri/tauri.conf.json`,
`apps/desktop/src-tauri/Cargo.toml`, `apps/server/Cargo.toml`, and `Cargo.lock`. The app shows it
next to the project name in the top left corner. To fix a mismatch, run
`scripts/release.sh version X.Y.Z`.

## Changelog

`CHANGELOG.md` and each release's notes come from the commit history with
[git-cliff](https://git-cliff.org/), configured in `cliff.toml`. Each commit goes into a group by
the first word of its subject:

| Group | Subjects starting with |
| --- | --- |
| Fixed | Fix, Fixed, Repair, Stop, Prevent, Correct |
| Added | Add, Show, Support, Allow, Introduce, Let, Remember, Reopen, Animate, Draw, Track, List, Render, Enable, Create |
| Removed | Remove, Drop, Delete, Deprecate |
| Changed | Update, Change, Polish, Tighten, Refactor, Rename, Move, Align, Make, Improve, Describe, Sidebar, Choice, Use, Replace |
| (left out) | Release, Bump, Merge, Chore |
| Other | anything else |

Internal changes stay out. A commit that only touches `.github/`, `scripts/`, `docs/`,
`cliff.toml`, or `*.md` is left out (`exclude_paths` in `cliff.toml`), so commit CI/CD, release,
and tooling changes separately from app changes. An internal change that has to touch app files
gets a `skip` parser for its subject instead.

So write commit subjects with a leading verb from that table. The app shows `CHANGELOG.md`, as it
was when the copy was built, in **Settings → Updates**, so users read these entries.

## Cut a release

From a clean `main` that is up to date with `origin/main`:

```sh
pnpm release 0.2.0
```

It needs `git`, `git-cliff` (`cargo install git-cliff --locked`), `pnpm`, `cargo`, `gh` (signed in),
`jq`, and `curl`. The script:

1. Checks the version, that `main` is clean and up to date, that the tag is new, and that the repo
   has the four secrets the workflow needs (see [Secrets](#secrets)).
2. Runs `pnpm typecheck`, `pnpm format:check`, `cargo clippy --workspace --all-targets -- -D warnings`,
   and `cargo test --workspace`, then prints the release notes.
3. After you type `y`, writes the version, regenerates `CHANGELOG.md` (a full rewrite, not
   `--prepend`), commits both as `Release v0.2.0`, creates the annotated tag, and pushes `main` and
   the tag. Nothing is written before you confirm.
4. Follows the Release workflow, and stops with the failed jobs' errors if it fails. Ctrl-C stops
   only the script, not the build.
5. Checks that `latest.json` names the new version, and prints the download links.

## The Release workflow

`.github/workflows/release-appimage.yml` runs on a `v*` tag. Pushes to `main` build nothing.

| Job | Runner | Command | Publishes |
| --- | --- | --- | --- |
| appimage | `ubuntu-22.04` (older glibc, so the AppImage runs on newer distributions) | `scripts/release.sh ci-build linux` | `Patchwork_amd64.AppImage` |
| windows | `windows-latest` | `scripts/release.sh ci-build windows` | `Patchwork_<version>_x64-setup.exe` |
| macos | `macos-14` (Apple silicon) | `scripts/release.sh ci-build macos` | `Patchwork_<version>_aarch64.dmg`, `Patchwork.app.tar.gz` |
| release | `ubuntu-latest`, after the three above | `scripts/release.sh ci-release` | `latest.json`, the GitHub release |

Each build job sets the tag's version first, so a tag pushed without the script still builds with
its own version. It builds with the signing key, uploads its files to
`updates.patchworkeditor.com/<tag>/`, and passes each file's name, SHA-256, and signature to the
release job as a job output. The release job writes `latest.json` from those, uploads it last (so
it never names a file that isn't there yet), purges Bunny's cached copy, and creates a GitHub
release named `Patchwork v0.2.0` with the notes from `git cliff --latest`.

To check the build without releasing, run the workflow by hand from the Actions tab
(**Run workflow**). It builds the current version and publishes nothing.

### Private-repo limits

A private repo gets limited Actions resources on the free plan, so the workflow stays inside them:

- **No artifacts.** Builds upload straight to Bunny. Artifact storage is counted across the
  billing month, so deleted artifacts keep counting until it resets.
- **No caches.** A cache saved for one tag is never restored by the next tag, so saving one only
  costs time.
- **Minutes.** The free plan has 2,000 a month. Windows minutes count twice and macOS minutes ten
  times. v0.10.11 took about 20 minutes on Linux, 18 on Windows, and 2 on macOS, so about 80
  counted minutes, or roughly 25 releases a month. Each job has a timeout (macOS 30 minutes, Linux and
  Windows 45, release 10), and a second run for the same tag cancels the first.

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

`latest.json` has the shape of GitHub's release API, plus each file's signature:

```json
{
  "tag_name": "v0.2.0",
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
| `TAURI_SIGNING_PRIVATE_KEY` | The updater signing key | Signing the update files |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | That key's password | Signing the update files |

Set one with `gh secret set <NAME>` and paste the value at the prompt. If a Bunny key leaks,
regenerate it in Bunny and set it again.

The signing key's public half is in `apps/desktop/src-tauri/tauri.signing.conf.json` (`pubkey`),
which the bundler reads when it signs, and decoded in `updates.rs` (`SIGNING_KEY`), which the app
checks against. Losing the private key means no installed copy can update again, so keep a copy
outside GitHub. Changing it means every copy needs a manual reinstall.

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
  tag.
- **"Artifact storage quota has been hit."** Something is still uploading GitHub artifacts. The
  workflow above uses none; check that no step calls `actions/upload-artifact`.
- **A job hit its timeout or the minutes ran out.** Wait for the monthly reset, or rerun the
  failed run with `gh run rerun <id>` once there are minutes again.
- **`latest.json` still shows the old version.** Purge it in Bunny (pull zone → Purge Cache) or
  run `curl -X POST -H "AccessKey: <api key>" "https://api.bunny.net/purge?url=https://updates.patchworkeditor.com/latest.json"`.
- **A copy won't install an update.** Its Settings → Updates shows why. "Not signed with
  Patchwork's release key" means the file and the key don't match.

## Rules

- Don't edit `CHANGELOG.md` by hand; the next release rewrites it.
- Don't change version numbers by hand.
- Tags start with `v` followed by a digit, such as `v0.2.0`, because git-cliff and the workflow
  match that pattern.
- Never reuse, move, or delete a pushed tag. A broken release is followed by the next patch.

### For agents

- Pushing `main` builds nothing. Push only when the user asks.
- Cut a release only when the user asks, with `pnpm release <version>`. The script follows the
  workflow itself; don't poll it separately.
- If the user asks for a version like `0.4.91` after `0.4.9`, explain the ordering problem above
  and confirm before tagging.
- **Discord changelog**: every time the version is bumped and pushed, write Discord release notes
  and end the reply with them in one fenced `markdown` code block, so the formatting survives
  copying. They come in addition to `CHANGELOG.md`, not instead of it. Write them from
  `git log <previous tag>..v<version>`, in this shape:
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
