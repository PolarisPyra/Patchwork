# Architecture

## Repo layout

The repo is a Cargo workspace split into applications under `apps/`, backend and platform libraries
under `crates/`, and a vendored GPUI stack under `vendor/` (excluded from workspace members so
workspace lints target Patchwork's code; see [vendor/README.md](../vendor/README.md)).

### Applications (`apps/`)

- `apps/desktop` (`patchwork`, default workspace member) — the native window, drawn with GPUI. It
  calls `Core` directly and applies its agent events on the UI thread.
  - `src/main.rs` — opens the core, background tasks, and the window
  - `src/workspace/` — the window: its state, layout, bars, update card, actions, `hosts/` (glue for pages and dialogs from desktop
    crates), and `dock/` (the tabbed area under the chat and its place in the window)
  - `src/{chat,sidebar,canvas,agents,orchestrator,terminal}/` — UI, one folder per feature; the
    dock's other tabs are `src/logs.rs` (Output) and `src/screen.rs`
- `apps/desktop/crates/` — UI foundations and desktop feature crates, split to keep incremental
  rebuilds small:
  - `tokens` (`patchwork-ui-tokens`) — build-time packed, compressed SVG icon pool (`icons.pack`)
    and compile-time typed `IconName` registry (with debug-mode disk reload)
  - `theme` (`patchwork-ui-theme`) — design tokens, palette, sizing, embedded assets, icons, brand
    marks, and view task helpers
  - `components` (`patchwork-ui-components`) — buttons, inputs, menus, dialogs, popovers, badges,
    avatars, scrollbars, and actions
  - `markdown` (`patchwork-ui-markdown`) — Markdown layout, syntax highlighting, sigils, formatting,
    and selectable text
  - `editor` (`patchwork-ui-editor`) — multi-line text area and composer editor
  - `backdrop` (`patchwork-ui-backdrop`) — animated backdrops, ambient scenes, and background presets
  - `shared` (`patchwork-desktop-shared`) — helpers binding core domain types to theme marks and
    labels
  - `browser` (`patchwork-desktop-browser`) — browser sidecar view drawing web pages and quilts
  - `android` (`patchwork-desktop-android`) — Android sidecar view (emulators, devices, screen
    mirroring, and logcat)
  - `settings` (`patchwork-desktop-settings`) — Settings dialog, its panes, and the background picker
  - `search` (`patchwork-desktop-search`) — command palette and search modal
  - `onboarding` (`patchwork-desktop-onboarding`) — first-run setup flow
  - `quilts` (`patchwork-desktop-quilts`) — Quilts gallery and viewer page
  - `dialogs` (`patchwork-desktop-dialogs`) — workspace dialogs (`add_project`, `credential`,
    `granular`, and `interrupted`)
  - `dylib` (`patchwork-dylib`) — bundles desktop dependencies into one shared library so `dynamic`
    development builds relink only Patchwork
- `apps/remote` (`patchwork-remote`) — helper binary copied to SSH hosts to run agents, their hooks,
  and their relays remotely.
- `apps/android` (`patchwork-android`) — the Android client, a GPUI app that connects to a running
  Patchwork. It runs on `vendor/gpui_android`, GPUI's Android backend, and is built into an APK by
  `cargo run android` (`scripts/cargo-android`).

### Backend and platform crates (`crates/`)

- `crates/patchwork_platform` (`patchwork-platform`) — the user's machine, with no knowledge of
  chats or providers: processes and their environment, login shells, paths (`files/dirs.rs`),
  opening files on the desktop, the system credential store (`keychain`: Secret Service through
  `oo7` on Linux, `keyring` elsewhere), machines reached over SSH (`ssh/`), and the shared `Error`.
- `crates/patchwork_daemon` (`patchwork-daemon`) — app lifecycle: signed updates
  (`updates/release.rs`), restarts, crash containment, and logging.
- `crates/patchwork_themes` (`patchwork-themes`) — theme, scene, and code theme files as data.
- `crates/patchwork_vault` (`patchwork-vault`) — encrypted secret vault (XChaCha20-Poly1305,
  Argon2id passwords, and recovery keys).
- `crates/patchwork_git` (`patchwork-git`) — git repository operations, hunk staging, conflict
  resolution, and askpass helpers.
- `crates/provider_core` (`patchwork-provider-core`) — shared agent provider schema, capability
  flags, CLI discovery, probing, and diagnostics.
- `crates/provider_acp`, `crates/provider_api`, `crates/provider_claude`, `crates/provider_codex`,
  `crates/provider_antigravity`, `crates/provider_opencode` (`patchwork-provider-*`) — per-provider
  CLI/API discovery, configuration, model catalogs, probing, and bridges.
- `crates/patchwork_backend` (`patchwork-backend`) — the core backend crate with no UI dependency:
  - `src/lib.rs` — the module list, `Core`, and how it opens. Every other module is one domain,
    holding its wire types, its `impl Core` operations, and its logic:
  - `src/chats/` — chats, messages, and chat tools (`tools.rs`); `src/projects/` — projects, their
    folders, and file watching (`watch.rs`); `src/settings/` — app settings; `src/agents/` — custom
    agents
  - `src/provider/` — provider turn wiring, hooks, and parsing built on top of `crates/provider_*`
  - `src/orchestration/` — runs turns, grouped by feature: `turn/` (the `Runner`, starting and
    ending a turn, its process, turns cut off by a restart), `messages/` (sending, steering, and
    queueing), `output/` (reading agent output into agent events and streaming them), `approvals/`,
    `changes/` (which files each reply changed, and per-turn checkpoints), `tasks/` (background
    tasks and subagents), `capabilities/` (chat-app dispatcher capabilities), and `goal/`
  - `src/harness/` — Patchwork's own API-provider agent loop and built-in tools
  - `src/permissions/` — approval rules and the permission hook Codex and Antigravity run
  - `src/extensions/` — user extensions: `agents.rs`, `skills.rs`, and hook bundles (`hooks/`)
  - `src/persistence/` — SQLite store (`store.rs`) and schema migrations (`migrations/mod.rs`)
  - `src/gateway/` — chat app connections (Telegram, Discord, etc.), inbound routing, personalities,
    and pairing
  - `src/checks/` — post-turn project check detection, execution, and failure parsing
  - `src/import/`, `src/usage/`, `src/costs/`, `src/scheduled/`, `src/computer/`, `src/quilts/`,
    `src/themes/`, `src/source_control/`, `src/worktree/`, `src/terminal/`, `src/browser/`,
    `src/android/`, `src/vault/`, `src/updates/`, `src/logging/`, `src/events.rs`,
    `src/backgrounds.rs`, `src/notifications.rs`, `src/issues.rs`, `src/service.rs`

## Data model

Everything the sidebar shows comes from one SQLite database, `patchwork.db`, in the data folder. The store loads it into memory at startup and writes each change through before it is visible (`crates/patchwork_backend/src/persistence/store.rs`).

| Table | What it holds |
| --- | --- |
| `settings` | App settings as JSON: theme, connected agents, integrations. |
| `projects` | One row per project folder: `name`, `path` (unique), and `position` for sidebar order. |
| `folders` | Sub-folders inside a project: `project_id`, `name`, `position`. Organisation only; no directory is created on disk. |
| `chats` | Tasks and chats. `project_id` NULL makes it a chat. `folder_id` NULL puts it at the top level of its project. `session_id` lets the agent resume. |
| `messages` | The messages of each chat, with text, tool calls, plan, file changes, approvals, attachments, and what the project's checks made of the turn. |
| `scheduled_tasks` | Prompts that start a task on a schedule: its shape as JSON, the project, agent and access mode to run with, and when it last and next runs. |
| `app_state` | Small key/value state that is not settings, such as the chat that was open last. |

Where a chat runs:

- **Task:** in its own git worktree, which its first turn makes with `git worktree add -b patchwork/<id>` into `.patchwork/worktrees/<repo>-<id>` in the project's repository (kept out of `git status` by the repository's `info/exclude`; a project on an SSH host keeps them in the host's Patchwork folder), unless the project edits in place, isn't a git repository on this computer, or the turn only plans. The task's `app_state` key `worktree:<id>` records it; a fork and the subagents work in their task's worktree unless the settings give a fork its own. If making it fails, that turn runs in the project's `path` and the task shows why. Once a task has a worktree, it never works in the project's `path` instead: a turn whose worktree can't be used or made again doesn't start, and checkpoints and Source Control refuse while it is gone.
  - **Moved:** worktrees an earlier version made under the app's local data directory are moved into their repository's `.patchwork/worktrees` by the hourly cleanup, with `git worktree move`, once their task is idle. A moved task's next message starts a new agent session, sent the transcript.
  - **Parked:** cleanup removed the folder and kept the branch, by the settings' rule or because it found the folder deleted or git's record of it gone. The next turn makes it again from its branch at the same path.
  - **Broken:** the folder is there but isn't linked to its repository, and `git worktree repair` can't link it. Nothing commits in it, removes it, or makes it again until the user moves the folder away.
  - **Deleted task:** what it hadn't committed is committed to its branch, then the folder goes while no other task shares it, if the settings remove worktrees with deleted tasks. The branch goes only once the project, or the remote's default branch, has all of it.
  - **Settled:** an idle, clean worktree whose branch landed on the default branch, by its changes or a merged pull request into it, has its branch moved there.
- **Chat:** in `chats/<chat id>` under the app's local data directory (`app_local_data_dir`). The folder is deleted with the chat.

Sidebar order is the `position` column. A new chat gets `-created_at`, so the newest is first until it is dragged. Dropping rows calls `reorder_chats`, which numbers that list `0..n`.

Schema changes go in as a new entry appended to `MIGRATIONS` in `crates/patchwork_backend/src/persistence/migrations/mod.rs`. Never edit one that has shipped.

## File changes

The open project is watched for changes, and the UI never polls git.

- **Watching** (`crates/patchwork_backend/src/projects/watch.rs`). On Linux, inotify watches one folder at a time, so its `linux` module adds a watch for each folder git keeps. On macOS and Windows, FSEvents and ReadDirectoryChangesW cover a whole tree with one watch, so its `tree` module watches the project root. On every platform, its `Filter` drops paths git ignores, `node_modules`, `target`, and everything in `.git` below its top level. The top level is kept because commits, staging, and checkouts change it.
- **Refreshing.** Events are merged until 250 ms pass without one, or for at most 1 s. Then `Core::watch_project` runs one `git status`. It runs one `git diff HEAD --numstat` for line counts only when a tracked file changed. The result goes to the UI as a `project-changed` event. Git runs with `GIT_OPTIONAL_LOCKS=0`, so its own reads do not rewrite `.git/index` and trigger another refresh.
- **Which files a reply changed** (`crates/patchwork_backend/src/orchestration/changes/attribution.rs`).
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

- **What runs** (`crates/patchwork_backend/src/checks/detect.rs`). The first of: a `check` script in `package.json`, else its `test` script, run with the package manager the lockfile names when that tool is installed; `cargo test`; `python -m pytest`; `go test ./...`; a Makefile's `check` or `test` target; `mix test`. A project with none of them has nothing to check, and its replies show no card at all.
- **When** (`crates/patchwork_backend/src/orchestration/turn/end.rs`). When a turn finishes with `Complete` and its reply changed at least one file. A stopped or failed turn changed nothing to judge.
- **How** (`crates/patchwork_backend/src/checks/run.rs`). Through the same shell an agent's commands run in, in the project folder, with `CI=1` and colour off. Output keeps its last 16 KB; the process group is killed at 5 minutes, and that is never a pass.
- **What it means** (`TurnChecks` in `crates/patchwork_backend/src/chats/messages.rs`). `passed`, `failed`, `timedOut`, or `notRun`, with the command, how long it took, one `CheckFailure` per thing that failed (`crates/patchwork_backend/src/checks/parse.rs` reads cargo's, Node's, Jest's, Vitest's, pytest's and Go's shapes), and the tail of the output. A check that did not finish is `timedOut` or `notRun`, never `passed`. A check left running when the app closed is `notRun` at the next start (`turn/recovery.rs`).
- **What a reply shows** (`apps/desktop/src/chat/transcript/reply/checks.rs`). Under the files a reply changed: the outcome, each failure by name, and the output behind a disclosure. Its button runs the command again; its footer turns the project's checks on or off without leaving the chat. A reply shows a card only when checks ran or are running for it, so with them off there is no card at all: the switch is in Settings → Checks, which is one place to look rather than a card in every reply saying it did not check. The result arrives as a `checksRecorded` event, saved on the message like an approval.
- **Asking another way**. The command palette's "Run checks for the open task" runs them for the reply on screen, whether or not the project has them on, and the card it produces is where they can be turned on.

## Approvals and questions

An agent that wants permission, or an answer, sends a request the turn then waits on
(`crates/patchwork_backend/src/orchestration/approvals/`). The user's answer normally goes back down the
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

`crates/patchwork_platform/src/files/dirs.rs` splits Patchwork's files by who owns them, each in a `patchwork` folder inside the platform's own:

| Holds | Linux | macOS | Windows |
|---|---|---|---|
| Config: what the user edits, `skills/`, `agents/`, `hooks/`, `personalities/`, `mcp/` (one `<name>.json` per MCP server), `providers.json` | `~/.config/patchwork` | `~/Library/Application Support/patchwork` | `%APPDATA%\patchwork` |
| Data: what Patchwork keeps, `patchwork.db` and `api-sessions/` | `~/.local/share/patchwork` | `~/Library/Application Support/patchwork/data` | `%LOCALAPPDATA%\patchwork` |
| Cache: what Patchwork rebuilds, `launch/<cli>/` (the skills, agents and hooks each CLI is launched with), `mcp-cli-tools.json`, `screenshots/` | `~/.cache/patchwork` | `~/Library/Caches/patchwork` | `%LOCALAPPDATA%\patchwork\cache` |
| State: `mcp-relay.json` | `~/.local/state/patchwork` | the data folder | the data folder |
| Logs, one folder per launch | `~/.local/state/patchwork/logs` | `~/Library/Logs/patchwork` | `%LOCALAPPDATA%\patchwork\logs` |

macOS has one folder for config and data, and Windows one for data and cache, so the second is a subfolder of the first. Chats' scratch folders and pasted images stay in the app-identifier data folder.

On launch, `Dirs::adopt_old_layout` moves what older versions kept in the config folder to its new folder and deletes what nothing reads any more; something already in its new place wins. Patchwork also moves its database from the older `~/.config/dev.patchwork.ade` folder, and converts any older `settings.json` / `threads.json` into the database, keeping them as `*.json.bak`.
