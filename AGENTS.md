# Patchwork

Native desktop app in Rust: the UI is drawn with GPUI over its core in `crates/core`. Run all commands from the repo root.

Root `Cargo.toml` lints already enforce `unwrap()`, `println!`, undocumented `unsafe`, `missing_debug_implementations`, and style. This file covers what lints cannot see.

## Workspace layout

| Crate | Package | Owns |
|---|---|---|
| `crates/desktop` | `patchwork` | Window, workspace layout/dock/hosts, and in-window UI features. Calls `Core` directly. |
| `crates/ui` | `patchwork-ui` | Shared controls, theme, embedded assets (`assets/`), text/Markdown tools, actions, and hand-off helpers. Knows no feature; reads only `patchwork-core`. |
| `crates/sidecar` | `patchwork-sidecar` | Browser view drawing web pages and quilts. |
| `crates/pages` | `patchwork-pages` | Entity pages (Quilts, Android, onboarding) emitting events to `crates/desktop/src/workspace/hosts/`. |
| `crates/modals` | `patchwork-modals` | Entity dialogs and cards (Settings, granular, search, Add project, background, credential, interrupted). |
| `crates/core` | `patchwork-core` | Domains (providers, chats, projects, source control, settings, permissions, extensions, scheduled, orchestration, persistence). |
| `crates/android` | `patchwork-android` | Android SDK, emulators, devices, running apps via `Host` implemented on `Core`. |
| `crates/system` | `patchwork-system` | Machine helpers (`processes/`, `files/`, `machine/`), keychain, SSH client, and `Error`. Re-exported as `patchwork_core::system`. |
| `crates/remote` | `patchwork-remote` | Helper binary run on SSH hosts. |

The app's version is `[workspace.package] version` in the root `Cargo.toml`.

## Where code goes

- **Logic lives in `crates/core`.** UI crates draw state and call `Core` operations; if `Core` can decide it, the UI does not.
- **Core folders are domains, not layers.** `crates/core/src/lib.rs` holds only the module list, `Core`, and `Core::open`. Every other module is one domain holding its types, `impl Core` operations, and logic. Domains may reach down to `crate::system`, `logging/`, and other domains' types, never up into `orchestration/` or `Core` operations; `persistence/` stores what it is given and decides nothing.
- **Desktop and UI placement.** `crates/desktop/src` holds `main.rs` and feature folders, never loose files. `workspace/` is only the window shell, `hosts/`, and `dock/`; feature methods on `Workspace` stay in that feature's folder. Move UI code into `crates/ui` only once a second feature needs it, and keep an item `pub(crate)` unless another crate uses it.
- **File sizing and naming.** Keep a module as one file until it passes ~600 lines or two clear concerns, then split into `name/mod.rs` plus one file per concern. Don't create files under ~40 lines. Name files for what they hold (`commit_box.rs`, `lifecycle.rs`), never catch-alls (`ops.rs`, `utils.rs`, `helpers.rs`) or layer names (`render.rs`, `view.rs`). Don't repeat the folder name in the file name.
- **`mod.rs` is an index only.** Only `//!` docs, `mod` declarations (`#[cfg(test)] mod tests;`), and `use`/`pub use` re-exports. No functions, types, consts, statics, macros, `impl`s, or inline `mod x { … }`. Sub-folders get their own `mod.rs`. Sibling files import by path (`use super::run::git;`), never `use super::*` outside tests. Narrow visibility (`pub(super)`, `pub(in crate::parent)`).

## Commands & verification

| Task | Command |
|---|---|
| Run the app | `cargo run -p patchwork` |
| Format | `cargo fmt --all` / `cargo fmt --all -- --check` |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Tests | `cargo test --workspace` |
| Build AppImage | `scripts/build.sh` |

Before saying a task is done, run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`. All three must pass with zero warnings; report failing output. Never silence a lint—fix the code or use `#[expect(…, reason = "…")]` when the lint is wrong for that line.

## Never add

- **Tests, scripts, or harnesses.** No test files, shell/Python scripts, or QA harnesses anywhere (only `scripts/build.sh` and `scripts/hooks/` exist). You may run `cargo test --workspace`, not add to it. Run one-off commands inline. For UI changes, say what to check in the app.
- **Anything in `/tmp` or `$TMPDIR`.** Scratch work goes in gitignored `research/<job>/` at the repo root and is deleted when done. Pass tools an explicit path under `research/` when their default is `/tmp`.
- **Unrequested git actions.** No commits, branches, worktrees, or pushes unless asked. No destructive git commands (`reset --hard`, `checkout -- <file>`, `stash`, `clean`). Ask before deleting, moving, or renaming files.

## Rust guidelines

- **Errors.** Return `Result<T, Error>` (`patchwork_system::Error` / `patchwork_core::Error`); add a variant only when callers distinguish it. Write messages for a person (what failed and how to fix it). Propagate with `?`; never swallow errors (if truly harmless, log why with `log::warn!`). Use `unwrap_or_else(PoisonError::into_inner)` for locks. Log with `log` structured fields (`log::warn!(error:% = e; "Could not save: {{error}}")`), never logging keys, tokens, or header values.
- **State machines.** Name states with one enum carrying each state's data instead of loosely coupled booleans/options/maps. Transition in one lock or UI update (check-and-act together). Derive instead of duplicating state; key per-item state by item; give every waiting state a failure/timeout/stop exit.
- **Types & functions.** Make invalid calls unrepresentable (enums over bool pairs, newtypes over bare IDs, `Option` over sentinels). Keep fields private, max ~5 params (otherwise pass a struct), borrow unless retaining ownership, clone visibly, match exhaustively, name magic numbers as `const` with why, and keep `// Rust guideline compliant <date>` under `//!` docs in every `.rs` file. Public items get a `///` summary under 15 words and `# Errors` when returning `Result`.
- **Performance.** Never block the UI thread: run disk, process, and network work on background tasks and apply results on the UI thread. Check concurrency, pass count, allocations, streaming, timeouts, early exits, and I/O batching on hot paths.

## Running the app & performance checks

- The owner usually has Patchwork open; never stop or restart an instance you did not start.
- A debug build runs as "Patchwork (dev)" with separate `patchwork-dev` folders (scheduled tasks off). Start your own with `cargo run -p patchwork` in the background, say so, and stop it by PID when done. If a dev build is already open, isolate yours by setting `XDG_{CONFIG,DATA,CACHE,STATE}_HOME` under `target/own-xdg`. There is no hot reload.
- To debug lag: add temporary `log::warn!` timing around the suspect in your own debug build, reproduce, isolate causes by disabling one suspect at a time, fix, re-measure, remove timing, and stop your instance by PID.

## Design & accessibility

- Cut taglines, helper text, and secondary labels; fix the layout instead.
- Every spinner turns in step: use `ui::spinner`, never a rotating icon or custom spinner animation.
- Ship full accessibility on every new UI feature:
  - Name every control (`.aria_label(…)` on icon buttons and inputs), regions, and lists.
  - Interactive controls get `.role(…)`, `.tab_index(0)`, and Enter/Space handling.
  - Everything mouse-driven works by keyboard (menus, tabs, drag handles, resize separators with arrow keys and `.aria_numeric_value(…)` min/max).
  - Expose state via `.aria_expanded`, `.aria_selected`, `.aria_toggled`.
  - Dialogs use `Role::Dialog`, a name, and close on Escape. Errors use `Role::Alert`; live status/progress uses `Role::Status`.
  - Hover-only controls stay in the accessibility tree and appear on focus.

## Git & releases

- Leave changes uncommitted until reviewed. Every change lands on `main` via a squash-merged PR (ruleset requires PR + CI; only the Release workflow pushes to `main` directly). One PR per change; keep CI/docs/tooling in their own `ci`, `docs`, or `chore` commits.
- **"Push" / "push to git"** runs the whole PR flow without asking again:
  1. `git fetch origin main`, then create a branch per change from `origin/main` (carrying uncommitted edits cleanly, without touching local `main`).
  2. Commit each change on its branch and push.
  3. Open the PR with `gh pr create`: title is the commit subject, body fills in `.github/pull_request_template.md` (lead with what users see in the first 300 chars for release highlights; plain prose, no AI fluff).
  4. Enable auto-merge: `gh pr merge --squash --auto --delete-branch`.
  5. Switch back to `main` and `git pull --rebase origin main` (do not cherry-pick unmerged PRs onto local `main`).
  - In a Patchwork task worktree (`patchwork/…`), never switch, rebase, or pull that branch: commit on it, create each PR branch in a temporary worktree from `origin/main` (`git worktree add -b <name> <dir> origin/main`), cherry-pick there, push, open PR with `--auto`, and remove the temporary worktree.
- **"Release"** cuts a release from `origin/main` (see [docs/release.md](docs/release.md)):
  - Run `gh workflow run release.yml` (the workflow picks the next `YY.M.N` tag, writes notes, builds all three platforms, signs and publishes, tags `main`, and commits `Release YY.M.N`). Never bump versions or write `CHANGELOG.md` entries by hand.
  - Manual tagging (`git fetch origin --tags && git tag <YY.M.N> origin/main && git push origin <YY.M.N>`) also works.
- Commit subjects and PR titles use plain-language conventional commits: `type(scope): what users see now` (`feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `ci`, `chore`).
- Force-pushing, moving/deleting pushed tags, and rewriting pushed history require explicit confirmation. Never watch GitHub Actions runs unless asked.

## Finishing

When only the user's steps remain (trying the change, reviewing, confirming), report what was done and what wasn't, then end the turn. The same applies to `/goal`.
