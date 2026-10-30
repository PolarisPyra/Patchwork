# Artifacts, scheduled tasks, plugins, and computer use

Four features that sit around the agents. Each one is off or empty until it is used, and none of
them changes how a turn is run.

## Artifacts

`apps/web/src/features/chat/components/Artifact.tsx` draws two kinds of fence in a reply:

| Language | What happens |
| --- | --- |
| `mermaid` | Drawn with Mermaid, loaded the first time a reply has a diagram. A diagram that fails says so in one line instead of showing a trace. |
| `html`, `htm`, `svg`, `xhtml` | Run in an `iframe` with `sandbox="allow-scripts"`, so it cannot read the app, its storage, or the cookies of anything else. |

Every provider's replies go through `Markdown.tsx`, so the same two fences are drawn whatever the
agent wrote. A block has a toggle back to its source, and the frame reports its own height so a
short artifact does not leave an empty box. A frame cannot be measured from the app's side: the
height arrives as a message from the artifact.

## Scheduled tasks

`apps/server/src/scheduled/mod.rs` works out the next run from the shape of a schedule and the local
clock; there is no cron parser. The shapes are Codex's own: every few hours, every day, weekdays,
or named days.

- The row is in `scheduled_tasks` (`apps/server/src/persistence/scheduled.rs`), and saving a task
  reschedules it from now.
- `Server::run_due_scheduled_tasks` starts the tasks whose time has come. The desktop shell calls it
  every 30 seconds from a thread of its own (`start_scheduled_tasks` in
  `apps/desktop/src-tauri/src/lib.rs`).
- A task that starts makes an ordinary thread with the task's name, then sends the prompt as its
  first message, so everything after that is a normal turn: approvals, files, usage.
- A task that fails to start is logged and its next run recorded, so it does not retry every tick.
  A task whose project was deleted runs as a chat rather than not at all.

## Plugins

A bundle is a folder in the plugins folder (`<config>/plugins` unless Settings chooses another)
with a `plugin.json`. A plugin carries hooks and apps; skills belong in the skills folder and MCP
servers in Settings → MCP Servers. The app ships no plugins. Patchwork's own hooks live in a
`patchwork` bundle there, switched on and off like any other:

```
plugins/patchwork/
└── hooks.json   { "hooks": { "stop": [ { "hooks": [ { "type": "command", "command": "…" } ] } ] } }
```

`apps/server/src/plugins/` reads the manifest from `.patchwork-plugin/plugin.json`,
`.codex-plugin/plugin.json`, or a plain `plugin.json`, so a Codex bundle works as it is. A folder
with none of those and a `hooks.json` is a plugin of hooks alone, named for its folder unless the
file gives `name`. `hooks` and `apps` are each either inline or a path, relative to the bundle
folder, to a file holding them, and may be nested under their own key as Codex writes them
(`{ "hooks": { … } }`, `{ "apps": { … } }`). Each part is read on its own: one this version cannot
read is left empty, and the rest of the bundle still loads.

Hook events match whatever their case: `userPromptSubmit`, `sessionStart`, `preToolUse`,
`permissionRequest`, `postToolUse`, `preCompact`, `postCompact`, `subagentStart`,
`subagentStop`, `stop`, `interrupt`, `sessionEnd`. `command` and `builtin` handlers run. A group's
`matcher` limits a tool event to the tools whose whole name it matches as a regular expression
(`Bash`, `Edit|Write`); none, or `*`, is every tool.

Patchwork runs the turn events itself, for every agent: `userPromptSubmit` before a turn starts,
and `stop` or `interrupt` then `sessionEnd` after it ends. Such a command runs in the user's shell,
or bash when that shell does not read POSIX commands (fish, Nushell), in the turn's project folder, with `PATCHWORK_EVENT`, `PATCHWORK_THREAD`, `PATCHWORK_CWD`,
`PATCHWORK_AGENT` and `PATCHWORK_STATUS` set, and is stopped at its `timeoutSec` (default 60, 1 to
600). A failed hook is logged and never stops the turn.

The other events happen inside the agent, so the agent runs them on its own hooks, with the event
as JSON on stdin; a `preToolUse` hook refuses a call with exit code 2 and its reason on stderr, or
with `{"hookSpecificOutput": {"permissionDecision": "deny", "permissionDecisionReason": "…"}}`.
Each agent gets the payload and tool names it uses itself:

| Agent | Events | How |
| --- | --- | --- |
| Claude Code | every one | `hooks/hooks.json` in the plugin folder Patchwork passes with `--plugin-dir` |
| Codex | every one | `-c hooks.<Event>` overrides, trusted by hash, beside Patchwork's permission hook |
| Antigravity | `sessionStart`, `preToolUse`, `postToolUse` | the `.agents/hooks.json` it loads from a folder added with `--add-dir` |
| API providers | `preToolUse`, `postToolUse` | Patchwork's own agent runs them around each call, with Claude Code's payload |
| OpenCode | none | it has no hooks of this kind |

A `builtin` handler, `{ "type": "builtin", "name": "boundedWaits" }`, is a check Patchwork carries
itself (`apps/server/src/plugins/builtin/`). An agent CLI runs it as `patchwork --hook <name>
<agent>`, which reads the call in that agent's format and answers in it; Patchwork's own agent runs it
in-process. A name this version does not carry is logged and skipped. A built-in in a group with no
`matcher` gets the tools it is for, so `boundedWaits` needs none: it covers each agent's shell tool.

Settings → Hooks lists each hook by when it runs and what it does, and names the agents whose turns
skip it, such as OpenCode for a tool hook.

- `boundedWaits` refuses a shell command that polls in a loop (`until` or `while` with a `sleep` in
  it) with no `timeout`, and one that polls for a word cargo prints in colour (`Running`,
  `Compiling`, `Finished`, `Building`). It reads the command as the shell does, so only what runs
  counts: a loop quoted as data, in `printf`, `echo`, `jq`, `grep`, a commit message, or a heredoc
  written to a file, is not a loop. The scripts `sh -c` and its kin, `eval`, `$(…)`, and a shell's
  heredoc run are read too, and one that `timeout` runs has its limit.

## Computer use

`apps/server/src/computer/mod.rs` gives Patchwork's own agent a screenshot and the pointer and keyboard
under it. It is off until Settings → Permissions turns it on; `set_enabled` mirrors the setting so
the tool list and the approval rules can ask without a database read.

- `apps/server/src/computer/screen.rs` captures the primary monitor. On Windows and macOS that is `xcap`.
  On Linux it is the desktop portal under Wayland, cut to the primary monitor that XWayland
  reports, and the root window under X11. It uses `zbus` and `x11rb`, which the app already builds
  for notifications and the clipboard. `xcap` is not used on Linux, since it would also build
  PipeWire's bindings, which need newer headers than the Ubuntu the AppImage is built on.
- The capture is never scaled: it is written at the screen's own size, either as a `data:` URL
  (for the pane) or to a PNG (for the agent, which reads it with `read_file`).
- A capture takes a moment on Wayland: the portal round-trip is about two and a half seconds on a 4K
  screen. That is the cost of the screenshot, not of the scaling.
- Input uses `enigo`: move, click, type, press a key with modifiers, and scroll.
- The tools are `computer_screenshot`, `computer_move`, `computer_click`, `computer_type`,
  `computer_key`, and `computer_scroll`. They are offered only when the setting is on, never in plan
  mode, and they ask under the `screen` permission kind unless a rule allows the screen.
- `cargo test -p patchwork-server -- --ignored screenshot_captures_the_screen` takes a real
  screenshot and checks the PNG header. It needs a display, so it is ignored by default.
