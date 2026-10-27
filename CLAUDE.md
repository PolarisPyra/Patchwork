# Patchwork

Tauri desktop app: React frontend, Rust backend. Run everything from the repo root.

## Layout

- pnpm workspace, `apps/*`:
  - `apps/desktop` (`patchwork-desktop`): Tauri app, owns `@tauri-apps/cli`.
  - `apps/web` (`patchwork-web`): frontend.
- Cargo workspace: `apps/server` (providers, persistence, usage, thread import) and `apps/desktop/src-tauri`.
- QA harness: `scripts/qa/` (see its README).

## Commands

| Task | Command |
|---|---|
| Dev app | `pnpm tauri dev` |
| Web typecheck | `pnpm typecheck` |
| Format TSX | `pnpm format` / `pnpm format:check` (only `apps/**/*.tsx`) |
| Rust lint | `cargo clippy --workspace --all-targets` |
| Rust tests | `cargo test --workspace` |
| Rust bench | `cargo bench -p patchwork-server` |
| Flamegraph | `cargo flamegraph -p patchwork-server --bench stream -- --bench <filter>` |
| UI QA | `pnpm qa:activity-bar`, `pnpm qa:shot <out.png> [selector]` |

- For backend perf, use the Criterion benches in `apps/server/benches/`, adding one for a new hot path. No one-off scripts.
- `pnpm exec tauri` from the root fails. Use `pnpm tauri`.

## Verify

- Web changes: run `pnpm typecheck`. For TSX changes, run `pnpm format` and then `pnpm format:check`.
- Rust changes: run clippy and `cargo test --workspace`.
- Report the output of any check that fails.

## Dev session

- Only ever run the app with `pnpm tauri dev`. Never use `pnpm dev`, `target/debug/patchwork`, or other build outputs.
- I usually have a session open. Check with `ss -ltnp | grep 1420`. If one is running, reuse it, since it holds port 1420.
- If none is running, start one in the background. You may also restart it when needed. Say when you do either, and never restart while a reply is streaming.
- Stop only sessions you started. Kill the whole process tree by PID, then confirm port 1420 is free. Never stop mine.
- The dev window has a yellow and black hazard-stripe frame and `data-dev-session="true"`. No stripes means it isn't the dev session, so don't judge changes by it.
- Hot reload covers UI and Rust edits. Say when a change needs a restart instead: a new dependency, `tauri.conf.json`, capabilities, or the Vite config.

## While I'm watching

- No destructive git commands: no `reset --hard`, `checkout -- <file>`, `stash`, or `clean`.
- Every save must compile, because hot reload applies it.
- Ask before deleting, moving, or renaming files.

## Design

- Cut taglines, helper text, and secondary labels. If a screen needs them, fix the layout.
- Pick what improves UX, DX, and agent experience without breaking anything.

## UI checks

- Never write throwaway `.mjs`/`.js` files for QA: not in the repo, not in `scripts/`, not anywhere else.
- For layout, spacing, or alignment issues, screenshot and measure the dev session before and after the change, using only the existing `pnpm qa:*` scripts.
- If they can't measure what you need, extend an existing check in `scripts/qa/checks/` with an argument or option instead of adding a new file.
- Add a new check file only when I ask for one or it will clearly be reused. Say so when you do.
- Screenshots go where `qa:shot` puts them. Don't leave extra images or logs around.
- You have standing permission to take these screenshots. Take them only for UI checks or when I ask.
- Say so if you add or upgrade Playwright, since it changes `package.json` and the lockfile.

## Performance checks

The app renders in WebKitGTK, not Chromium, so Playwright numbers don't count. Measure lag in the real dev window:

1. Run a collector inline with `node -e` in the background, not as a saved file. It should listen on `127.0.0.1`, send CORS `*`, and print each POST body to stdout. Read the results from its output.
2. Add a temporary `import.meta.env.DEV` block to `apps/web/src/main.tsx`. It should click the real element, record `requestAnimationFrame` gaps and how long the action takes, then POST the results to the collector.
3. To find the cause, inject a `<style>` per suspect to turn one thing off at a time, and compare the frame gaps. Put the UI back the way you found it.
4. Fix the cause, then measure again.
5. Revert `main.tsx` and kill the collector by PID (not `pkill -f`). `git status` should show no new files. Tell me the window reloaded and what got clicked.

`/usr/lib/webkit2gtk-4.1/MiniBrowser` can test `WEBKIT_*` env vars, but its numbers are only a hint.

## Tests

- Never write tests: no test files, runners, dependencies, or scripts.
- You may run `cargo test --workspace`, but don't add to it.
- Exception: Playwright under `scripts/qa/` as a measuring tool. It never asserts, has no spec files, and no `*.test.*` files.

## Git

- Don't commit until I've reviewed the change and say so. Don't push unless I ask.
- When a QA check is added or changed, commit it along with the change it checks. Never leave one untracked.
- Commit subjects are short, sentence-case, and imperative, with no prefix. Describe the user-visible change, for example "Nudge the header project dropdown 2px left".

## Finishing

- When only my steps are left (trying the change, reviewing, confirming), report what was done and what wasn't, then end the turn. Don't wait or ask for confirmation.
- The same applies to a `/goal`: it's met once everything left depends only on me.