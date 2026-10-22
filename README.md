# Patchwork

**A desktop workspace for coding agents.** Patchwork runs Claude Code, Codex, and Antigravity
on the subscriptions you already have, and keeps every task, project, and chat in one place.
Each edit lands in your project, and you review it before you keep it.

<!-- TODO: add a screenshot or GIF of a task in the diff view, e.g. assets/screenshot.png -->

## Why Patchwork

- **No new accounts or API keys.** Patchwork drives the agent CLIs already installed and signed
  in on your machine.
- **Review before you keep it.** The canvas shows each task's Plan, Code, and Diff views. Stage a
  file to `git add` it, or reject it to restore it.
- **One sidebar for every agent.** Projects hold their tasks, and standalone chats live at the top
  level. Each chat runs in a scratch folder that is deleted with it.

## Features

- **Three agents, one interface:** Claude Code, Codex, and Antigravity, with the same chat,
  plan, and diff views for each.
- **Access modes per task:** supervised, auto-accept edits, auto, or full access. Approval
  requests show up in the chat.
- **Goals and steering:** set a `/goal` and let the agent work toward it, or send a message
  while a turn is running to steer it (Claude Code and Codex).
- **Subscription usage:** the **Usage** button at the bottom of the sidebar shows each
  provider's limits.
- **Import what you already have:** bring in conversations from Claude Code, Codex, the
  Gemini CLI (Antigravity chats), and [T3 Code](https://github.com/pingdotgg/t3code). Each one
  becomes a task in its project, and follow-ups resume the original session.

## Install

Download `Patchwork_amd64.AppImage` from the
[latest release](https://github.com/PolarisPyra/patchwork/releases/latest), make it
executable, and run it:

```sh
chmod +x Patchwork_amd64.AppImage
./Patchwork_amd64.AppImage
```

Release builds are currently Linux only. To build from source, see
[Development](docs/development.md).

## Requirements

Install and sign in to at least one agent CLI:

- [Claude Code](https://docs.claude.com/en/docs/claude-code) (`claude`)
- [Codex](https://github.com/openai/codex) (`codex`)
- Antigravity (`agy`). **Settings** can install it for you.

Patchwork detects which of these CLIs are installed. Claude Code and Codex also report whether
you are signed in.

## Quick start

1. Open Patchwork and add a project folder.
2. Start a task in the project with its **+** button, and pick an agent and an access mode.
3. When the agent finishes, review the changes in the canvas. Stage the files you want, and
   reject the rest.

To try an agent without touching a project, start a chat from the top of the sidebar.

## Your data

- Patchwork stores its state in a SQLite database at `~/.config/patchwork/patchwork.db`.
- Tasks run directly in their project folder. Chats run in a scratch folder under the app's data
  directory, which is deleted with the chat.
- The agent CLIs do the work and talk to their own services. Patchwork reads the files they save,
  such as session transcripts, to show history and usage. See
  [Agent CLIs](docs/agent-clis.md) for every file and command it touches.

## Documentation

- [Development](docs/development.md): build, run, and test Patchwork
- [Architecture](docs/architecture.md): repo layout and the data model
- [Agent CLIs](docs/agent-clis.md): how each agent is detected, run, and imported
- [Releasing](docs/releasing.md): changelog, versioning, and the release workflow
- [All docs](docs/README.md)

## Contributing

Bug reports, fixes, and guides are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Not decided yet.
