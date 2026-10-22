# Patchwork

An agentic development environment built with Rust + Tauri 2 and a React/TypeScript UI with light and dark themes.

Patchwork drives the **Claude Code**, **Codex**, and **Antigravity** CLIs already installed and signed in on your machine. Each task is a conversation with one agent; edits land directly in your project, and you review them in the canvas (Plan / Code / Diff) where **Stage** stages a file (`git add`) and **Reject** restores it.

The sidebar is a tree: **chats** without a project are top-level threads, and each project lists its own tasks (use its **+** to start one). Chats run in a private scratch folder under the app's data directory, which is deleted with the chat.

Conversations you had with an agent outside Patchwork, either in the agent's own CLI or in [T3 Code](https://github.com/pingdotgg/t3code), can be imported (during onboarding, or later in **Settings → Imports**). T3 Code's database is only read, never modified. Each one becomes a task in the project for its folder, with its messages copied in, and follow-ups resume the original CLI session. The **Usage** button at the bottom of the sidebar shows each provider's subscription limits (Claude, Codex, and Antigravity).

State lives in SQLite at `~/.config/patchwork/patchwork.db`, and future config files go in the same folder. On first launch Patchwork moves its data there from the old `~/.config/dev.patchwork.ade` folder, and converts any older `settings.json` / `threads.json` into the database, keeping them as `*.json.bak`.

## Develop

```sh
pnpm install
pnpm tauri dev        # run the app
scripts/build.sh appimage   # target/release/bundle/appimage/Patchwork_amd64.AppImage
scripts/build.sh install    # release binary, icons, and launcher under ~/.local
```

`scripts/build.sh appimage` builds the AppImage under a fixed name, `Patchwork_amd64.AppImage`, so
it doesn't change with the version. `scripts/build.sh install` builds the release binary and
installs it under `~/.local` with its icons and a launcher named **Patchwork**.
Set `PREFIX` to install somewhere else. The launcher's id is
`dev.patchwork.ade`, so it doesn't replace the launcher of an AppImage you already run. The
install step refreshes the desktop, icon, and KDE caches.

Requires Rust, pnpm, and the Tauri Linux prerequisites (webkit2gtk-4.1).

## Tests

```sh
cargo test --workspace                    # unit tests (parsers, importers, persistence, git)
cargo test -p patchwork-server -- --ignored  # live turns against the real agent CLIs
```

## Releases

Changelog entries and release notes come from the commit history with
[git-cliff](https://git-cliff.org/), configured in `cliff.toml`. Each commit is sorted into
a group by the start of its subject line, and commits that start with `Release`, `Bump`,
`Merge`, or `Chore` are left out. Words that don't match a group go under **Other**, so write
commit subjects with a leading verb, for example `Add ...`, `Fix ...`, or `Change ...`.

The tag sets the app version, so you never bump version numbers by hand. The version in the
repo stays at `0.1.0` as a placeholder, which means a local build reports that version.

To cut a release, run the script from a clean `main` that is up to date with `origin/main`:

```sh
scripts/release.sh 0.2.0
```

The script checks the version and the repo state, runs the typecheck, format check, Clippy, and
the Rust tests, and prints the release notes for the new version. After you confirm, it
regenerates `CHANGELOG.md` with a full rewrite (not `--prepend`, because the file already has an
Unreleased section), commits it as `Release v0.2.0`, creates the annotated tag, and pushes `main`
and the tag. Nothing is written until you confirm.

Pushing the tag starts the **Release AppImage** workflow (`.github/workflows/release-appimage.yml`).
Before it builds, the workflow writes the tag's version (`0.2.0`) into `package.json`,
`apps/desktop/package.json`, `apps/desktop/src-tauri/tauri.conf.json`,
`apps/desktop/src-tauri/Cargo.toml`, and `apps/server/Cargo.toml`. It then builds the AppImage on
Ubuntu 22.04, runs `git cliff --latest` for the release notes, and creates a GitHub release named
`Patchwork v0.2.0` with the AppImage attached. Pushes to `main` do not build anything.

To check the build without making a release, run the workflow by hand from the Actions tab
(**Run workflow**). It builds the files as they are, so it uses the placeholder version, and
creates no release.

Don't edit `CHANGELOG.md` by hand, since the next regeneration overwrites it. Tags must start
with `v` followed by a digit, such as `v0.2.0`, because that is the pattern git-cliff and the
workflow both look for.

## Layout

The repo is split into `apps/web`, `apps/server`, and `apps/desktop`:

- `apps/server` — the backend, a Rust crate with no Tauri dependency
  - `src/lib.rs` — `Server`, the operations the UI calls
  - `src/provider/` — one module per agent CLI: detection, models, turn arguments, JSONL parsing
  - `src/runner.rs` — spawns agent CLIs, turns their output into agent events
  - `src/persistence/` — SQLite store and schema migrations
  - `src/import/` — reads each CLI's saved sessions for importing
  - `src/usage/` — subscription usage limits per provider
  - `src/workspace.rs` — git and file access
  - `src/types.rs` — data contract shared with the UI
- `apps/desktop/src-tauri` — the Tauri shell: exposes `Server` as commands and forwards events
- `apps/web` — the React UI (`~/` imports resolve to `apps/web/src`)
  - `src/app/` — app root; `src/api.ts` — typed bridge mirroring `apps/server/src/types.rs`
  - `src/components/{chat,sidebar,usage,settings,onboarding,canvas,workspace}/` — UI
  - `src/hooks/`, `src/lib/` — state and helpers; `src/theme.css` — design tokens

## Data model

Everything the sidebar shows comes from one SQLite database, `patchwork.db`, in the config folder. The store loads it into memory at startup and writes each change through before it is visible (`apps/server/src/persistence/mod.rs`).

| Table | What it holds |
| --- | --- |
| `settings` | App settings as JSON: theme, connected agents, integrations. |
| `projects` | One row per project folder: `name`, `path` (unique), and `position` for sidebar order. |
| `folders` | Sub-folders inside a project: `project_id`, `name`, `position`. Organisation only; no directory is created on disk. |
| `threads` | Tasks and chats. `project_id` NULL makes it a chat. `folder_id` NULL puts it at the top level of its project. `session_id` lets the agent resume. |
| `messages` | The messages of each thread, with text, tool calls, plan, file changes, approvals, and attachments. |
| `app_state` | Small key/value state that is not settings, such as the thread that was open last. |

Where a thread runs:

- **Task:** directly in its project's `path`. There is no copy and no git worktree, so tasks in the same project share one working tree.
- **Chat:** in `chats/<thread id>` under the app's local data directory (`app_local_data_dir`). The folder is deleted with the chat.

Sidebar order is the `position` column. A new thread gets `-created_at`, so the newest is first until it is dragged. Dropping rows calls `reorder_threads`, which numbers that list `0..n`.

Schema changes go in as a new entry appended to `MIGRATIONS` in `apps/server/src/persistence/migrations.rs`. Never edit one that has shipped.

## Agent CLIs

Patchwork has no model access of its own. It runs the `claude`, `codex`, and `agy` CLIs that are
installed and signed in on the machine, and reads some of their files. This section lists every
point where it touches them.

### Claude Code (`claude`)

| | |
| --- | --- |
| Detect | `claude --version`, then `claude auth status`, whose JSON says whether and how it is signed in. Home is `$CLAUDE_CONFIG_DIR` or `~/.claude`. |
| Default model | `$ANTHROPIC_MODEL`, else `model` in `~/.claude/settings.json`. |
| Run | `claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --permission-mode <mode> --permission-prompt-tool stdio [--resume <session>] [--model <m>] [--effort <e>] --append-system-prompt <installed shells>` |
| Access modes | Supervised `default`, Auto-accept edits `acceptEdits`, Auto `auto`, Full access `bypassPermissions`. |
| Prompt | A stream-json `user` line on stdin, with images as base64 blocks. stdin stays open for the turn. |
| Output used | `stream_event` text and reasoning deltas; `assistant` tool calls, token usage, and synthetic command output; `user` tool results; `system` init (session id) and background task start and end; `rate_limit_event` (usage limits); `result` (end of turn, errors). `TodoWrite` becomes the plan, edit tools become touched files, `Task`/`Agent` calls become subagents. |
| Approvals | `control_request` `can_use_tool` becomes an Allow / Deny request, answered with a `control_response`. In Full access every request is allowed at once, except `AskUserQuestion`. |
| Questions | `AskUserQuestion` arrives as `can_use_tool` in every mode. The answers go back in `updatedInput.answers`, by question text. |
| Steer | A `user` line with `"priority": "now"`, taken at the next tool boundary. |
| Subagents | Stopped with a `stop_task` control request. |
| Goals | `/goal <objective>` and `/goal clear` are sent as messages and handled by Claude; its `Goal set` / `Goal active` / `Goal cleared` replies and `Stop hook feedback` checks are read from the stream. Whether a goal was met is read from the session transcript, `~/.claude/projects/*/<session>.jsonl`. |
| Compaction | `/compact` is sent as a message. An idle chat whose context is over the threshold is compacted just before its prompt cache expires (an hour after the last reply). |
| Import | Sessions in `~/.claude/projects/<folder>/*.jsonl`: titles, user and assistant text, session id, and folder. |
| Usage limits | The OAuth token from `~/.claude/.credentials.json` (or the macOS keychain), sent to `api.anthropic.com/api/oauth/usage` and `/profile`; during a turn, `rate_limit_event` updates them. The token is never refreshed or written. |

### Codex (`codex`)

| | |
| --- | --- |
| Detect | `codex --version`, then `codex login status`. Home is `$CODEX_HOME` or `~/.codex`. |
| Default model | `model` in `~/.codex/config.toml`, or in its selected `profile`. |
| Run | `codex app-server`, spoken to in JSON-RPC over stdin and stdout: `initialize`, `initialized`, then `thread/start` or `thread/resume` (with the saved thread id), then `turn/start` with the prompt, attachments, model, and effort. |
| Access modes | Sent with `turn/start` as approval policy / sandbox: Supervised `untrusted` / `readOnly`, Auto-accept edits `on-request` / `workspaceWrite`, Auto `on-request` / `workspaceWrite` with Codex's own reviewer, Full access `never` / `dangerFullAccess`. |
| Output used | `item/started` and `item/completed` for messages, commands (with their output), MCP calls, web searches, and file changes; `item/reasoning/summaryTextDelta` (reasoning); `turn/started`, `turn/plan/updated`, `turn/completed`, `error`; `thread/goal/updated` and `thread/goal/cleared`. Token usage is not read from the stream. |
| Approvals | `item/commandExecution/requestApproval` and `item/fileChange/requestApproval`, answered `accept` or `decline`. Any other request is refused. |
| Questions | `item/tool/requestUserInput`, answered with the picks by question id. |
| Steer | `turn/steer` with the running turn's id. Without one, the message waits for the next turn. |
| Goals | `/goal` commands become `thread/goal/set`, `get`, and `clear`. A new goal is set paused, its first turn starts, and then it is set active. When a turn ends with the goal active, stdin stays open 5 seconds so Codex can start its next turn itself. **Stop** first sets the goal paused. |
| Import | Rollouts in `~/.codex/sessions/**/*.jsonl` and titles from `~/.codex/session_index.jsonl`. |
| Usage limits | The newest `token_count` rate limits in the most recent rollout files. Nothing is fetched. |

### Antigravity (`agy`)

| | |
| --- | --- |
| Install | **Settings** can install it: Patchwork downloads `https://antigravity.google/cli/install.sh` and runs it with `--dir ~/.config/patchwork/clis`. That copy is used before one on `PATH`. |
| Detect | `agy --version`. There is no login check; a run that is not signed in fails. |
| Run | `agy --output-format stream-json <mode> [--conversation=<id>] [--model=<m>] [--effort=<e>] --print=<prompt>`. The prompt is an argument and nothing is written to stdin. |
| Access modes | Supervised no flag (it skips actions that need approval), Auto-accept edits `--mode=accept-edits`, Auto `--mode=auto`, Full access `--dangerously-skip-permissions`. |
| Output used | `init` (conversation id), `step_update` text deltas and tool steps, `result` (errors). The turn ends when the process exits. No reasoning, plan, or token usage is reported. |
| Not available | Approvals, questions, and steering: in `--print` mode it cannot ask anything, so messages sent during a turn wait for the next one. |
| Goals | `/goal <objective>` is sent as the prompt. The goal counts as met when the run ends cleanly, since it does not stop before verifying it. Clearing sends "Stop the current goal." because it has no clear command. |
| Import | Chats saved by the earlier Gemini CLI, under `~/.gemini/tmp/<project>/chats`, with folders from `~/.gemini/projects.json`. |
| Usage limits | `agy -p /usage --output-format json`. |

### T3 Code

Imports only: `~/.t3/userdata/statev2.sqlite` is opened read-only, and its Claude and Codex
threads are copied in with their session ids, so follow-ups resume the original CLI session.
