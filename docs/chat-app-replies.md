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
standup update with real names and numbers, and keep formatting light. `<personality>` sets the
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
