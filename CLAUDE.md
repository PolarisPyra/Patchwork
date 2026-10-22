# CLAUDE.md

## Development setup

- **Dev instance:** the only app instance that is ever started or restarted. Run it only with `pnpm tauri dev`. Never run `pnpm dev` on its own, `target/debug/patchwork`, or other build outputs directly.
- The user keeps their own `pnpm tauri dev` session open and views changes there. Vite hot-reloads UI edits, and Rust changes rebuild on save, so a restart is rarely needed. If one is needed, restart only that `pnpm tauri dev` session and say so.
- Do not start the dev app or take screenshots of it unless the user asks.
- **Previewing the app:** when the user asks to run, preview, or check the app, start it from `~/Projects/PatchworkADE` with `pnpm tauri dev`. If a `pnpm tauri dev` session is already running, use that one rather than starting a second, since both need port 1420.
- **Launch from the repo root.** The root `pnpm tauri` script forwards to `patchwork-desktop`, which owns `@tauri-apps/cli`. Do not run `pnpm exec tauri` from the root; it fails with `Command "tauri" not found`. To check the CLI, use `pnpm tauri --version`.

## Editing alongside the dev session

- The user views changes live, so never run destructive git commands that discard or hide working-tree changes: `git reset --hard`, `git checkout -- <file>`, `git stash`, `git clean`, or similar.
- Keep each edit complete and compiling. Hot reload applies every save, so do not leave half-finished files or broken imports between steps.
- Say explicitly when a change needs a dev session restart, a new Cargo or pnpm dependency, or a config change (`tauri.conf.json`, capabilities, Vite config). Hot reload does not pick these up.
- Do not delete, move, or rename files without asking first. Those changes can break the dev session's watchers and the user's view.

## Project layout

- pnpm workspace (`pnpm-workspace.yaml`) with packages under `apps/*`. Root `package.json` scripts use these filters:
  - `patchwork-desktop` (Tauri app): `pnpm tauri dev`, `pnpm build`.
  - `patchwork-web` (frontend): `pnpm typecheck`, `pnpm dev:web`.
- Cargo workspace (`Cargo.toml`) with members `apps/server` and `apps/desktop/src-tauri`.
- Formatting: `pnpm format` and `pnpm format:check` cover `apps/**/*.tsx`. Run `pnpm format` on TSX changes.

## Commands

- Dev app: `pnpm tauri dev` (from the repo root only; see Development setup).
- Web typecheck: `pnpm typecheck`.
- Format TSX: `pnpm format`. Check only: `pnpm format:check`. Both cover `apps/**/*.tsx` and nothing else.
- Rust lint: `cargo clippy --workspace --all-targets`.
- Rust tests: `cargo test --workspace`.
- UI measurement and screenshots against the running dev session: `pnpm qa:stripe`, `pnpm qa:shot <out.png> [selector]`.

## Verification

- Web: `pnpm typecheck`, plus `pnpm format:check` for TSX changes.
- Rust: `cargo clippy --workspace --all-targets` and `cargo test --workspace`.
- Run the checks that match what you changed, and report the output if any fail.

## Tests

- Never write tests. Do not add unit, integration, or frontend tests, test files, test runners, test dependencies, or test scripts, even when a change would benefit from one.
- The root `test` script (`cargo test --workspace`) already exists. You may run it as a check, but do not add to it or add other test scripts.
- Exception: Playwright is allowed, only as the UI-check tool under `scripts/qa/` and its `qa:*` root scripts, for measuring and screenshotting the running dev session. Those scripts must not assert or run as tests. Do not add `*.test.*` files, a test runner, a `test` script, or any Playwright spec file.
- Always push QA scripts you save under `scripts/qa/`. Do not leave them untracked: commit each one with the change it checks and push it with the next push, along with this file's other finished work.

## Committing

- Do not commit or push until the user has reviewed the change and says to. Leave finished work uncommitted in the working tree.
- Do not push at all unless the user explicitly asks.
- Commit subjects are short, sentence-case, imperative-style summaries with no prefix, such as "Nudge the header project dropdown 2px left". Describe the user-visible change.

## UI checks

- For alignment, layout, spacing, or other UI and UX issues, measure and screenshot the running app before changing code, and again after the change. Use the in-repo harness in `scripts/qa/` (see its README): `pnpm qa:stripe` measures the tool stripe, and `pnpm qa:shot <out.png> [selector]` saves a screenshot. Add new checks under `scripts/qa/checks/`.
- The harness connects to the user's running `pnpm tauri dev` session at localhost:1420. Never start the app to run it. If the server is not up, tell the user to start it.
- This rule is the standing permission for those screenshots and measurements, which the "do not take screenshots unless asked" rule above would otherwise block.
- Playwright is a root devDependency, so adding or upgrading it changes `package.json` and `pnpm-lock.yaml`. Say so when you do it.
