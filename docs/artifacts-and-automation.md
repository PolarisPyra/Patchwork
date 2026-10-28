# Artifacts, scheduled tasks, plugins, and computer use

Four features that sit around the agents. Each one is off or empty until it is used, and none of
them changes how a turn is run.

## Artifacts

`apps/web/src/components/chat/Artifact.tsx` draws two kinds of fence in a reply:

| Language | What happens |
| --- | --- |
| `mermaid` | Drawn with Mermaid, loaded the first time a reply has a diagram. A diagram that fails says so in one line instead of showing a trace. |
| `html`, `htm`, `svg`, `xhtml` | Run in an `iframe` with `sandbox="allow-scripts"`, so it cannot read the app, its storage, or the cookies of anything else. |

Every provider's replies go through `Markdown.tsx`, so the same two fences are drawn whatever the
agent wrote. A block has a toggle back to its source, and the frame reports its own height so a
short artifact does not leave an empty box. A frame cannot be measured from the app's side: the
height arrives as a message from the artifact.

## Scheduled tasks

`apps/server/src/scheduled.rs` works out the next run from the shape of a schedule and the local
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
├── plugin.json   { "name": "Patchwork", "hooks": "./hooks.json" }
└── hooks.json    { "hooks": { "stop": [ { "hooks": [ { "type": "command", "command": "…" } ] } ] } }
```

`apps/server/src/plugins.rs` reads the manifest from `.patchwork-plugin/plugin.json`,
`.codex-plugin/plugin.json`, or a plain `plugin.json`, so a Codex bundle works as it is. `hooks`
and `apps` are each either inline (`{ "hooks": { … } }`, `{ "apps": { … } }`) or a path, relative to
the bundle folder, to a file holding that. Each part is read on its own: one this version cannot
read is left empty, and the rest of the bundle still loads.

Hook events match whatever their case: `userPromptSubmit`, `sessionStart`, `preToolUse`,
`permissionRequest`, `postToolUse`, `preCompact`, `postCompact`, `subagentStart`,
`subagentStop`, `stop`, `interrupt`, `sessionEnd`. Only `command` handlers run, and so far only on
`userPromptSubmit` (before a turn starts) and `stop` or `interrupt` then `sessionEnd` (after it
ends). A command runs in the user's shell in the turn's project folder, with `PATCHWORK_EVENT`,
`PATCHWORK_THREAD`, `PATCHWORK_CWD`, `PATCHWORK_AGENT` and `PATCHWORK_STATUS` set, and is stopped
at its `timeoutSec` (default 60, 1 to 600). A failed hook is logged and never stops the turn.

## Computer use

`apps/server/src/computer.rs` gives Patchwork's own agent a screenshot and the pointer and keyboard
under it. It is off until Settings → Permissions turns it on; `set_enabled` mirrors the setting so
the tool list and the approval rules can ask without a database read.

- A screenshot is captured with `xcap`, scaled to 1440 pixels wide with a triangle filter, and
  written either as a `data:` URL (for the pane) or to a PNG (for the agent, which reads it with
  `read_file`).
- A capture takes a moment on Wayland: the portal round-trip is about two and a half seconds on a 4K
  screen. That is the cost of the screenshot, not of the scaling.
- Input uses `enigo`: move, click, type, press a key with modifiers, and scroll.
- The tools are `computer_screenshot`, `computer_move`, `computer_click`, `computer_type`,
  `computer_key`, and `computer_scroll`. They are offered only when the setting is on, never in plan
  mode, and they ask under the `screen` permission kind unless a rule allows the screen.
- `cargo test -p patchwork-server -- --ignored screenshot_captures_the_screen` takes a real
  screenshot and checks the PNG header. It needs a display, so it is ignored by default.
