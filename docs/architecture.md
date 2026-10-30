# Architecture

## Repo layout

The repo is split into `apps/web`, `apps/server`, and `apps/desktop`:

- `apps/server` — the backend, a Rust crate with no Tauri dependency
  - `src/lib.rs` — the module list, `Server`, and how it opens. Every other folder is one domain, holding its wire types (`types.rs`, re-exported from the folder), its `impl Server` operations (`ops.rs` where it has them), and its logic:
  - `src/threads/` — threads and their messages; `src/projects/` — projects and their folders; `src/settings/` — the settings
  - `src/provider/` — one module per agent CLI (`claude/`, `codex/`, `antigravity.rs`, `opencode/`, `api/`), finding and probing them (`locate.rs`, `probe.rs`), what a turn sends them (`turn.rs`) and reads back (`parsed.rs`), and handing each one its hooks (`hooks.rs`)
  - `src/orchestration/` — runs turns, grouped by feature, one folder each, with `orchestration.rs` as the index: `turn/` (the `Runner`, starting and ending a turn, its process, turns cut off by a restart), `messages/` (sending, steering and queueing), `output/` (reading agent output into agent events and streaming them), `approvals/`, `changes/` (which files each reply changed, and per-turn checkpoints), `tasks/` (background tasks and subagents), and `goal/`
  - `src/permissions/` — the rules Patchwork answers approvals by, and the permission hook Codex and Antigravity run
  - `src/plugins/` — plugin bundles and their hooks, with the built-in hooks Patchwork carries (`builtin/`)
  - `src/persistence/` — SQLite store and schema migrations
  - `src/import/` — reads each CLI's saved sessions for importing
  - `src/usage/` — subscription usage limits per provider
  - `src/scheduled/` — scheduled tasks and the next run of each, from its shape and the local clock
  - `src/computer/` — a screenshot, and the pointer and keyboard under it
  - `src/workspace/` — git and file access; `src/watch/` — watches the open project for file changes (see [File changes](#file-changes))
  - `src/system/` — the user's machine: the programs Patchwork starts and their environment, login shells, paths, opening files on the desktop
  - `src/logging/` — the debug logs
- `apps/desktop/src-tauri` — the Tauri shell: exposes `Server` as commands and forwards events
- `apps/web` — the React UI (`~/` imports resolve to `apps/web/src`)
  - `src/app/` — app root; `src/api/` — typed bridge to the backend, one area per file
  - `src/features/{chat,sidebar,usage,updates,settings,onboarding,canvas,panel,workspace}/` — UI, one folder per feature
  - `src/features/chat/components/Artifact.tsx` — Mermaid diagrams and sandboxed HTML/SVG artifacts in a reply
  - `src/components/`, `src/hooks/`, `src/lib/` — what two or more features share; `src/theme.css` — design tokens

## Data model

Everything the sidebar shows comes from one SQLite database, `patchwork.db`, in the data folder. The store loads it into memory at startup and writes each change through before it is visible (`apps/server/src/persistence/mod.rs`).

| Table | What it holds |
| --- | --- |
| `settings` | App settings as JSON: theme, connected agents, integrations. |
| `projects` | One row per project folder: `name`, `path` (unique), and `position` for sidebar order. |
| `folders` | Sub-folders inside a project: `project_id`, `name`, `position`. Organisation only; no directory is created on disk. |
| `threads` | Tasks and chats. `project_id` NULL makes it a chat. `folder_id` NULL puts it at the top level of its project. `session_id` lets the agent resume. |
| `messages` | The messages of each thread, with text, tool calls, plan, file changes, approvals, and attachments. |
| `scheduled_tasks` | Prompts that start a task on a schedule: its shape as JSON, the project, agent and access mode to run with, and when it last and next runs. |
| `app_state` | Small key/value state that is not settings, such as the thread that was open last. |

Where a thread runs:

- **Task:** directly in its project's `path`. There is no copy and no git worktree, so tasks in the same project share one working tree.
- **Chat:** in `chats/<thread id>` under the app's local data directory (`app_local_data_dir`). The folder is deleted with the chat.

Sidebar order is the `position` column. A new thread gets `-created_at`, so the newest is first until it is dragged. Dropping rows calls `reorder_threads`, which numbers that list `0..n`.

Schema changes go in as a new entry appended to `MIGRATIONS` in `apps/server/src/persistence/migrations/mod.rs`. Never edit one that has shipped.

## File changes

The open project is watched for changes, and the UI never polls git.

- **Watching** (`apps/server/src/watch/`). On Linux, inotify watches one folder at a time, so `linux.rs` adds a watch for each folder git keeps. On macOS and Windows, FSEvents and ReadDirectoryChangesW cover a whole tree with one watch, so `tree.rs` watches the project root. On every platform, `filter.rs` drops paths git ignores, `node_modules`, `target`, and everything in `.git` below its top level. The top level is kept because commits, staging, and checkouts change it.
- **Refreshing.** Events are merged until 250 ms pass without one, or for at most 1 s. Then `Server::watch_project` runs one `git status`. It runs one `git diff HEAD --numstat` for line counts only when a tracked file changed. The result goes to the UI as a `project-changed` event. Git runs with `GIT_OPTIONAL_LOCKS=0`, so its own reads do not rewrite `.git/index` and trigger another refresh.
- **Which files a reply changed** (`apps/server/src/orchestration/changes/attribution.rs`).
  - **Edit tools.** They name their files, and those files are listed as soon as the tool reports them.
  - **Other changes.** Changes made any other way, such as a shell `rm`, are found by comparing `git status` with how it stood when the reply started.
  - **Who made a change.** No platform reports which process wrote a file. A change therefore counts only if it happened while one of the reply's calls that could write it was running:
    - A read or search call writes nothing.
    - An edit writes only the files it names.
    - A shell command can write anything, and a background command keeps that window open until it ends. Each window gets 1 s of grace after it closes, because FSEvents can report late.
  - **Two replies at once.** A file that two replies could claim goes to the reply whose call named it: an edit of that file, or a command that mentions its path or name.
  - **Late reports.** Some agents report a call only after it finishes. OpenCode in `run` mode is one, and it gives the call's real start time. A change that no call covers yet is held until one does, or until the reply ends.
  - **Projects that are not open.** Their files are not watched, so their replies are settled by the `git status` comparison alone when the turn ends.
- **What attribution still cannot separate.** These cases would need the writing process's ID:
  - A file the user saves while the agent's shell command is running.
  - A file that two replies' concurrent shell commands both could have written, when neither command names it.
  - Writes from a process an agent left running outside a tool call and outside a background task it reported.
- **Deleted files.** The reply's file list and the Changes view strike these through. A file with a git status of `deleted` shows its line counts. A deleted file git never tracked has no status, so the UI asks `missing_files` and labels it "Deleted".

## Config location

`apps/server/src/system/dirs.rs` splits Patchwork's files by who owns them, each in a `patchwork` folder inside the platform's own:

| Holds | Linux | macOS | Windows |
|---|---|---|---|
| Config: what the user edits, `skills/`, `agents/`, `plugins/`, `mcp/` (one `<name>.json` per MCP server), `providers.json` | `~/.config/patchwork` | `~/Library/Application Support/patchwork` | `%APPDATA%\patchwork` |
| Data: what Patchwork keeps, `patchwork.db` and `api-sessions/` | `~/.local/share/patchwork` | `~/Library/Application Support/patchwork/data` | `%LOCALAPPDATA%\patchwork` |
| Cache: what Patchwork rebuilds, `launch/<cli>/` (the skills, agents and hooks each CLI is launched with), `model-manifest.json`, `mcp-cli-tools.json`, `screenshots/` | `~/.cache/patchwork` | `~/Library/Caches/patchwork` | `%LOCALAPPDATA%\patchwork\cache` |
| State: `mcp-relay.json` | `~/.local/state/patchwork` | the data folder | the data folder |
| Logs, one folder per launch | `~/.local/state/patchwork/logs` | `~/Library/Logs/patchwork` | `%LOCALAPPDATA%\patchwork\logs` |

macOS has one folder for config and data, and Windows one for data and cache, so the second is a subfolder of the first. Chats' scratch folders and pasted images stay in the app-identifier data folder.

On launch, `Dirs::adopt_old_layout` moves what older versions kept in the config folder to its new folder and deletes what nothing reads any more; something already in its new place wins. Patchwork also moves its database from the older `~/.config/dev.patchwork.ade` folder, and converts any older `settings.json` / `threads.json` into the database, keeping them as `*.json.bak`.
