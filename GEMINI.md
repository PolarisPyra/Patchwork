# Patchwork

Native desktop app in Rust: the UI is drawn with GPUI, over its core in `crates/core`. Run everything from the repo root.

## Layout

- Cargo workspace:
  - `crates/desktop` (package `patchwork`): the app: the window and the features drawn in it. It calls `Core` directly.
  - `crates/ui` (`patchwork-ui`): what two or more UI crates use: the controls, theme, assets, the app's actions, and the shared menus, dialogs and text tools.
  - `crates/sidecar` (`patchwork-sidecar`): the browser, which draws web pages and quilts.
  - `crates/pages` (`patchwork-pages`): the pages the window shows: Quilts, the Android panel, and first-run setup. Each is its own entity and asks the window to act through its events, which `crates/desktop/src/workspace/hosts/` handles.
  - `crates/modals` (`patchwork-modals`): the dialogs and cards over the window: Settings, granular settings, search, Add a project, the background picker, git sign-in, and the interrupted replies card. Each is its own entity with events, like the pages; the window's glue for each is in `crates/desktop/src/workspace/hosts/`.
  - `crates/core` (`patchwork-core`): providers, persistence, usage, chat import.
  - `crates/android` (`patchwork-android`): the Android tools: SDK, emulators, devices, running an app. It reads the app's settings and state through its `Host` trait, which `Core` implements in `core/src/android.rs`; the app calls it through `core.android()`.
  - `crates/system` (`patchwork-system`): the machine-level helpers, grouped as `processes/`, `files/` and `machine/`, the credential store (`keychain.rs`), and the one `Error`. It knows nothing of chats or providers; the core re-exports it as `crate::system`.
  - `crates/remote` (`patchwork-remote`): the helper Patchwork runs on a machine reached over SSH. The app's end of that connection is `crates/system/src/ssh/`.
- The app's version is `[workspace.package] version` in the root `Cargo.toml`.

### The UI crates

- `crates/desktop/src` holds `main.rs` and folders, never loose files. A folder is one piece of UI (`chat`, `sidebar`, `canvas`, `agents`, `orchestrator`, `terminal`, `logs`, `screen`, `workspace`), with its state and entry view in their own files. A sub-feature takes the same shape one level down: `chat/composer/`. `workspace/` is only the window: its state, layout, bars, actions and update card, `hosts/` for its glue to the pages and dialogs of other crates, and `dock/`, the tabbed area under the chat. What a feature does to the window sits in that feature's folder (`chat/session.rs`, `agents/lifecycle.rs`, `sidebar/projects/`, `canvas/area.rs`), and each dock tab is its own feature: `terminal/`, `logs/` (Output) and `screen/`; the browser is `crates/sidecar/src/browser/`, and the pages are `crates/pages/src/{quilts,android,onboarding}/`, and the dialogs are `crates/modals/src/{settings,granular,add_project}/`, `search.rs`, `background.rs`, `credential.rs` and `interrupted.rs`, with Settings' panes in `settings/panes/`; each is laid out the same way.
- `crates/ui/src` holds only what two or more features use: the shared controls in `controls/` (`button`, `icon_button`, `spinner`, `scrollbar`, menus, dialogs, …), the text tools in `text/` (formatting, highlighting, Markdown, selection, the text field), the embedded assets and logos in `brand/`, the colours and sizes in `theme/`, the app's actions in `actions.rs`, and the hand-off helpers in `tasks.rs`. Callers write `patchwork_ui::theme`.
- New code goes in the feature that uses it. Move it up into `crates/ui` once a second feature needs it, and down into the feature when only one is left. `crates/ui` knows no feature: it reads only `patchwork-core`.
- `crates/ui/src/brand/assets.rs` embeds the fonts, icons and provider logos in the root `assets/`.
- An inherent `impl Workspace` must sit in the crate that defines `Workspace`, so a feature that adds methods to it stays in `crates/desktop`.

### `crates/core/src`

- Folders are domains, not layers. `lib.rs` holds only the module list, `Core`, and `Core::open`; every other module is one domain (`chats`, `projects`, `source_control`, `settings`, `permissions`, `extensions`, `scheduled`, `provider`, `orchestration`, …) and holds its types, its `impl Core` operations, and its logic, so a type is `crate::chats::Chat`. A folder domain splits them into `types.rs` and one file per concern, named for what it does (`chats/{lifecycle,list,options}.rs`, `updates/check.rs`), never a catch-all such as `ops.rs`; a one-file domain such as `backgrounds.rs` keeps the same order: types first, then the operations, then the logic. There is no `types/` or `server/` folder. A helper another domain calls is `pub(crate)`.
- Machine-level helpers that no domain owns live in `crates/system`, which the core reads as `crate::system`: the command and environment programs run with, login shells, the home folder and path resolution, making folders, opening files on the desktop and in the editor.
- A domain's files may reach down to `crate::system`, `logging/` and other domains' types, never up into `orchestration/` or a `Core` operation; `persistence/` stores what it is given and decides nothing.
- An area that exists on both sides keeps the same name on each: `crates/core/src/source_control/`, `crates/desktop/src/sidebar/source_control/` reads `patchwork_core::source_control`.
- A lib crate's `pub` items are its API, so the workspace lints ask them for `Debug`, `#[must_use]` and a short first doc paragraph. Keep an item `pub(crate)` unless another crate uses it.

### Every crate

- A module is one file until it passes about 600 lines or has two clear concerns. Then it becomes `name/mod.rs` plus one file per concern, and `mod.rs` re-exports the public items so callers keep their paths: `provider/claude/{config,input,parse,launch,cli}.rs`, `chat/composer/`.
- Name a file for the thing it holds, as the app shows it or as the code does it (`commit_box.rs`, `file_list.rs`, `rename_field.rs`, `git_status.rs`), not for its layer (`render.rs`, `view.rs`, `types.rs`, `actions.rs`), and don't repeat the folder's name in it (`usage/ring.rs`, not `usage/usage_ring.rs`). `crates/desktop/src/sidebar/` is the model.
- `mod.rs` is the folder's index and never holds code: only `//!` docs, `mod` lines (with their `#[cfg(test)]`), and `use`/`pub use` re-exports. No functions, types, consts, statics, macros, impls or inline `mod x { … }` blocks; those go in a named file beside it, and a sub-folder gets its own `mod.rs`. See `AGENTS.md`.
- Don't create a file under about 40 lines. Put the code in its only user, or in the sibling that owns its concern. A folder's own wire types go in its `types.rs`. Test fixtures go in the module's `tests.rs`.
- Files in a folder import each other's items by path (`use super::state::Composer;`), not `use super::*`.

## Commands

| Task | Command |
|---|---|
| Run the app | `cargo run -p patchwork` |
| Format | `cargo fmt --all` / `cargo fmt --all -- --check` |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Tests | `cargo test --workspace` |
| Build the AppImage | `scripts/build.sh` |

CI runs the check form of each of the last three on every push to `main`.

## Verify

- Run `cargo fmt --all`, then clippy with `-D warnings` (no warnings left behind), then `cargo test --workspace`.
- Report the output of any check that fails.

## Rust

- Keep logic in `crates/core`. The app draws state and calls `Core` operations; it decides nothing the core could.
- No `unwrap()`/`expect()` in app code: return errors instead. Tests are the exception.
- Use the `log` macros (`log::warn!`, `log::error!`), which the app writes to its log file, not `println!`/`eprintln!`.
- Never block the UI thread. Move heavy or blocking work to a background task and apply the result on the UI thread.

## Running the app

- I usually have Patchwork open. Don't stop or restart an instance you didn't start.
- A debug build is its own app, "Patchwork (dev)": it keeps `patchwork-dev` folders, which start as a copy of the installed app's config and database with the scheduled tasks turned off, so it opens beside the installed app and never touches its data. I try a change with `cargo run -p patchwork`, before anything is committed.
- To see a change, build and start your own with `cargo run -p patchwork` in the background, and say so. Stop it by its PID when you're done. When a dev build is already open, yours would only raise that one: give yours its own folders by setting the four `XDG_*_HOME` variables to folders under `target/own-xdg`.
- There is no hot reload: every change needs a rebuild and a restart of your instance.

## While I'm watching

- No destructive git commands: no `reset --hard`, `checkout -- <file>`, `stash`, or `clean`.
- Ask before deleting, moving, or renaming files.

## Design

- Cut taglines, helper text, and secondary labels. If a screen needs them, fix the layout.
- Pick what improves UX, DX, and agent experience without breaking anything.
- Every new feature ships full accessibility, so screen readers can read and use all of it:
  - Every control has a name: icon buttons and inputs get `.aria_label(…)`.
  - Anything that acts gets a `.role(…)`, a `.tab_index(0)`, and Enter and Space handling.
  - Everything the mouse can do, the keyboard can too: menus, tabs, drag handles and resize separators (arrow keys, `.aria_numeric_value(…)` with its min and max).
  - States are exposed: `.aria_expanded`, `.aria_selected`, `.aria_toggled`.
  - Dialogs have `Role::Dialog`, a name, and close on Escape. Regions and lists have names.
  - Errors use `Role::Alert`, and progress or status that changes uses `Role::Status`.
  - Controls that show only on hover stay in the accessibility tree and show on focus.
- Every spinner turns in step. Use `ui::spinner`, never a rotating icon or a spinner with its own animation.

## Scripts

- Never add scripts: no shell, Python, or other script files, and no QA checks or harnesses. Not in the repo, not in `scripts/`, and not anywhere else. Run what a task needs inline instead. The only scripts are `scripts/build.sh`, which builds the AppImage, and `scripts/hooks/`.
- There is no QA harness. For a UI change, say what to look at in the app, and I check it.

## Performance checks

Measure lag in a debug build you started:

1. Add temporary timing around the suspect (frame times, how long the action takes) and write it with `log::warn!`.
2. Run the action in your instance, then read the numbers from the log.
3. To find the cause, turn one suspect off at a time and compare.
4. Fix the cause, then measure again.
5. Remove the timing, stop your instance by PID, and check `git status` shows only the fix.

## Tests

- Never write tests: no test files, runners, dependencies, or scripts.
- You may run `cargo test --workspace`, but don't add to it.

## Git

- Don't commit until I've reviewed the change and say so, and don't push until I ask.
- Every change reaches `main` through a PR that is squash-merged, so it lands as one commit whose subject is the PR title plus "(#N)". A ruleset on `main` requires the PR and CI; only the release workflow pushes to `main` directly. One PR per change: two features are two PRs.
- "Push to git", or just "push", covers the whole job without asking again or waiting for review:
  1. Run `git pull --rebase origin main` (each release's workflow pushes "Release YY.M.N" to `main`, so the remote is often one ahead), then make a branch per change from `origin/main`.
  2. Commit each change on its branch and push the branch.
  3. Open the PR with `gh pr create`: the title is the commit subject, and the body fills in `.github/pull_request_template.md`. The release notes read both.
  4. Run `gh pr merge --squash --auto --delete-branch`, so it merges once the checks pass.
  5. Switch back to `main`, run `git pull --rebase origin main`, and `git cherry-pick` each branch's commit, so the working tree still has the change while its PR waits on the checks. A later `git pull --rebase origin main` drops that commit once the squash-merge is on the remote.

  In a Patchwork task worktree (on a `patchwork/…` branch), never switch, rebase or pull into its branch: Patchwork merges that branch. Skip steps 1 and 5. Commit each change on the task's own branch, then make each PR branch in a worktree of its own (`git fetch origin main`, `git worktree add -b <name> <dir> origin/main`), cherry-pick that commit there, push, open the PR, then `git worktree remove <dir>`. Once the squash-merge is on `origin/main`, the task's worktree has nothing left to merge.

  That is all a push is. Say what went up.
- "Release" means: run `git pull --rebase origin main`, then tag `origin/main` (local `main` may hold a commit whose PR has not merged) with the next `YY.M.N` (the next N this month, or 0 in a new month) and push the tag. The Release workflow writes the notes, builds and publishes, then commits the version and the `CHANGELOG.md` section to `main` as "Release YY.M.N" (`docs/release.md`). Never set the version or write the changelog section yourself.
- Force-pushing, moving or deleting a pushed tag, and rewriting pushed history still need their own ask.
- Commit subjects and PR titles are conventional commits in plain language: `type(scope): what users see now`, for example "fix(chat): new threads no longer spike CPU". The type is `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `ci`, or `chore`; the scope is the area (`chat`, `sidebar`, `settings`, `core`, `release`, …). The release notes count the `fix` and `perf` commits no highlight covers.
- The PR body says the problem in a sentence or two, then how the change fixes it, as `.github/pull_request_template.md` lays out. The release highlights read its first 300 characters, so lead with what users see. Run it through the declauding skill before opening the PR.
- Keep internal CI and tooling changes in their own commits, typed `ci`, `docs`, or `chore`.
- Never watch GitHub Actions runs unless I ask.

## Finishing

- When only my steps are left (trying the change, reviewing, confirming), report what was done and what wasn't, then end the turn. Don't wait or ask for confirmation.
- The same applies to a `/goal`: it's met once everything left depends only on me.
