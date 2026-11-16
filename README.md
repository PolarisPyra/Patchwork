<p align="center">
  <img src="assets/logo.png" width="96" alt="">
</p>

<h1 align="center">Patchwork</h1>

<p align="center">
  All your coding agents, in one window.<br>
  Claude Code, Codex, Antigravity and OpenCode, with API support. Free, with no account and no telemetry.
</p>

<p align="center">
  <a href="https://github.com/PolarisPyra/Patchwork/releases/latest"><img src="https://img.shields.io/github/v/release/PolarisPyra/Patchwork?label=release&color=9b5de5" alt="Latest release"></a>
  <img src="https://img.shields.io/badge/Linux%20%C2%B7%20Windows%20%C2%B7%20macOS-555" alt="Linux, Windows, and macOS">
  <a href="LICENSE"><img src="https://img.shields.io/github/license/PolarisPyra/Patchwork?color=5bc0eb" alt="MIT License"></a>
</p>

<p align="center">
  <a href="https://patchworkeditor.com">Website</a> ·
  <a href="#download">Download</a> ·
  <a href="docs/README.md">Docs</a> ·
  <a href="https://ko-fi.com/polarispyra">Donate</a>
</p>

<p align="center">
  <img src="docs/screenshots/grid-1.png" alt="A task in progress on router firmware: the agent reads the image header, searches for the checksum, edits the parser, and runs the tests while it explains the CRC-32 it found" width="860">
</p>

<table>
  <tr>
    <td><img src="docs/screenshots/grid-2.png" alt="A new task in the dotfiles project, with the prompt box and a few suggestions for getting started"></td>
    <td><img src="docs/screenshots/grid-4.png" alt="A finished task in a save editor listing the three files it changed, with the Usage panel open showing the 5-hour session and weekly limits"></td>
    <td><img src="docs/screenshots/grid-3.png" alt="Settings on General, with the theme toggle, app icons, and chat backgrounds"></td>
  </tr>
</table>

## What it does

You probably already have an agent CLI you like. Patchwork gives it a proper home: your projects
and tasks in a sidebar, and the agent's work laid out step by step as it goes.

- **Every agent, one window.** Claude Code, Codex, Antigravity, OpenCode, and API models all get
  the same chat, plan, and code views. Switching agents is a dropdown, not a new app.
- **You choose how much it does alone.** Supervised, auto-accept edits, auto, or full access, set
  per task. When it needs permission, it asks in the chat.
- **Set a goal, steer as it goes.** Hand it a `/goal` and walk away, or send a message mid-turn to
  change course.
- **API support.** DeepSeek or any OpenAI-compatible endpoint works from
  **Settings → Providers**.
- **Pick up old sessions.** Import past conversations from Claude Code, Codex, the Gemini CLI, or
  OpenCode and carry on where you left off.
- **It all stays on your machine.** No account, no proxy, no telemetry. Requests go straight from
  your computer to your provider.

There's more: scheduled tasks, skills, MCP servers, hooks, a Usage view for your subscription
limits, a diff of each task's changes, and diagrams or HTML mocks that render right in the chat.

## How it compares

| | Patchwork | Claude desktop app | Codex app | Cursor |
| --- | --- | --- | --- | --- |
| **Agents and models** | | | | |
| Coding agents | Claude Code, Codex, Antigravity, OpenCode | Claude only | Codex only | Its own agent |
| API support | DeepSeek, OpenRouter or any OpenAI-compatible endpoint | Claude models only, through Anthropic or a cloud provider | Not documented for the app | OpenAI-compatible endpoints, not for every feature |
| Imports chats from | Claude Code, Codex, Gemini CLI, OpenCode | Earlier Claude installs | Claude Code, Claude Cowork, Cursor (last 30 days) | Not documented |
| **Getting it** | | | | |
| Platforms | Linux, Windows, macOS (Apple silicon) | macOS, Windows, Linux (beta) | macOS, Windows, Linux (preview) | macOS, Windows, Linux |
| Account | None | Claude account | ChatGPT account | Cursor account |
| Price | Free | Paid Claude plan | ChatGPT plan, free tier included | Free tier, paid from $8 a month |
| Open source | Yes, MIT | No | The CLI is; the app is not stated | No |
| **Working with agents** | | | | |
| Permission modes | Supervised, Auto-accept edits, Auto, Full access | Manual, Accept edits, Plan, Auto, Bypass permissions | Ask for approval, Approve for me, Full access, Custom | Auto-review, Allowlist, Run Everything |
| Scheduled tasks | On your machine | On your machine or in the cloud | On your machine or in the cloud | In the cloud |
| Parallel tasks in worktrees | Yes | Yes | Yes | Yes |
| Diff review | Yes | Yes | Yes | Yes |
| MCP servers | Yes | Yes | Yes | Yes |
| Skills | Yes | Yes | Yes | Yes |
| Hooks | Yes | Not documented for the app | Yes | Yes |
| View plan usage | Yes | Yes | On the web | On the web |

## Download

| Platform | Download |
| --- | --- |
| Linux | [`Patchwork_amd64.AppImage`](https://github.com/PolarisPyra/Patchwork/releases/latest/download/Patchwork_amd64.AppImage) |
| Windows | [`Patchwork_x64.exe`](https://github.com/PolarisPyra/Patchwork/releases/latest/download/Patchwork_x64.exe) |
| macOS (Apple silicon) | [`Patchwork_aarch64.dmg`](https://github.com/PolarisPyra/Patchwork/releases/latest/download/Patchwork_aarch64.dmg) |

These links always point at the latest version. On Linux, run `chmod +x Patchwork_amd64.AppImage`
first. None of them is signed with a developer certificate, so the first time, choose More info →
Run anyway on Windows, and on macOS drag Patchwork to Applications, then Control-click it and
choose Open. Settings → Updates says when a new version is out.

## Getting started

1. Install and sign in to at least one agent: [Claude Code](https://docs.claude.com/en/docs/claude-code),
   [Codex](https://github.com/openai/codex), [Antigravity](https://antigravity.google/), or
   [OpenCode](https://opencode.ai/). Patchwork finds it on its own. No CLI? Add an API provider
   under **Settings → Providers** and use the built-in agent.
2. Add a project folder, click **+** to start a task, and pick an agent.
3. Watch it work, and send a message any time to steer it.

Just want to try something? Start a chat from the top of the sidebar. It runs in a throwaway folder
that goes away with the chat.

## Your data

Tasks run right in your project folder. Settings live in `~/.config/patchwork` and app state in
`~/.local/share/patchwork` (macOS and Windows use their own folders, see
[Architecture](docs/architecture.md)). API keys go in your system keychain, or a file only you can
read if there isn't one. [Agent CLIs](docs/agent-clis.md) lists every file Patchwork reads from
your agents.

## Contributing

Bug reports and fixes are welcome. [Development](docs/development.md) covers building from source,
[CONTRIBUTING.md](CONTRIBUTING.md) covers the rest, and the [changelog](CHANGELOG.md) has every
release.

Patchwork is [MIT licensed](LICENSE).
