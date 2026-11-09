# Architecture

## Repo layout

The repo is split into `crates/server` and `apps/desktop`:

- `crates/server` — the backend, a Rust crate with no UI dependency
  - `src/lib.rs` — the module list, `Server`, and how it opens. Every other module is one domain, holding its wire types, its `impl Server` operations and its logic: a folder domain splits them into `types.rs` (re-exported from the folder) and `ops.rs`, and a one-file domain keeps them in that order:
  - `src/threads/` — threads and their messages; `src/projects.rs` — projects and their folders; `src/settings.rs` — the settings
  - `src/provider/` — one module per agent CLI (`claude/`, `codex/`, `antigravity.rs`, `opencode/`, `api/`), finding and probing them (`locate.rs`, `probe.rs`), what a turn sends them (`turn.rs`) and reads back (`parsed.rs`), and handing each one its hooks (`hooks.rs`)
  - `src/orchestration/` — runs turns, grouped by feature, one folder each, with `orchestration/mod.rs` as the index: `turn/` (the `Runner`, starting and ending a turn, its process, turns cut off by a restart), `messages/` (sending, steering and queueing), `output/` (reading agent output into agent events and streaming them), `approvals/`, `changes/` (which files each reply changed, and per-turn checkpoints), `tasks/` (background tasks and subagents), and `goal/`
  - `src/permissions/` — the rules Patchwork answers approvals by, and the permission hook Codex and Antigravity run
  - `src/plugins/` — plugin bundles and their hooks, with the built-in hooks Patchwork carries (`builtin/`)
  - `src/persistence/` — SQLite store and schema migrations
  - `src/import/` — reads each CLI's saved sessions for importing
  - `src/usage/` — subscription usage limits per provider
  - `src/scheduled.rs` — scheduled tasks and the next run of each, from its shape and the local clock
  - `src/computer.rs` — a screenshot, and the pointer and keyboard under it
  - `src/workspace/` — git and file access; `src/watch.rs` — watches the open project for file changes (see [File changes](#file-changes))
  - `src/system/` — the user's machine: the programs Patchwork starts and their environment, login shells, paths, opening files on the desktop
  - `src/logging/` — the debug logs
  - `src/backgrounds.rs` — chat backgrounds
- `apps/desktop` — the app, drawn with GPUI (package `patchwork`). It calls `Server` directly and applies its agent events on the UI thread.
  - `src/main.rs` — opens the server and the window; `src/workspace/` — the window: its toolbar, regions and resizing
  - `src/{chat,sidebar,settings,canvas,panel,onboarding}/` — UI, one folder per feature
  - `src/ui.rs` — the controls the features share; `src/theme.rs` — the colours and sizes; `src/assets.rs` — the icons and fonts it embeds from `assets/`

## Data model

Everything the sidebar shows comes from one SQLite database, `patchwork.db`, in the data folder. The store loads it into memory at startup and writes each change through before it is visible (`crates/server/src/persistence/store.rs`).

| Table | What it holds |
| --- | --- |
| `settings` | App settings as JSON: theme, connected agents, integrations. |
| `projects` | One row per project folder: `name`, `path` (unique), and `position` for sidebar order. |
| `folders` | Sub-folders inside a project: `project_id`, `name`, `position`. Organisation only; no directory is created on disk. |
| `threads` | Tasks and chats. `project_id` NULL makes it a chat. `folder_id` NULL puts it at the top level of its project. `session_id` lets the agent resume. |
| `messages` | The messages of each thread, with text, tool calls, plan, file changes, approvals, attachments, and what the project's checks made of the turn. |
| `scheduled_tasks` | Prompts that start a task on a schedule: its shape as JSON, the project, agent and access mode to run with, and when it last and next runs. |
| `app_state` | Small key/value state that is not settings, such as the thread that was open last. |

Where a thread runs:

- **Task:** directly in its project's `path`, so tasks in the same project share one working tree. "New task in a worktree" (a project's menu, or the palette) runs `git worktree add -b patchwork/<id>` into `worktrees/<repo>-<id>` under the app's local data directory and adds it as a project of its own, so its tasks work apart. Removing that project removes the worktree, refused while it has changes not committed; the branch stays.
- **Chat:** in `chats/<thread id>` under the app's local data directory (`app_local_data_dir`). The folder is deleted with the chat.

Sidebar order is the `position` column. A new thread gets `-created_at`, so the newest is first until it is dragged. Dropping rows calls `reorder_threads`, which numbers that list `0..n`.

Schema changes go in as a new entry appended to `MIGRATIONS` in `crates/server/src/persistence/migrations/runner.rs`. Never edit one that has shipped.

## File changes

The open project is watched for changes, and the UI never polls git.

- **Watching** (`crates/server/src/watch.rs`). On Linux, inotify watches one folder at a time, so its `linux` module adds a watch for each folder git keeps. On macOS and Windows, FSEvents and ReadDirectoryChangesW cover a whole tree with one watch, so its `tree` module watches the project root. On every platform, its `Filter` drops paths git ignores, `node_modules`, `target`, and everything in `.git` below its top level. The top level is kept because commits, staging, and checkouts change it.
- **Refreshing.** Events are merged until 250 ms pass without one, or for at most 1 s. Then `Server::watch_project` runs one `git status`. It runs one `git diff HEAD --numstat` for line counts only when a tracked file changed. The result goes to the UI as a `project-changed` event. Git runs with `GIT_OPTIONAL_LOCKS=0`, so its own reads do not rewrite `.git/index` and trigger another refresh.
- **Which files a reply changed** (`crates/server/src/orchestration/changes/attribution.rs`).
  - **Edit tools.** They name their files, and those files are listed as soon as the tool reports them.
  - **Other changes.** Changes made any other way, such as a shell `rm`, are found by comparing `git status` with how it stood when the reply started.
  - **Who made a change.** No platform reports which process wrote a file. A change therefore counts only if it happened while one of the reply's calls that could write it was running:
    - A read or search call writes nothing.
    - An edit writes only the files it names.
    - A shell command can write anything, and a background command keeps that window open until it ends. Each window gets 1 s of grace after it closes, because FSEvents can report late.
  - **Two replies at once.** A file that two replies could claim goes to the reply whose call named it: an edit of that file, or a command that mentions its path or name. When two shell commands could both have written it and neither names it, it goes to neither rather than to both: one file under two replies reads as work each of them did. It still shows in the sidebar's Changes view, which lists the project as git sees it.
  - **Late reports.** Some agents report a call only after it finishes. OpenCode in `run` mode is one, and it gives the call's real start time. A change that no call covers yet is held until one does, or until the reply ends.
  - **Projects that are not open.** Their files are not watched, so their replies are settled by the `git status` comparison alone when the turn ends.
- **What attribution still cannot separate.** These cases would need the writing process's ID:
  - A file the user saves while the agent's shell command is running.
  - Which of two replies' concurrent shell commands wrote a file when neither names it, so neither reply lists it.
  - Writes from a process an agent left running outside a tool call and outside a background task it reported.
- **Deleted files.** The reply's file list and the Changes view strike these through. A file with a git status of `deleted` shows its line counts. A deleted file git never tracked has no status, so the UI asks `missing_files` and labels it "Deleted".

## Checks

A reply can say whether the project's own tests still pass, so a change is judged by something
other than how it reads. It is off by default and turned on in Settings → Checks
(`Settings::checks`), because running a test suite after every task is the user's call, not the
app's. There is one default for projects that have not answered, and a project can answer for
itself: a suite too slow to run every time can be off while the others are on.

- **What runs** (`crates/server/src/checks/detect.rs`). The first of: a `check` script in `package.json`, else its `test` script, run with the package manager the lockfile names when that tool is installed; `cargo test`; `python -m pytest`; `go test ./...`; a Makefile's `check` or `test` target; `mix test`. A project with none of them has nothing to check, and its replies show no card at all.
- **When** (`crates/server/src/orchestration/turn/end.rs`). When a turn finishes with `Complete` and its reply changed at least one file. A stopped or failed turn changed nothing to judge.
- **How** (`crates/server/src/checks/run.rs`). Through the same shell an agent's commands run in, in the project folder, with `CI=1` and colour off. Output keeps its last 16 KB; the process group is killed at 5 minutes, and that is never a pass.
- **What it means** (`TurnChecks` in `crates/server/src/threads/messages.rs`). `passed`, `failed`, `timedOut`, or `notRun`, with the command, how long it took, one `CheckFailure` per thing that failed (`crates/server/src/checks/parse.rs` reads cargo's, Node's, Jest's, Vitest's, pytest's and Go's shapes), and the tail of the output. A check that did not finish is `timedOut` or `notRun`, never `passed`. A check left running when the app closed is `notRun` at the next start (`turn/recovery.rs`).
- **What a reply shows** (`apps/desktop/src/chat/checks.rs`). Under the files a reply changed: the outcome, each failure by name, and the output behind a disclosure. Its button runs the command again; its footer turns the project's checks on or off without leaving the chat. A reply shows a card only when checks ran or are running for it, so with them off there is no card at all: the switch is in Settings → Checks, which is one place to look rather than a card in every reply saying it did not check. The result arrives as a `checksRecorded` event, saved on the message like an approval.
- **Asking another way**. The command palette's "Run checks for the open task" runs them for the reply on screen, whether or not the project has them on, and the card it produces is where they can be turned on.

## Approvals and questions

An agent that wants permission, or an answer, sends a request the turn then waits on
(`crates/server/src/orchestration/approvals/`). The user's answer normally goes back down the
process's stdin, or through the hook waiter, and the turn carries on.

- **When no turn is waiting** (`answer.rs`, `answer_in_new_turn`). The app closed under the request,
  or the reply it belonged to was stopped, and the request is still on screen above the composer.
  The answer is recorded on the stored approval either way, so the card stops asking. For a
  question, its answer is also sent as a message from the user — "My answer to …: …" — which starts
  a new turn. The agent it reaches may be a new session, which is why the answer repeats the
  question it belongs to.
- **A permission request is not resent.** There is no call left to allow, so answering one only
  records the answer.

## Config location

`crates/server/src/system/dirs.rs` splits Patchwork's files by who owns them, each in a `patchwork` folder inside the platform's own:

| Holds | Linux | macOS | Windows |
|---|---|---|---|
| Config: what the user edits, `skills/`, `agents/`, `plugins/`, `mcp/` (one `<name>.json` per MCP server), `providers.json` | `~/.config/patchwork` | `~/Library/Application Support/patchwork` | `%APPDATA%\patchwork` |
| Data: what Patchwork keeps, `patchwork.db` and `api-sessions/` | `~/.local/share/patchwork` | `~/Library/Application Support/patchwork/data` | `%LOCALAPPDATA%\patchwork` |
| Cache: what Patchwork rebuilds, `launch/<cli>/` (the skills, agents and hooks each CLI is launched with), `mcp-cli-tools.json`, `screenshots/` | `~/.cache/patchwork` | `~/Library/Caches/patchwork` | `%LOCALAPPDATA%\patchwork\cache` |
| State: `mcp-relay.json` | `~/.local/state/patchwork` | the data folder | the data folder |
| Logs, one folder per launch | `~/.local/state/patchwork/logs` | `~/Library/Logs/patchwork` | `%LOCALAPPDATA%\patchwork\logs` |

macOS has one folder for config and data, and Windows one for data and cache, so the second is a subfolder of the first. Chats' scratch folders and pasted images stay in the app-identifier data folder.

On launch, `Dirs::adopt_old_layout` moves what older versions kept in the config folder to its new folder and deletes what nothing reads any more; something already in its new place wins. Patchwork also moves its database from the older `~/.config/dev.patchwork.ade` folder, and converts any older `settings.json` / `threads.json` into the database, keeping them as `*.json.bak`.
