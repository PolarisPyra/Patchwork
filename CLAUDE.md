# Patchwork

Native desktop app in Rust: the UI is drawn with GPUI, over the backend in `crates/server`. Run everything from the repo root.

## Layout

- Cargo workspace:
  - `crates/desktop` (package `patchwork`): the app. It calls `Server` directly.
  - `crates/server` (`patchwork-server`): providers, persistence, usage, thread import.
- The app's version is `[workspace.package] version` in the root `Cargo.toml`.

### `crates/desktop/src`

- `src/` holds `main.rs` and folders, never loose files. A folder is one piece of UI (`chat`, `sidebar`, `settings`, `canvas`, `panel`, `workspace`, `onboarding`), with its state and entry view in their own files (`state.rs`, `render.rs`, `view.rs`). A sub-feature takes the same shape one level down: `chat/composer/`, `settings/{providers,extensions,mcp}/`.
- `ui/` holds only what two or more features use: the shared controls in `ui/controls.rs` (`button`, `icon_button`, `spinner`, `scrollbar`, …), the colours and sizes in `ui/theme.rs`, the embedded icons and fonts in `ui/assets.rs`, and the shared menus, dialogs and text tools.
- New code goes in the feature that uses it. Move it up into `ui/` once a second feature needs it, and down into the feature when only one is left.
- `ui/assets.rs` embeds `crates/desktop/assets/` (fonts, icons) and the provider logos in the root `assets/`.

### `crates/server/src`

- Folders are domains, not layers. `lib.rs` holds only the module list, `Server`, and `Server::open`; every other module is one domain (`threads`, `projects`, `settings`, `permissions`, `plugins`, `scheduled`, `provider`, `orchestration`, …) and holds its types, its `impl Server` operations, and its logic, so a type is `crate::threads::Thread`. A folder domain splits them into `types.rs` and one file per concern, named for what it does (`threads/{lifecycle,list,options}.rs`, `updates/check.rs`), never a catch-all such as `ops.rs`; a one-file domain such as `projects.rs` keeps the same order: types first, then the operations, then the logic. There is no `types/` or `server/` folder. A helper another domain calls is `pub(crate)`.
- Machine-level helpers that no domain owns live in `system/`: the command and environment programs run with, login shells, the home folder and path resolution, opening files on the desktop.
- A domain's files may reach down to `system/`, `logging/` and other domains' types, never up into `orchestration/` or a `Server` operation; `persistence/` stores what it is given and decides nothing.
- An area that exists on both sides keeps the same name on each: `crates/server/src/threads/`, `crates/desktop/src/sidebar/` reads `crate::threads`.

### Both crates

- A module is one file until it passes about 600 lines or has two clear concerns. Then it becomes `name/mod.rs` plus one file per concern, and `mod.rs` re-exports the public items so callers keep their paths: `provider/claude/{config,input,parse,launch,cli}.rs`, `chat/composer/`.
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

- Keep logic in `crates/server`. The app draws state and calls `Server` operations; it decides nothing the server could.
- No `unwrap()`/`expect()` in app code: return errors instead. Tests are the exception.
- Use the `log` macros (`log::warn!`, `log::error!`), which the app writes to its log file, not `println!`/`eprintln!`.
- Never block the UI thread. Move heavy or blocking work to a background task and apply the result on the UI thread.

## Running the app

- I usually have Patchwork open. Don't stop or restart an instance you didn't start.
- To see a change, build and start your own with `cargo run -p patchwork` in the background, and say so. Stop it by its PID when you're done.
- There is no hot reload: every change needs a rebuild and a restart of your instance.

## While I'm watching

- No destructive git commands: no `reset --hard`, `checkout -- <file>`, `stash`, or `clean`.
- Ask before deleting, moving, or renaming files.

## Design

- Cut taglines, helper text, and secondary labels. If a screen needs them, fix the layout.
- Pick what improves UX, DX, and agent experience without breaking anything.
- Every new feature ships full accessibility, so screen readers and the agent-desktop skill can read and use all of it:
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
- "Push to git", or just "push", covers the whole job without asking again or waiting for review:
  1. Stage and commit anything uncommitted, with its `Changelog:` paragraph.
  2. Run `git pull --rebase origin main`: each release's workflow pushes a commit of its own to `main` (the website's download links), so the remote is often one ahead. Then push `main`.

  That is all a push is. The Release workflow then keeps a "Release vYY.M.N" pull request up to date with the version and the `CHANGELOG.md` section, and merging it, which is mine to do, builds and publishes the release (`docs/release.md`). Never set the version, write the changelog section, tag, or merge that pull request yourself. Say what went up.
- Force-pushing, moving or deleting a pushed tag, and rewriting pushed history still need their own ask.
- Commit subjects are short, sentence-case, and imperative, with no prefix. Describe the user-visible change, for example "Nudge the header project dropdown 2px left".
- `CHANGELOG.md` is user-facing: the app shows it in Settings → Updates. Every commit that changes the app needs a paragraph in its message starting with `Changelog:` and its group (Added, Changed, Fixed, Removed, or Security), written for users by the rules in `docs/changelog-style.md`: what they see or can do now, where, and what is different, for example "Changelog: Changed: Every loading indicator now uses the steady spinner from the thread list: plan steps, background tasks, … The old one rotated and looked like it shook." A Fixed entry says what went wrong and what happens now, never "again" or "back". Run every `Changelog:` paragraph through the declauding skill before committing it. A change users can't notice gets `Changelog: none`, and so does a follow-up to a change that hasn't been released yet. The `commit-msg` hook (`git config core.hooksPath scripts/hooks`) enforces both; if it turns a commit down, fix the message rather than bypassing the hook.
- Keep internal CI and tooling changes out of the changelog. Commit them separately from app changes, so they only touch `.github/`, `scripts/`, `docs/`, or `*.md`, which need no `Changelog:` paragraph. If an internal change has to touch app files, write `Changelog: none`.
- Never watch GitHub Actions runs unless I ask.

## Finishing

- When only my steps are left (trying the change, reviewing, confirming), report what was done and what wasn't, then end the turn. Don't wait or ask for confirmation.
- The same applies to a `/goal`: it's met once everything left depends only on me.
