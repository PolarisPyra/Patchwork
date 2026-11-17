# Chat app replies

How the agent of a chat started from a chat app (Settings → Gateway service) answers questions
about other chats, and how it is told to talk: the `read_chat` tool, the chat-app note, and the
personalities folder.

## Reading another chat's progress

`read_chat` is one of Patchwork's chat tools (`crates/core/src/threads/chat_tools.rs`), beside
`list_chats`, `start_chat`, `send_to_chat` and `wait_for_chat`. It lets the agent answer "what's
going on in X" or "progress on Y" with a short report on another chat.

`Runner::read_chat` in `crates/core/src/orchestration/tasks/chats.rs` finds the chat:

- With `chat_id`, it reads that chat.
- With `name`, it splits the words, drops filler words (`IGNORED`: "what's", "going", "progress",
  "task", …), and scores each chat by how many of the rest match a word of its title or project
  name. Words match when they are equal, or when one starts with the other and the shorter is at
  least 4 letters, so "margretes reverse engineering" finds "Reverse engineer" in the project
  `Margrete_v1.9.3`. The project name is optional.
- The chat with the best score is read. When several share it, it returns up to 8 of them
  (`CANDIDATES`), replying ones first and then the most recently active, and the agent picks one
  or asks. No match is an error that points the agent to `list_chats`.

A chat run from a chat app can read the chats of every project. Any other chat reads only its own
project's.

The report (`progress`) is JSON the agent rewrites as a standup update:

| Field | What it holds |
| --- | --- |
| `state` | `working`, `waiting`, `done` or `failed` |
| `task` | The chat's first message |
| `doing`, `replying_for_minutes` | The step it is on now, and how long the reply has run |
| `waiting_on` | The question or approval it is waiting on |
| `goal`, `plan` | Its goal, and its plan with the steps done out of the total |
| `recent_steps` | The last 12 tool calls of its latest reply, with their status (`STEPS`); a subagent's calls are left out |
| `background_tasks` | Tasks of its latest reply still running in the background |
| `files_changed` | The files changed by its latest reply that changed any |
| `messages` | Its last 6 messages, each cut to 1500 characters (`MESSAGES`, `MESSAGE_LIMIT`); a reply still streaming keeps its newest text |
| `queued_messages` | Messages waiting for the reply to finish |

## Listing what is running

`list_chats` answers "what's running" and "what are the tasks in X". Each chat comes with its
`state`, what it is `doing` while it replies, and `latest`, the end of its latest reply cut to 300
characters (`LATEST_LIMIT`), so one call is enough for a summary. Chats that are replying come
first. With `project`, only the chats of the project whose name fits best are listed, matched
the same way as a `read_chat` name.

When a `read_chat` name fits several chats equally, as a project's name does, the candidates
carry the same fields, so the agent can report on all of them without reading each.

`list_tasks` is the wider list: every task and session Patchwork knows of, grouped by
application (`capabilities/tasks.rs`). An application is a project, such as Mixer or Margrete,
or "Patchwork" for chats outside any project. A task is a chat, a sub agent of a chat, a
background task a chat started, or a scheduled task. With `outside`, the agent sessions on the
computer that Patchwork did not run are listed too, read from where each agent keeps them.

Each task has its application, name, kind, agent, model, state, start time and duration, last
activity, what it is doing, folder, the end of its output, tokens used, pending approvals, usage
limits, and the capabilities that act on it. The states are `running`, `waiting`, `failed`,
`terminated`, `idle`, `cancelled` and `completed`. A task is `running` only while Patchwork has
its process or turn; a chat whose reply was cut off by Patchwork closing is `terminated`, and a
session Patchwork did not run is never `running`.

## The chat-app note and the personality

`Core::chat_app_voice` in `crates/core/src/gateway/inbound.rs` adds two blocks to the end of the
message Patchwork sends the agent:

```
<chat-app>
You are talking with the person over a chat app… (CHAT_APP)
</chat-app>
<personality>
The chosen personality's text, or Patchwork's own (VOICE)
</personality>
```

`<chat-app>` never changes. It tells the agent to lead with the answer, report progress as a
standup update with real names and numbers, and keep formatting light. It also says that
"tasks" and "what is running" mean Patchwork's chats, to be answered from `list_chats` and
`read_chat` and never from shell commands, processes, git or logs. `<personality>` sets the
tone.

The blocks go in the user's message, not the system prompt. They carry the same weight as
anything the user types, so they cannot replace the agent's own instructions or Patchwork's
appended ones. A personality file that says "ignore your system prompt" is refused like the same
words typed in the chat would be. The blocks are added when the message is sent and are not
saved, so the chat in Patchwork shows only what the user wrote.

They are added when all of these hold:

- the chat is bound to a chat app conversation,
- the chat has no custom agent's role (an agent summoned with `&name` keeps its own voice),
- the message starts the agent's session, or the agent compacted its context on its own while
  replying to the message before.

The session check is in `crates/core/src/orchestration/turn/start/launch.rs`, which calls
`chat_app_voice` with whether the thread has a session yet. Patchwork's own compaction starts a
new session, so it is covered there. When the agent compacts by itself (Claude, Codex, OpenCode,
or Patchwork's own agent), the reply is saved with a `Part::Compaction`. The `compacted` helper
looks for one in the replies since the user's previous message, so the next message carries the
blocks again in case the summary dropped them. Antigravity does not report compaction.

## Personalities

A personality is a Markdown file in `<config>/personalities/` (see
[Config location](architecture.md#config-location)). The file's name without `.md` is the
personality's name, and the whole file is its text. There is no frontmatter.

```
personalities/
├── Butler.md
├── Hype.md
├── Pirate.md
└── Terse.md
```

`crates/core/src/gateway/personalities.rs` lists the folder and reads the chosen file. The first
time the folder is needed and does not exist, it is created with the four examples above
(`EXAMPLES`). It is never filled again, so a deleted example stays deleted.

Settings → Gateway service → Personality picks one. The choice is saved as
`gateway.personality`, the file's name, or none for Patchwork's own voice. The pane reads the
folder again every second while it is open, so a new file shows in the menu without reopening
Settings. The Personalities section at the bottom of the pane shows the folder's path and an
example file.

A change of personality reaches chats started after it, and chats already running at their next
compaction. A chosen file that is missing or empty falls back to Patchwork's own voice.

## The dispatcher and the capability registry

The chat a chat app talks to is a dispatcher. Its agent has no shell, files or web: it is started
with the dispatcher prompt (`chat_tools::DISPATCHER`), every built-in tool off, and Patchwork's own
tool server as the only one. The tools that server lists to a dispatcher are the capabilities in
`crates/core/src/orchestration/capabilities/`, and nothing else. Work that needs the computer goes
to a chat the dispatcher starts in a project with `start_chat`.

A capability is one row of the table (`table.rs` for the app, `chat_table.rs` for chats,
`control_table.rs` for tasks, the window, files and integrations). The row
holds its id, which is also its tool name, its area, a description for the model, its arguments,
what it returns, the kinds of thing it acts on, its permission, whether it can be taken back,
whether its work goes on after it returns, the events the person hears about later, and its
handler. `list_capabilities` returns the rows, all of them or one area's. Adding a capability is
adding a row and a handler; the tool list, the argument check, the asking and the log follow from
the row.

Every call goes through `registry::invoke`:

1. The id must be in the table. Anything else is answered "Patchwork has no capability …".
2. The arguments must be an object with only the row's arguments, each of its type, and every
   required one given.
3. The permission decides whether the person is asked first (below).
4. The handler runs, and what it returns or the error it failed with goes to the agent as it is.
5. A call that changes something runs once: while it runs, the same call with the same
   arguments is refused, and when it comes again straight after, it is answered with what the
   first one returned instead of running again (`Ran` in `registry.rs`). Reads are not held back.
6. The log gets `chat app ran <id>` or `chat app could not run <id>: <error>`, with the chat and
   the permission. Arguments are not logged, since they hold prompts and settings.

### Who is asked, by access mode

The access mode is the dispatcher chat's own, the one in Settings → Gateway service (`gateway.access`).
Changing it there, or with `set_setting`, reaches the conversations that already have a dispatcher
with their next message.

| Permission | What it covers | Supervised | Auto-accept edits, Auto, Full access |
|---|---|---|---|
| `read` | Reads only | Runs | Runs |
| `act` | What the person does from the chat list: start, message, stop or answer a chat. The chats ask for themselves, by their own access | Runs | Runs |
| `change` | Changes a setting, a color scheme, a project's name, a chat's options; sends a project file; brings in chats; turns an MCP server or hook bundle on or off | Asks | Runs |
| `confirm` | Cannot be taken back: deleting a chat, a color scheme or a scheduled task, removing a project or an MCP server, unpairing a person, installing an update or an agent, turning a chat app on or off, making a pairing code | Asks | Asks |

A `change` that widens what Patchwork may do also asks outside Supervised (`Ctx::escalate`): the
settings `gateway.access`, `computerUse`, `androidUse`, `systemPrompts`, `unlimitedSteps`,
`turnTokenLimit` and `integrations`, raising a chat's access with `set_chat_options`, turning an
MCP server or a hook bundle on, adding a folder as a project (its files can then be listed, sent
and worked on), and sending a file whose path looks like it holds secrets (`SECRETS` in
`capabilities/navigate.rs`). A sent file's name keeps only letters, digits, `.`, `-`, `_` and
spaces, because it goes into the chat app's upload request.

The request names the capability and its arguments ("Patchwork: set_setting", "path: textSize,
value: small") and is answered with the platform's Allow and Deny buttons, or in words where it
has none. It is the same request the desktop shows on the dispatcher's chat, so either side can
answer it. A denied call fails with "The user declined this call", and nothing is changed.

### The capabilities

| Area | Capabilities |
|---|---|
| `tasks` | `list_tasks` |
| `chats` | `list_chats`, `read_chat`, `start_chat`, `send_to_chat`, `wait_for_chat`, `stop_chat`, `answer_chat`, `notify_when_done`, `open_chat`, `rename_chat`, `archive_chat`, `set_chat_options`, `delete_chat` |
| `projects` | `list_projects`, `list_project_files`, `add_project`, `rename_project`, `remove_project` |
| `settings` | `get_settings`, `set_setting` |
| `navigation` | `open_project`, `open_file`, `open_settings` |
| `files` | `send_project_file`, `send_chat_changes` |
| `import-export` | `export_chat`, `scan_imports`, `import_chats` |
| `themes` | `list_themes`, `read_theme`, `preview_theme`, `save_theme`, `apply_theme`, `delete_theme` |
| `usage` | `usage_limits`, `resume_chat_at_reset` |
| `notifications` | `set_chat_notices`, `unmute_all_chats` |
| `chat-apps` | `gateway_status`, `restart_gateway`, `set_chat_app_enabled`, `make_pairing_code`, `unpair_user` |
| `providers` | `list_providers`, `install_agent` |
| `integrations` | `list_mcp_servers`, `restart_mcp_server`, `set_mcp_server_enabled`, `remove_mcp_server`, `list_agents`, `list_hooks`, `set_hook_enabled` |
| `extensions` | `list_models`, `list_skills`, `set_skill_enabled` |
| `scheduled` | `list_scheduled_tasks`, `run_scheduled_task`, `set_scheduled_task_enabled`, `delete_scheduled_task` |
| `patchwork` | `list_capabilities`, `diagnostics`, `check_for_updates`, `install_update` |

Mixer, Margrete and any other application the person works on are projects. `list_tasks` with
`application` lists one's tasks; neither application has a task interface of its own for
Patchwork to read, so what Patchwork knows of them is what runs in their projects.

### Settings

`get_settings` reads one setting by its path, such as `textSize` or `notifications.sound`, or all
of them. `set_setting` changes one: the new value is put into a copy of the settings, the copy
must still read as valid settings (a wrong value fails with what the setting accepts), and then it
is saved and the window is told to redraw (`AgentEvent::SettingsChanged`). It returns what the
setting was and what it is now.

Two settings are never shown or changed, because they hold secrets: `terminal.env` and
`gateway.platforms`. Bot tokens and provider keys are encrypted in Patchwork's vault, which no
capability reads.
`LOCKED` in `capabilities/settings.rs` lists the settings a chat app cannot change, each with what
to do instead; projects, for one, go through the project capabilities.

### Color schemes

The dispatcher's own model writes a color scheme from what the person describes, in the format
`read_theme` returns. `save_theme` checks it the way Settings → Appearance does and refuses one
whose text would be hard to read, saying which contrast fell short. `apply_theme` draws Patchwork
in a saved scheme or in `light`, `dark`, `oled` or `system`, and returns what it was drawn in
before, so "switch back" has an id to use.

`preview_theme` sends a picture of the window drawn in a scheme, saved or not, to the
conversation as `preview.png` (`themes/preview.rs`): the chat list, a chat with a message, a reply
and code, the composer, and the scheme's colors. Text is drawn as bars in the colors text has,
since the picture is drawn without fonts. It returns the text that would be hard to read. On a
chat app that takes no files, it fails and says so.

### Usage limits

`usage_limits` returns what each provider last reported, with the reset time the provider gave
and the time now. Where a provider gave no reset time there is none in the answer, and the
dispatcher is told never to work one out. `resume_chat_at_reset` has a chat sent on when its
limit resets.

### The window

`open_chat`, `open_project`, `open_file` and `open_settings` move Patchwork's window on the
computer and raise it (`AgentEvent::OpenChat`, `AgentEvent::Show`). They are done by the desktop
when the event reaches it; the capability returns once it has asked. `open_file` takes a path in
the project or a name: one fitting file is opened, and several are listed for the dispatcher to
ask which.

### Files, exports and imports

`send_project_file` sends a text file of a project, up to 2 MB, to the conversation. A name that
looks like it holds secrets (`.env`, a key, `credentials`, …) asks the person in every access
mode. `send_chat_changes` sends a chat's changes as a diff file, and `export_chat` sends a chat as
Markdown. `scan_imports` counts the chats other agent apps left on the computer and `import_chats`
brings them in.

### Chat apps, agents and integrations

`set_chat_app_enabled` turns a chat app's connection on or off. It can be turned on only when the
tokens it needs are saved in Settings on the computer; no capability reads or takes a token.
`make_pairing_code` makes a pairing code. Both ask the person every time.

`list_providers` lists the agents with whether each is installed and turned on, and
`install_agent` runs an agent's installer after asking. Signing in and API keys stay on the
computer. `list_mcp_servers`, `list_agents` and `list_hooks` list what is set up, without a
server's command arguments, environment or headers, which can hold secrets.

### Updates

`install_update` notes the conversation that asked, returns, and has Patchwork restart once the
dispatcher's reply has been sent (`AgentEvent::RestartToUpdate`). When the chat apps are
connected after the restart, the conversation is told Patchwork is back and on which version
(`tell_back_from_update` in `gateway/control.rs`).

### What a dispatcher cannot do

- Run a command, read a file's contents itself or use the web. A chat it starts does that.
- Allow a command, a file change or a plan for another chat. It passes the person's answer on
  with `answer_chat`.
- Read or set a token, a key or the environment of the terminal.
- Set a chat app up: its tokens are entered in Settings on the computer.
- Sign an agent in or enter an API key.
- Ask for a secret: `request_secret` is not listed to it.

### Slash commands

Three are left, for what acts on the conversation itself and needs no model: `/help`, `/new`
(start the conversation over, or `/new <project>` to talk straight to a new chat in a project) and
`/stop`. The buttons under notices send their own (`/open`, `/answer`, `/mute`, `/resume`). The
others (`/status`, `/running`, `/threads`, `/use`, `/projects`, `/diff`, `/model`, `/agent`,
`/plan`, `/access`, `/unmute`) are gone: sent now, they reach the dispatcher as words, which does
the same through the capabilities. `pass_on` takes the leading slash off a dispatcher's message,
because its agent would otherwise answer `/status` itself, as a command it does not have. A chat
started with `/new <project>` keeps the slash, so its agent's own commands still work.

### Messages that arrive twice

A chat app sends a message again when its connection drops before Patchwork's answer reaches it.
The gateway drops a message whose platform id it has already taken, and, on platforms that give
no id, the same words from the same person within 3 seconds (`repeated` in `gateway/inbound.rs`).
