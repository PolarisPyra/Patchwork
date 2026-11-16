# CI and deploys

The GitHub Actions workflows in `.github/workflows`: what starts each one, what it caches, the
secrets it reads, and how long it takes.

## The workflows

| Workflow | Starts on | What it does |
| --- | --- | --- |
| CI (`ci.yml`) | Every PR, and each push to `main` that changes more than `site/`, `docs/` or `*.md` | One job, `check`: rustfmt, clippy with `-D warnings`, `cargo test --workspace` |
| PR Changelog (`pr-changelog.yml`) | A PR opened, edited or pushed to | Runs the `commit-msg` hook on the PR's title and body |
| Site (`site.yml`) | A PR or a push to `main` that changes `site/`, `.github/deploy/` or the workflow | Checks the pages; on `main`, deploys `site/` to Cloudflare Pages and checks the deployed page |
| Release (`release.yml`) | A pushed tag `YY.M.N`, or by hand | Writes the notes, builds the three apps, signs and publishes them, commits the version to `main` ([Releasing](release.md)) |
| Audit (`audit.yml`) | Mondays, and a push to `main` that changes `Cargo.lock` | `cargo audit` |

The ruleset on `main` requires a PR, squash merging, and the `check` and `Changelog` checks.
GitHub offers merge queues only for repositories an organization owns, and this one belongs to a
user account, so there is none. A PR's checks run on its merge with `main` as it was at the last
push to the PR, and CI runs once more on `main` after the merge.

## The app and the website

The app and the website are delivered separately. The Release workflow builds and publishes the
app and never deploys the website. The Site workflow deploys the website and compiles nothing; it
shares no job, cache or artifact with the others.

The website depends on the app in one place: `site/index.html` links to the files of one release.
The Release workflow's last job writes the new tag into those links
(`node site/refresh-downloads.mjs`) and commits the page to `main`. That commit changes `site/`,
so the Site workflow deploys it like any other change to the website. No Rust type, schema or
generated file is shared.

## Caches

- CI restores one Rust cache (`shared-key: ci`, about 1.3 GB of compiled dependencies), saved only
  by runs on `main`. PRs read it. The three workspace crates are compiled in every run.
- Each Release build job (`linux`, `windows`, `macos`) has its own Rust cache of 1.5 to 1.8 GB,
  also saved only by runs on `main`. A tag's run reads the newest one `main` saved. GitHub keeps
  10 GB per repository and lets a tag's run read only caches saved from `main` or from that tag.
- To refresh the Release caches after a dependency changes, run the Release workflow by hand on
  `main` (Actions → Release → Run workflow). It builds all three apps and publishes nothing.

`gh cache list` shows what is stored. A Release build that takes twice as long as usual has
usually found no cache: look for "No cache found" in its `Swatinem/rust-cache` step, then refresh
as above.

## Secrets

| Secret | Read by | For |
| --- | --- | --- |
| `CLOUDFLARE_API_TOKEN`, `CLOUDFLARE_ACCOUNT_ID` | Site | Deploying to the `patchwork-site` Pages project. Without the token the workflow fails. |
| `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Release | Signing the release's files |
| `RELEASE_DEPLOY_KEY` | Release | Pushing the release commit to `main` without a PR |
| `DEEPSEEK_API_KEY` | Release | The highlights in the notes; without it the notes are the full list |
| `DISCORD_RELEASE_WEBHOOK_URL` | Release | Announcing the release; without it nothing is posted |

## Timings

Measured on 2026-11-16 from the 150 most recent CI runs and the 30 most recent Release runs, in
seconds. All run on GitHub's standard runners, which are free for a public repository.

| | p50 | p95 |
| --- | --- | --- |
| CI, from the push to the result | 104 | 129 |
| Waiting for a runner | 2 | 3 |
| Installing the Linux packages | 18 | 33 |
| Installing the Rust toolchain | 9 | 11 |
| Restoring the Rust cache | 22 | 29 |
| Format | 3 | 3 |
| Lint | 16 | 20 |
| Test (168 tests, which run in 1.5 to 5 seconds; the rest is compiling) | 26 | 33 |
| Release, from the tag to the last job | 618 | 946 |
| Release build job: Linux / Windows / macOS | 490 / 322 / 504 | 625 / 403 / 924 |

The CI cache was found in each of the 15 runs sampled. Two of the 150 runs compiled every
dependency, and took 780 and 811 seconds.

The tests are not split across jobs, because they run in under 5 seconds. A second job would
spend about 50 seconds installing packages and restoring the cache before it compiled anything.

## Before a push

The `pre-push` hook runs the format and lint checks locally; see
[Development](development.md#git-hooks).
