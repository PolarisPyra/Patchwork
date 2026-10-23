# Patchwork

Patchwork is a desktop workspace for coding agents. It runs Claude Code, Codex, Antigravity, and
OpenCode through the CLIs already installed on your machine, so there are no new accounts or API
keys to manage. Projects, tasks, and chats share one sidebar, and every change an agent makes is
reviewed in the app before you keep it.


## Features

- **Four agents, one interface.** Claude Code, Codex, Antigravity, and OpenCode share the same
  chat, plan, and diff views.
- **Review before you keep it.** Each task's canvas shows its Plan, Code, and Diff views. Stage the
  files you want with `git add`, or reject a file to restore it.
- **Access modes per task.** Choose supervised, auto-accept edits, auto, or full access. Approval
  requests appear in the chat.
- **Goals and steering.** Set a `/goal` for the agent to work toward, or send a message during a
  running turn to steer it. Supported by Claude Code, Codex, and OpenCode 2.x.
- **Projects and chats.** Projects group their tasks. Standalone chats sit at the top of the
  sidebar and run in a scratch folder that is deleted with the chat.
- **Subscription usage.** The **Usage** button at the bottom of the sidebar shows each provider's
  limits.
- **Import existing work.** Bring in conversations from Claude Code, Codex, the Gemini CLI
  (Antigravity chats), and [T3 Code](https://github.com/pingdotgg/t3code). Each one becomes a task
  in its project, and follow-ups resume the original session.

## Installation

Download `Patchwork_amd64.AppImage` from the
[latest release](https://github.com/PolarisPyra/patchwork/releases/latest), make it executable, and
run it:

```sh
chmod +x Patchwork_amd64.AppImage
./Patchwork_amd64.AppImage
```

Release builds are currently Linux only. To build from source, see
[Development](docs/development.md).

## Requirements

Install at least one agent CLI and sign in to it with the CLI's own login command. Patchwork
detects which CLIs are installed and uses them as they are.

- [Claude Code](https://docs.claude.com/en/docs/claude-code) (`claude`)
- [Codex](https://github.com/openai/codex) (`codex`)
- [Antigravity](https://antigravity.google/) (`agy`)
- [OpenCode](https://opencode.ai/) (`opencode`)

## Getting started

1. Open Patchwork and add a project folder.
2. Click the **+** button in the project to start a task, then pick an agent and an access mode.
3. When the agent finishes, review the changes in the canvas. Stage the files you want and reject
   the rest.

To try an agent without a project, start a chat from the top of the sidebar.

## Your data

- Patchwork stores its state in a SQLite database at `~/.config/patchwork/patchwork.db`.
- Tasks run directly in their project folder. Chats run in a scratch folder under the app's data
  directory, which is deleted with the chat.
- The agent CLIs do the work and talk to their own services. Patchwork reads the files they save,
  such as session transcripts, to show history and usage. [Agent CLIs](docs/agent-clis.md) lists
  every file and command it touches.

## Documentation

- [Development](docs/development.md): build, run, and test Patchwork
- [Architecture](docs/architecture.md): repo layout and the data model
- [Agent CLIs](docs/agent-clis.md): how each agent is detected, run, and imported
- [Releasing](docs/releasing.md): changelog, versioning, and the release workflow
- [All docs](docs/README.md)

Other project files:

- [CONTRIBUTING.md](CONTRIBUTING.md): how to send a bug report, fix, or guide
- [CHANGELOG.md](CHANGELOG.md): release history, generated from commit subjects
- [AGENTS.md](AGENTS.md): instructions for coding agents working in the repo
- [CLAUDE.md](CLAUDE.md): project rules for Claude Code
- [UI checks](scripts/qa/README.md): browser measurements and screenshots of the dev session


## Contributing

Bug reports, fixes, and guides are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Patchwork is released under the [MIT License](LICENSE).
