# Patchwork

Tauri desktop app: React frontend, Rust backend. Run everything from the repo root.

## Layout

- pnpm workspace, `apps/*`:
  - `apps/desktop` (`patchwork-desktop`): Tauri app, owns `@tauri-apps/cli`.
  - `apps/web` (`patchwork-web`): frontend.
- Cargo workspace: `crates/server` (providers, persistence, usage, thread import), `crates/api` (the hosted account service, on Postgres; see `docs/accounts.md`), `apps/desktop/src-tauri`, and `commercial/crates/licensing`.
- `commercial/`: the paid features under the commercial license, as `commercial/crates/licensing` (plans and license keys) and `commercial/web` (`@patchwork/commercial-web`, their UI). See `docs/licensing.md`.

### `apps/web/src`

- `features/<feature>/` is one piece of UI (`chat`, `sidebar`, `settings`, `canvas`, `panel`, `workspace`, …). Its entry component sits at the root (`features/chat/Chat.tsx`), with `components/`, `hooks/`, `lib/` (pure, React-free) and `types.ts` beside it as needed.
- A sub-feature takes the same shape one level down: `features/chat/{composer,transcript,response,lineage,markdown,plan,prompts}/`, `features/settings/{providers,agents,app,system}/`.
- `components/`, `hooks/` and `lib/` at the top hold only what two or more features use: building blocks such as `Spinner`, `Dropdown`, `ProviderLogos`, and shared state and helpers. They never import from `features/`.
- New code goes in the feature that uses it. Move it up to the top level once a second feature needs it, and down into the feature when only one is left.
- Open a `components/`, `hooks/` or `lib/` folder only once it would hold four or more files. Until then, the files sit at the feature root.
- A folder past about 12 files splits into sub-features. Move a file into a sub-feature only if that sub-feature is its only user; otherwise it stays at the feature root.
- Split a large component into a sub-feature folder, not into loose files beside it.
- Import with the `~/` alias, not relative paths. A web worker is the exception: keep it beside the module that loads it with `new URL("./x.worker.ts", import.meta.url)`.
- `api/` is the bridge to the backend, one module per area (`threads`, `turns`, `projects`, `mcp`, …) with its commands, events and UI-side shapes. Add a command to its area's `xxxApi` object; `api/index.ts` joins them into `api`, so callers keep importing from `~/api`.
- One feature uses another only through its entry component or its `index.ts` (`~/features/sidebar`, `~/features/chat`), never its `components/`, `hooks/` or `lib/`. A piece two features need moves up to the top-level folders instead.

### `crates/server/src`

- Folders are domains, not layers. `lib.rs` holds only the module list, `Server`, and `Server::open`; every other module is one domain (`threads`, `projects`, `settings`, `permissions`, `plugins`, `scheduled`, `provider`, `orchestration`, …) and holds its wire types, its `impl Server` operations, and its logic, so a type is `crate::threads::Thread`. A folder domain splits them into `types.rs` and `ops.rs`; a one-file domain such as `projects.rs` keeps the same order: wire types first, then the operations, then the logic. There is no `types/` or `server/` folder. A helper another domain calls is `pub(crate)`.
- Machine-level helpers that no domain owns live in `system/`: the command and environment programs run with, login shells, the home folder and path resolution, opening files on the desktop.
- A domain's files may reach down to `system/`, `logging/` and other domains' types, never up into `orchestration/` or a `Server` operation; `persistence/` stores what it is given and decides nothing.
- An area that exists on more than one side keeps the same name on each: `threads/`, `src-tauri/src/commands/threads.rs`, `api/threads.ts`.
- A module is one file until it passes about 600 lines or has two clear concerns. Then it becomes `name/mod.rs` plus one file per concern, and `mod.rs` re-exports the public items so callers keep their paths: `provider/claude/{config,input,parse,launch,cli}.rs`, `harness/{agent,tools}`, `plugins/{manifest,agent_hooks,turn_hooks,builtin}`.
- `mod.rs` holds docs, `mod` and `use` lines, and small code the whole folder shares (under about 100 lines).
- Don't create a file under about 40 lines. Put the code in its only user, or in `mod.rs`. Test fixtures go in the module's `tests.rs`.

## Commands

| Task | Command |
|---|---|
| Dev app | `pnpm tauri dev` |
| All static checks | `pnpm check` (regenerates the types, then typecheck, format, rustfmt, clippy) |
| Wire types | `pnpm types` writes `apps/web/src/bindings` from the Rust types in `crates/server` |
| Web typecheck | `pnpm typecheck` |
| Format TS and TSX | `pnpm format` / `pnpm format:check` (`{apps,commercial}/**/*.{ts,tsx}`, not the generated `bindings/`) |
| Rust format | `cargo fmt --all` / `cargo fmt --all -- --check` |
| Rust lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Rust tests | `cargo test --workspace` |

- `pnpm exec tauri` from the root fails. Use `pnpm tauri`.

## Verify

- Web changes: run `pnpm typecheck`. For TS and TSX changes, run `pnpm format` and then `pnpm format:check`.
- Rust changes: run `cargo fmt --all`, then clippy with `-D warnings` (no warnings left behind), then `cargo test --workspace`.
- Report the output of any check that fails.

## Rust and Tauri

- Keep logic in `crates/server`. `src-tauri` is thin glue that only wires up commands, windows, and state.
- No `unwrap()`/`expect()` in app code: return errors instead. Tests are the exception.
- Use the project's logger, not `println!`/`eprintln!`.
- New `#[tauri::command]`: three edits. The server operation, the wrapper in the `commands/` module for its area with its line in `generate_handler!`, and the method in `api/<area>.ts`. App commands are not gated by `capabilities/` (`build.rs` declares no app manifest), so a new one needs no permission there.
- Commands return `Result<T, E>` where `E` serializes to a readable message. Never panic in a command.
- Make commands `async`, and move heavy or blocking work to `spawn_blocking`. Never block the main thread.
- JS `invoke()` takes camelCase arguments, which Tauri maps to snake_case in Rust. Keep it that way, and don't add renames to fight it.
- Stream output to the frontend with `tauri::ipc::Channel`, not global events.
- Keep capabilities minimal. Never widen a permission or use a wildcard just to make something work.
- Wire types the frontend reads come from `crates/server` through `pnpm types`, so change the Rust type and run `pnpm types`. Give a new public serde type `#[derive(TS)]` and `#[ts(export, export_to = "<domain>.ts")]`, using its server domain: ts-rs writes one file per domain (`cargo test` does too, so the files stay current), and `api/types.ts` imports it from `~/bindings/<domain>`. A hook check Patchwork carries itself is a `BuiltinHook` in `plugins/builtin/`, which a plugin names as `{ "type": "builtin", "name": … }`; it never matches on a command's raw text, it reads it with `plugins/builtin/shell.rs`. The frontend-only fields of a message or thread are added in `apps/web/src/api/types.ts`.

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
- Every new feature ships full accessibility, so screen readers and the agent-desktop skill can read and use all of it:
  - Every control has a name: icon buttons get `aria-label`, inputs a `<label>` or `aria-label`.
  - Use real elements (`button`, `a`, `input`). Anything else that acts gets a `role`, `tabIndex={0}`, and Enter and Space handling.
  - Everything the mouse can do, the keyboard can too: menus, tabs, drag handles and resize separators (arrow keys, `aria-valuenow`).
  - States are exposed: `aria-expanded`, `aria-selected`, `aria-pressed`, `aria-checked`, `aria-current`, `aria-disabled`, and `aria-haspopup` on what opens a menu.
  - Dialogs have `role="dialog"`, `aria-modal`, a name, and close on Escape. Regions and lists have names.
  - Errors use `role="alert"`, and progress or status that changes uses `role="status"`.
  - Controls that show only on hover use the `hover-reveal` class, so they stay in the accessibility tree and show on focus.
- Every spinner turns in step. Use `apps/web/src/components/Spinner.tsx`, which pins each arc's animation to the page's timeline, also when it shows again after being hidden. Never add a rotating icon or a spinner with its own animation.

## Scripts

- Never add scripts: no shell, Node, Python, or other script files, no QA checks or harnesses, and no new `package.json` scripts. Not in the repo, not in `scripts/`, and not anywhere else. Run what a task needs inline instead.
- There is no QA harness. For a UI change, say what to look at in the dev window, and I check it.

## Performance checks

Measure lag in the real dev window:

1. Run a collector inline with `node -e` in the background, not as a saved file. It should listen on `127.0.0.1`, send CORS `*`, and print each POST body to stdout. Read the results from its output.
2. Add a temporary `import.meta.env.DEV` block to `apps/web/src/main.tsx`. It should click the real element, record `requestAnimationFrame` gaps and how long the action takes, then POST the results to the collector.
3. To find the cause, inject a `<style>` per suspect to turn one thing off at a time, and compare the frame gaps. Put the UI back the way you found it.
4. Fix the cause, then measure again.
5. Revert `main.tsx` and kill the collector by PID (not `pkill -f`). `git status` should show no new files. Tell me the window reloaded and what got clicked.

## Tests

- Never write tests: no test files, runners, dependencies, or scripts.
- You may run `cargo test --workspace`, but don't add to it.

## Git

- Don't commit until I've reviewed the change and say so. Don't push unless I ask, every time: a commit I asked for is not a push, so leave the work committed on my machine and say it is unpushed.
- "Push to git", or just "push", is that ask, and it covers the whole job: stage and commit anything uncommitted, with its `Changelog:` paragraph, then push the branch to its remote. Don't ask me again, and don't wait for the review first. Say what went up and whether anything was left out; if there was nothing to commit or push, say that instead.
- One plain push is all it means. Force-pushing, pushing or moving a tag, rewriting pushed history, and pushing a branch I did not name are each their own request, so never fold one into "push".
- Releases go through a pull request. Every push to `main` runs CI, and when it passes, the Release workflow checks whether a commit since the last tag has a `Changelog:` paragraph other than `none`; if one does, it puts the next version and its notes on the `release` branch and keeps the "Release vYY.M.N" pull request up to date. Merging that pull request releases it: the run after the merge tags and publishes it. So "push" never releases, and you never bump the version. Merge it only when I ask to release, and only with a merge commit: `gh pr merge release --merge` (squash and rebase are off, since they would release later commits without their notes). Versions are CalVer `YY.M.N`: the two-digit year, the month, and which release of that month (26.11.4 → 26.11.5, then 26.12.0 in a new month). Don't work a number out yourself: `scripts/release.sh next` prints it.
- Only "release now" (or "bump ver", "push and release") asks for a release that can't wait: commit and push the work, then run `scripts/release.sh bump`, which counts the version from the newest tag, sets it, writes the notes, tags, and pushes.
- Commit subjects are short, sentence-case, and imperative, with no prefix. Describe the user-visible change, for example "Nudge the header project dropdown 2px left".
- `CHANGELOG.md` is user-facing: the app shows it in Settings → Updates and it is the text of each release. Every commit that changes the app needs a paragraph in its message starting with `Changelog:` and its group (Added, Changed, Fixed, Removed, or Security), written for users by the rules in `docs/changelog-style.md`: what they see or can do now, where, and what is different, for example "Changelog: Changed: Every loading indicator now uses the steady spinner from the thread list: plan steps, background tasks, … The old one rotated and looked like it shook." A change users can't notice gets `Changelog: none`. The `commit-msg` hook enforces both; if it turns a commit down, fix the message rather than bypassing the hook. At release, these paragraphs are the release's notes, word for word (see `docs/release.md`), so a follow-up to a change in the same release gets `Changelog: none`.
- To fix notes that were already released, edit that release's section of `CHANGELOG.md`, commit it, and run `scripts/release.sh notes <version>`. Never rewrite pushed history for it.
- Keep internal CI/CD, release, and tooling changes out of the changelog. Commit them separately from app changes, so they only touch `.github/`, `scripts/`, `docs/`, or `*.md`, which need no `Changelog:` paragraph. If an internal change has to touch app files (such as the Vite config or the dev session), write `Changelog: none`.
- Never watch GitHub Actions runs unless I ask. When releasing, run `scripts/release.sh bump` or `scripts/release.sh YY.M.N` in the foreground. It ends once it has pushed the tag; give me the run's link and end the turn.
- When releasing, the script writes, checks, and releases the notes without asking. Run it in the foreground and include the notes in your reply.

## Finishing

- When only my steps are left (trying the change, reviewing, confirming), report what was done and what wasn't, then end the turn. Don't wait or ask for confirmation.
- The same applies to a `/goal`: it's met once everything left depends only on me.