# Agent CLIs

Patchwork has no model access of its own. It runs the `claude`, `codex`, and `agy` CLIs that are
installed and signed in on the machine, and reads some of their files. This page lists every
point where it touches them.

## Claude Code (`claude`)

| | |
| --- | --- |
| Detect | `claude --version`, then `claude auth status`, whose JSON says whether and how it is signed in. Home is `$CLAUDE_CONFIG_DIR` or `~/.claude`. |
| Default model | `$ANTHROPIC_MODEL`, else `model` in `~/.claude/settings.json`. |
| Run | `claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --permission-mode <mode> --permission-prompt-tool stdio [--resume <session>] [--model <m>] [--effort <e>] --append-system-prompt <installed shells>` |
| Access modes | Supervised `default`, Auto-accept edits `acceptEdits`, Auto `auto`, Full access `bypassPermissions`. |
| Prompt | A stream-json `user` line on stdin, with images as base64 blocks. stdin stays open for the turn. |
| Output used | `stream_event` text and reasoning deltas; `assistant` tool calls, token usage, and synthetic command output; `user` tool results; `system` init (session id) and background task start and end; `rate_limit_event` (usage limits); `result` (end of turn, errors). `TodoWrite` becomes the plan, edit tools become touched files, `Task`/`Agent` calls become subagents. |
| Approvals | `control_request` `can_use_tool` becomes an Allow / Deny request, answered with a `control_response`. In Full access every request is allowed at once, except `AskUserQuestion`. |
| Questions | `AskUserQuestion` arrives as `can_use_tool` in every mode. The answers go back in `updatedInput.answers`, by question text. |
| Steer | A `user` line with `"priority": "now"`, taken at the next tool boundary. |
| Subagents | Stopped with a `stop_task` control request. |
| Goals | `/goal <objective>` and `/goal clear` are sent as messages and handled by Claude; its `Goal set` / `Goal active` / `Goal cleared` replies and `Stop hook feedback` checks are read from the stream. Whether a goal was met is read from the session transcript, `~/.claude/projects/*/<session>.jsonl`. |
| Compaction | `/compact` is sent as a message. An idle chat whose context is over the threshold is compacted just before its prompt cache expires (an hour after the last reply). |
| Import | Sessions in `~/.claude/projects/<folder>/*.jsonl`: titles, user and assistant text, session id, and folder. |
| Usage limits | The OAuth token from `~/.claude/.credentials.json` (or the macOS keychain), sent to `api.anthropic.com/api/oauth/usage` and `/profile`; during a turn, `rate_limit_event` updates them. The token is never refreshed or written. |

## Codex (`codex`)

| | |
| --- | --- |
| Detect | `codex --version`, then `codex login status`. Home is `$CODEX_HOME` or `~/.codex`. |
| Default model | `model` in `~/.codex/config.toml`, or in its selected `profile`. |
| Run | `codex app-server`, spoken to in JSON-RPC over stdin and stdout: `initialize`, `initialized`, then `thread/start` or `thread/resume` (with the saved thread id), then `turn/start` with the prompt, attachments, model, and effort. |
| Access modes | Sent with `turn/start` as approval policy / sandbox: Supervised `untrusted` / `readOnly`, Auto-accept edits `on-request` / `workspaceWrite`, Auto `on-request` / `workspaceWrite` with Codex's own reviewer, Full access `never` / `dangerFullAccess`. |
| Output used | `item/started` and `item/completed` for messages, commands (with their output), MCP calls, web searches, and file changes; `item/reasoning/summaryTextDelta` (reasoning); `turn/started`, `turn/plan/updated`, `turn/completed`, `error`; `thread/goal/updated` and `thread/goal/cleared`. Token usage is not read from the stream. |
| Approvals | `item/commandExecution/requestApproval` and `item/fileChange/requestApproval`, answered `accept` or `decline`. Any other request is refused. |
| Questions | `item/tool/requestUserInput`, answered with the picks by question id. |
| Steer | `turn/steer` with the running turn's id. Without one, the message waits for the next turn. |
| Goals | `/goal` commands become `thread/goal/set`, `get`, and `clear`. A new goal is set paused, its first turn starts, and then it is set active. When a turn ends with the goal active, stdin stays open 5 seconds so Codex can start its next turn itself. **Stop** first sets the goal paused. |
| Import | Rollouts in `~/.codex/sessions/**/*.jsonl` and titles from `~/.codex/session_index.jsonl`. |
| Usage limits | The newest `token_count` rate limits in the most recent rollout files. Nothing is fetched. |

## Antigravity (`agy`)

| | |
| --- | --- |
| Install | **Settings** can install it: Patchwork downloads `https://antigravity.google/cli/install.sh` and runs it with `--dir ~/.config/patchwork/clis`. That copy is used before one on `PATH`. |
| Detect | `agy --version`. There is no login check; a run that is not signed in fails. |
| Run | `agy --output-format stream-json <mode> [--conversation=<id>] [--model=<m>] [--effort=<e>] --print=<prompt>`. The prompt is an argument and nothing is written to stdin. |
| Access modes | Supervised no flag (it skips actions that need approval), Auto-accept edits `--mode=accept-edits`, Auto `--mode=auto`, Full access `--dangerously-skip-permissions`. |
| Output used | `init` (conversation id), `step_update` text deltas and tool steps, `result` (errors). The turn ends when the process exits. No reasoning, plan, or token usage is reported. |
| Not available | Approvals, questions, and steering: in `--print` mode it cannot ask anything, so messages sent during a turn wait for the next one. |
| Goals | `/goal <objective>` is sent as the prompt. The goal counts as met when the run ends cleanly, since it does not stop before verifying it. Clearing sends "Stop the current goal." because it has no clear command. |
| Import | Chats saved by the earlier Gemini CLI, under `~/.gemini/tmp/<project>/chats`, with folders from `~/.gemini/projects.json`. |
| Usage limits | `agy -p /usage --output-format json`. |

## T3 Code

Imports only: `~/.t3/userdata/statev2.sqlite` is opened read-only, and its Claude and Codex
threads are copied in with their session ids, so follow-ups resume the original CLI session.
