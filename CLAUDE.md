# Patchwork

Desktop AI coding workspace: a Tauri app with a React frontend and a Rust backend. Run every command from the repo root.

## Layout

- pnpm workspace (`pnpm-workspace.yaml`) with packages under `apps/*`:
  - `patchwork-desktop` (`apps/desktop`): the Tauri app. It owns `@tauri-apps/cli`. Scripts: `pnpm tauri dev`, `pnpm build`.
  - `patchwork-web` (`apps/web`): the frontend. Scripts: `pnpm typecheck`, `pnpm dev:web`.
- Cargo workspace (`Cargo.toml`) with members `apps/server` (providers, persistence, usage, thread import) and `apps/desktop/src-tauri`.
- QA scripts live in `scripts/qa/` (see its README).

## Commands

- Dev app: `pnpm tauri dev` (see Dev session).
- Web typecheck: `pnpm typecheck`.
- Format TSX: `pnpm format`. Check only: `pnpm format:check`. Both cover `apps/**/*.tsx` and nothing else.
- Rust lint: `cargo clippy --workspace --all-targets`.
- Rust tests: `cargo test --workspace` (the root `test` script).
- UI measurement and screenshots of the running dev session: `pnpm qa:activity-bar`, `pnpm qa:shot <out.png> [selector]`.

## Verification

Run the checks that match what you changed, and report the output if any fail.

- Web: `pnpm typecheck`, plus `pnpm format:check` for TSX changes (run `pnpm format` first).
- Rust: `cargo clippy --workspace --all-targets` and `cargo test --workspace`.

## Dev session

- The dev instance is the only app instance that is ever started or restarted. Start it only with `pnpm tauri dev` from the repo root. Never run `pnpm dev` on its own, `target/debug/patchwork`, or other build outputs directly.
- Do not run `pnpm exec tauri` from the root; it fails with `Command "tauri" not found`. The root `pnpm tauri` script forwards to `patchwork-desktop`. To check the CLI, use `pnpm tauri --version`.
- The user usually keeps a `pnpm tauri dev` session open and views changes there. If one is already running, use it rather than starting a second, since both need port 1420.
- When the user asks to run, preview, or check the app, use that session, or start it from `~/Projects/PatchworkADE` with `pnpm tauri dev`.
- You may start or restart it yourself when a change needs it or to verify work. Say so when you do, and check first that no reply is streaming, since a restart ends running agent turns.
- Telling it apart: the dev session (`pnpm tauri dev`, or the Vite server at localhost:1420) draws a yellow and black hazard-stripe frame around the window and sets `data-dev-session="true"` on `<html>`. The installed Patchwork has neither. If a screenshot or window has no stripes, it is not the dev session, so do not judge changes by it.
- Vite hot-reloads UI edits and Rust changes rebuild on save, so a restart is rarely needed. Say explicitly when a change needs a restart, a new Cargo or pnpm dependency, or a config change (`tauri.conf.json`, capabilities, Vite config); hot reload does not pick these up.

## Editing while the user watches live

- Never run destructive git commands that discard or hide working-tree changes: `git reset --hard`, `git checkout -- <file>`, `git stash`, `git clean`, or similar.
- Keep each edit complete and compiling. Hot reload applies every save, so do not leave half-finished files or broken imports between steps.
- Do not delete, move, or rename files without asking first. Those changes can break the dev session's watchers and the user's view.

## UI checks

- For alignment, layout, spacing, or other UI and UX issues, measure and screenshot the running dev session before changing code, and again after. Use the harness in `scripts/qa/`; add new checks under `scripts/qa/checks/`.
- The harness connects to the dev session at localhost:1420. If the server is not up, start it with `pnpm tauri dev` and say so.
- This is the standing permission for those screenshots and measurements. Take screenshots only for UI checks or when the user asks.
- Playwright is a root devDependency, so adding or upgrading it changes `package.json` and `pnpm-lock.yaml`. Say so when you do it.

## Performance checks

Headless Chromium is not the app's renderer. The app runs in WebKitGTK, without GPU compositing (`main.rs` sets `WEBKIT_DISABLE_DMABUF_RENDERER=1`), so animations and scrolling can be smooth in Playwright and choppy in the app. For any lag, jank or "choppy animation" report, measure in the real dev window:

1. Start a throwaway collector outside the repo: a small `node` HTTP server on `127.0.0.1` that sends `Access-Control-Allow-Origin: *` and appends each POST body to a file. The app's CSP is null, so the webview can post to it.
2. Add a temporary block to `apps/web/src/main.tsx`, inside `if (import.meta.env.DEV)`. A few seconds after load, it drives the UI itself, for example by calling `.click()` on the real element. It records `requestAnimationFrame` gaps over the animation, and the synchronous time of the action. Then it POSTs them to the collector. Editing `main.tsx` reloads the dev window, so the block runs on its own.
3. To find the cause, run variants inside the same window: inject a `<style>` per variant to switch one suspect off at a time, such as a background, an overlay, other animations, a pane, or a transition. Compare the frame gaps. Leave the user's state as it was, for example by re-collapsing a folder that started collapsed.
4. Apply the fix, and measure the final CSS or code once more in the real window.
5. Remove the temporary block so `main.tsx` matches git, stop the collector, and delete its files. Tell the user the dev window reloaded and what the block clicked.

WebKitGTK's `/usr/lib/webkit2gtk-4.1/MiniBrowser` can run a static copy of a page under different `WEBKIT_*` env vars. It does not reproduce the app's window, so treat its numbers as a hint, and confirm in the dev window. When stopping these helpers, kill them by PID: `pkill -f <pattern>` also matches the shell that runs it.

## Tests

- Never write tests. Do not add unit, integration, or frontend tests, test files, test runners, test dependencies, or test scripts, even when a change would benefit from one.
- `cargo test --workspace` already exists; you may run it, but do not add to it or add other test scripts.
- Exception: Playwright, only as the UI-check tool under `scripts/qa/` and its `qa:*` root scripts. Those scripts must not assert or run as tests. Do not add `*.test.*` files, a test runner, a `test` script, or any Playwright spec file.

## Git

- Do not commit or push until the user has reviewed the change and says to. Leave finished work uncommitted in the working tree.
- Do not push at all unless the user explicitly asks.
- QA scripts saved under `scripts/qa/` are never left untracked: commit each with the change it checks, and include it in the next push along with the other finished work.
- Commit subjects are short, sentence-case, imperative-style summaries with no prefix, such as "Nudge the header project dropdown 2px left". Describe the user-visible change.

## Finishing

- Do not wait on the user. When the only remaining steps are ones you cannot take, such as the user trying a change in the app, reviewing it, or confirming a result, treat the work as finished: report what was done and what was not, then end the turn. Do not ask for confirmation or keep posting "waiting" messages.
- If something cannot be done without the user, leave it undone and say so once. Never substitute a prompt to the user for the missing step.
- The same applies to a `/goal` whose remaining condition depends only on the user: the goal counts as met once everything else is done.
