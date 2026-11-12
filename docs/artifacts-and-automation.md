# Scheduled tasks, plugins, and computer use

Three features that sit around the agents. Each one is off or empty until it is used, and none of
them changes how a turn is run.

## Scheduled tasks

`crates/server/src/scheduled/` works out the next run from the shape of a schedule and the local
clock; there is no cron parser. The shapes are Codex's own: every few hours, every day, weekdays,
or named days.

- The row is in `scheduled_tasks` (`crates/server/src/persistence/scheduled.rs`), and saving a task
  reschedules it from now.
- `Server::run_due_scheduled_tasks` starts the tasks whose time has come. The desktop app calls it
  every 30 seconds from a thread of its own (`start_scheduled_tasks` in
  `crates/desktop/src/main.rs`).
- A task that starts makes an ordinary thread with the task's name, then sends the prompt as its
  first message, so everything after that is a normal turn: approvals, files, usage.
- A task that fails to start is logged and its next run recorded, so it does not retry every tick.
  A task whose project was deleted runs as a chat rather than not at all.

## Plugins

A bundle is a folder in the plugins folder (`<config>/plugins` unless Settings chooses another).
A plugin carries hooks and apps; skills belong in the skills folder and MCP servers in Settings →
MCP Servers. The app ships no plugins; Settings → Hooks shows an example bundle, one file:

```
plugins/guard/
└── hooks.json   { "hooks": { "preToolUse": [ … ], "stop": [ … ] } }
```

A plugin that runs hooks alone is that one file. A bundle that also carries apps has a
`plugin.json` beside it, with its hooks inline or in a `hooks.json` the manifest names.

`crates/server/src/plugins/` reads the manifest from `.patchwork-plugin/plugin.json`,
`.codex-plugin/plugin.json`, or a plain `plugin.json`, so a Codex bundle works as it is, and from
a `hooks.json` for a folder with none of those. A bundle's name is its folder's unless the file
gives `name`. `hooks` and `apps` are each either inline or a path, relative to the bundle
folder, to a file holding them, and may be nested under their own key as Codex writes them
(`{ "hooks": { … } }`, `{ "apps": { … } }`). Each part is read on its own: one this version cannot
read is left empty, and the rest of the bundle still loads.

Hook events match whatever their case: `userPromptSubmit`, `sessionStart`, `preToolUse`,
`permissionRequest`, `postToolUse`, `preCompact`, `postCompact`, `subagentStart`,
`subagentStop`, `stop`, `interrupt`, `sessionEnd`. Only `command` handlers run. A group's
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

Settings → Hooks lists each hook by when it runs and what it does, and names the agents whose turns
skip it, such as OpenCode for a tool hook.

## Computer use

`crates/server/src/computer/` gives Patchwork's own agent a screenshot and the pointer and keyboard
under it. It is off until Settings → Permissions turns it on; `set_enabled` mirrors the setting so
the tool list and the approval rules can ask without a database read.

- `crates/server/src/computer/` captures the primary monitor. On Windows and macOS that is `xcap`.
  On Linux it is the desktop portal under Wayland, cut to the primary monitor that XWayland
  reports, and the root window under X11. It uses `zbus` and `x11rb`, which the app already builds
  for notifications and the clipboard. `xcap` is not used on Linux, since it would also build
  PipeWire's bindings, which need newer headers than the Ubuntu CI builds on.
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
