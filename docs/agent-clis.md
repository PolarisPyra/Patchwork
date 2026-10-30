# Agent CLIs

Patchwork has no model access of its own. It runs the `claude`, `codex`, and `agy` CLIs that are
installed and signed in on the machine, and reads some of their files. This page lists every
point where it touches them.

## System prompts

Settings → Providers gives each agent a system prompt of its own: a System Prompt button on each
CLI's row, and a field in each API provider's form. Each agent gets it last, after Patchwork's own
instructions:

| Agent | How | A chat already started |
| --- | --- | --- |
| Claude Code | The end of `--append-system-prompt`, with `--system-prompt-snapshot off` | Gets the new prompt. Without the flag, Claude resumes with the prompt it recorded on the conversation's first request. A saved change also stops idle Claude processes, which were launched with the old one. |
| Codex | The end of `developerInstructions` on `thread/start` and `thread/resume` | Keeps the prompt it started with: `thread/resume` and `collaborationMode.settings.developer_instructions` do not replace it (checked with codex-cli 0.165.0). |
| OpenCode 2.x | The `patchwork-system-prompt` instruction entry on the session (`PUT /api/experimental/session/:id/instructions/entries/:key`), removed when the prompt is cleared. A server without the API runs the turn without it and says so on the bridge's stderr, in the agents' log. | Gets the new prompt. |
| `opencode run` | A file in its `instructions` config | Gets the new prompt. |
| Antigravity | A `SessionStart` hook in the `--add-dir` folder's `.agents/hooks.json` runs `patchwork --system-prompt-hook`, which answers with the prompt from `PATCHWORK_SYSTEM_PROMPT` as an `injectSteps` system message. Rules such as a `GEMINI.md` in an `--add-dir` folder are not read (checked with agy 1.4.11). | Keeps the prompt it started with: `SessionStart` runs only for a new conversation. |
| API providers | The end of Patchwork's system prompt, after the project's `AGENTS.md` or `CLAUDE.md` | Gets the new prompt. |

Antigravity's `hooks.json` keys each hook by a name (`patchwork-pretooluse-0`, …): a tool event
holds `{matcher, hooks}` groups, and `SessionStart` lists its commands directly. It refuses a
file in Claude Code's shape.

## The shell each agent runs

Each CLI picks the shell its commands run in, so the shell chosen in Settings is not always the
one an agent gets. `provider::agent_shell` follows each CLI's rule, the turn sets `SHELL` to it,
and the agent's instructions name it:

| Agent | Shell |
| --- | --- |
| Claude Code | Only bash or zsh: the chosen shell if it is one of them, else zsh, else bash |
| Codex | The account's login shell, whatever `SHELL` says |
| OpenCode, Antigravity, API providers | The chosen shell if it reads POSIX commands, else bash, else `sh`. OpenCode refuses fish and Nushell itself, and Patchwork's own agent calls its tool `bash` |

On Windows every agent gets the chosen shell. Plugin hook commands run in the user's shell, or
bash when it is fish or Nushell. The terminal panel always runs the chosen shell.

## Claude Code (`claude`)

| | |
| --- | --- |
| Detect | `claude --version`, then `claude auth status`, whose JSON says whether and how it is signed in. Home is `$CLAUDE_CONFIG_DIR` or `~/.claude`. |
| Default model | `$ANTHROPIC_MODEL`, else `model` in `~/.claude/settings.json`. |
| Run | `claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --permission-mode <mode> --permission-prompt-tool stdio [--resume <session>] [--model <m>] [--effort <e>] --system-prompt-snapshot off --append-system-prompt <installed shells, then the system prompt from Settings → Providers>` |
| Access modes | Supervised `default`, Auto-accept edits `acceptEdits`, Auto `auto`, Full access `bypassPermissions`. |
| Prompt | A stream-json `user` line on stdin, with images as base64 blocks. stdin stays open for the turn. |
| Output used | `stream_event` text and reasoning deltas; `assistant` tool calls, token usage, and synthetic command output; `user` tool results; `system` init (session id) and the task events `task_started`, `task_updated`, `task_progress`, and `task_notification`; `rate_limit_event` (usage limits); `result` (end of turn, errors). `TodoWrite` becomes the plan, edit tools become touched files, `Task`/`Agent` calls become subagents. |
| Approvals | `control_request` `can_use_tool` becomes an Allow / Deny request, answered with a `control_response`. In Full access every request is allowed at once, except `AskUserQuestion`. |
| Questions | `AskUserQuestion` arrives as `can_use_tool` in every mode. The answers go back in `updatedInput.answers`, by question text. |
| Permission rules | A `PreToolUse` hook from `--settings` runs Patchwork's binary before every `Bash`, `Edit`, `Write`, `MultiEdit`, `NotebookEdit`, `Read`, `Glob`, `Grep`, `LS`, `WebFetch`, and MCP call, and asks the relay. It is installed on every turn, and the rules are read at each call: a rule saved or changed mid-turn decides the calls after it, and answers one already held. It is also the only way to stop a call in full access, which auto-approves every tool before `can_use_tool` is consulted. |
| Steer | A `user` line with `"priority": "now"`, taken at the next tool boundary. |
| Subagents | Stopped with a `stop_task` control request. |
| Goals | `/goal <objective>` and `/goal clear` are sent as messages and handled by Claude; its `Goal set` / `Goal active` / `Goal cleared` replies and `Stop hook feedback` checks are read from the stream. Whether a goal was met is read from the session transcript, `~/.claude/projects/*/<session>.jsonl`. |
| Compaction | `/compact` is sent as a message, when the user sends it. |
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
| Permission rules | A `PreToolUse` hook runs Patchwork's binary before every `Bash`, `apply_patch`, and MCP call and asks the relay. It is installed on every turn, whether or not a rule guards anything yet, and the rules are read at each call: a rule saved or changed mid-turn decides the calls after it, and answers one already held. |
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
| Permission rules | The same hook before every tool call, in the hooks file of a folder `--add-dir` adds. Installed on every turn, so the rules in force decide each call as it happens. |
| Goals | `/goal <objective>` is sent as the prompt. The goal counts as met when the run ends cleanly, since it does not stop before verifying it. Clearing sends "Stop the current goal." because it has no clear command. |
| Import | Chats saved by the earlier Gemini CLI, under `~/.gemini/tmp/<project>/chats`, with folders from `~/.gemini/projects.json`. |
| Usage limits | `agy -p /usage --output-format json`. |

## OpenCode (`opencode`)

| | |
| --- | --- |
| Install | Not installed by Patchwork. `opencode` on `PATH` or in the usual install folders is used, else `~/.opencode/bin/opencode`, where its installer puts it. |
| Detect | `opencode --version` (`1.19.4`, or `v2.1.3` for 2.x). Its free models need no account, so an installed CLI counts as signed in; other providers are added with `opencode auth login`. |
| Models | `opencode models`, one `provider/model` per line. |
| Run (2.x) | Each turn starts Patchwork's own binary with `--opencode-bridge` (`apps/server/src/provider/opencode/bridge.rs`). The bridge starts a private `opencode serve --hostname 127.0.0.1 --port 0` in the task's folder, with `PWD` set to it and a password it generates in `OPENCODE_PASSWORD`, and stops it when the turn ends. It creates or resumes the session (`POST /api/session`, `GET /api/session/<id>`, then `POST .../model` and `.../agent`), writes its permission rules (`PATCH /api/session/<id>`), subscribes to `GET /api/event`, and sends the message (`POST /api/session/<id>/prompt`, with images as `files`). The turn's first stdin line is the turn; later lines are approval answers and steering messages. |
| Run (1.x) | `opencode run --format json --thinking [--auto] [--agent plan] [--session <id>] [--model <provider/model>] [--variant <effort>] [--file <image>]... -- <prompt>`. The prompt is an argument and stdin is closed, since `run` otherwise waits to read it. |
| Effort | 2.x sends it as the model's `variant`, 1.x as `--variant`. |
| Access modes | 2.x: every session rule names an action a permission rule can answer (`shell`, `edit`, `external_directory`, `webfetch`, MCP ids) and makes it `ask`, in every mode, so Patchwork decides each call when it arrives, by the rules in force then and the access mode. Full access and Auto also allow everything else; a tool caught in a loop stays OpenCode's own call. Plan mode denies `edit`. 1.x: Full access `--auto`; Auto-accept edits and Supervised pass the same checks in `OPENCODE_CONFIG_CONTENT`, and since a run cannot ask, an `ask` rule refuses the call and the rules are only as fresh as the launch. |
| Approvals and questions | 2.x only. `permission.asked` becomes an approval; it is answered with `POST /api/session/<id>/permission/<request>/reply` and `{"decision": "once"}` or `"reject"`. `question.asked` is answered with `.../question/<request>/reply` and the answers in order, or `.../reject`. |
| Steering | 2.x only. A message sent during a turn is posted to `/prompt` with `"delivery": "steer"`; `session.inbox.delivered` confirms it was taken. 1.x waits for the next turn. |
| Output used | 2.x, from the event stream: `session.text.delta`, `session.reasoning.delta`, `session.tool.called` / `.success` / `.failed` (the name comes from `session.tool.input.started`), `session.step.ended` (token counts), `session.execution.succeeded` / `.failed` / `.interrupted` (the turn ends). 1.x: `step_start` (session id), `reasoning`, `text`, `tool_use`, `step_finish` (token counts), `error`. |
| Not available | Slash commands, `/goal` included. |
| Import | Sessions from its SQLite database, `~/.local/share/opencode/opencode.db`. |
| Usage limits | None to show: it bills through the providers it is signed in to. |

## Background tasks

Work an agent starts that runs past its reply is listed under the message it started in until it
ends, with a stop button when the agent can stop it.

- Claude: a task is listed when it starts in the background, or when `task_updated` moves it there
  (a foreground command that overruns its timeout is moved this way). Each task is named by the
  top-level `task_id` of its `task_started`, `task_updated`, `task_progress`, and
  `task_notification`. A status in `task_updated` and the notification both end it. The CLI sent the
  status first in both captures, but either order works; each adds what it has (the notification carries the output file, which is
  empty for a subagent's own command). Progress is the line text `summary` or `description`.
  Every task the CLI reports holds stdin open until it ends, whatever its kind, because the CLI
  reports the end to the model and the turn's reply can end before it. Stopped with `stop_task`.
- Codex: a command or subagent thread still open when its turn completes is listed without a stop
  button, since Codex has no request that stops one. Its only stop is Stop, which ends the turn. It
  holds nothing open, so its turn ends with it as before. Codex does not wake the agent when one
  ends; the agent sees the result only when it polls.

Not done yet: tailing a task's output file in the UI, resuming a task by sending it a message
(`send_task_message`), keeping tasks alive across an app restart, and Monitor's per-line events.

## API providers

Not a CLI: Patchwork runs these itself, for any OpenAI-compatible Chat Completions endpoint.

| | |
|---|---|
| Configuration | `providers.json` in the config folder: each provider's id, name, preset, base URL, models, extra header names, and its system prompt, and a custom provider's logo (a PNG `data:` URL scaled to 128px), with no keys or header values. The base URL must start with `http://` or `https://`. Saving, fetching models, and each turn check it. Model ids are `<provider id>/<model id>`. |
| Keys | The system credential store through the `keyring` crate, under the service `dev.patchwork.ade` and the account `api-provider:<id>`. Header values are stored the same way, as a JSON array under `api-provider:<id>:headers`. Without a store, `api-keys.json` beside the config, mode 0600. A provider can name an environment variable instead, read from the app's environment or else the login shell's. |
| Run | Each turn starts Patchwork's own binary with `--api-bridge` (`apps/server/src/provider/api/bridge.rs`). The first stdin line carries the connection, key included, so the key is never in the arguments or environment. It posts to `<base URL>/chat/completions` with `stream: true` and the tools, runs the calls the model makes, and repeats until a reply makes none. Failed requests (429, 5xx, network) are retried 3 times. A reply that sends nothing for 180 s is stopped; a long reply that keeps streaming is not. When the provider refuses a request as too long for its context (a 413, or a 400, 415, or 422 whose body names the context or prompt length, as OpenAI, DeepSeek, OpenRouter, Groq, Mistral, xAI, Fireworks, vLLM, LM Studio, and llama.cpp word it), all but the last 8 tool results are cut to a note and the request is sent again, then all but the last 2, then all of them. If it is still too long, the conversation is summarized as `/compact` does, leaving out the oldest messages until the summary request fits, and the user's latest message follows the summary word for word. Ollama does not refuse: it drops the oldest messages itself. A request refused for its images is sent again without them, with a note in their place; DeepSeek gets that note from the start, since its API takes text only. `bash` keeps at most 1 MB of each output stream, the start and the latest, and the model is shown 40 KB. `grep` stops at 200 matches. |
| Tools | `read_file`, `list_dir`, `glob`, `grep`, `fetch_url`, `ask_user`, `update_plan`, and, outside plan mode, `write_file`, `edit_file`, `bash` (`apps/server/src/provider/api/tools.rs`). The system prompt adds the project's `AGENTS.md` or `CLAUDE.md`. |
| Sessions | The conversation is saved as JSON in `api-sessions/<id>.json` in the data folder after every step, once its tool calls are done. |
| Effort | Sent as `reasoning_effort`. |
| Access modes | Full access runs everything. Auto asks only before touching paths outside the project. Auto-accept edits also asks before `bash`, and Supervised before edits and `fetch_url` too. Plan mode offers only the read-only tools and `propose_plan`. |
| Steering | A message sent during a turn joins the conversation after the current tool calls. |
| Output used | `reasoning_content` (DeepSeek) or `reasoning`, `content`, `tool_calls`, and `usage` (with `prompt_tokens_details.cached_tokens` or DeepSeek's `prompt_cache_hit_tokens`). |
| Slash commands | `/compact` summarizes the conversation and keeps the summary. No `/goal`. |

## Against the official docs

Checked on 2026-10-30 against [Claude Code and the Claude API](https://platform.claude.com/docs/en/home),
the [Codex app-server and OpenAI API](https://developers.openai.com/api/docs), and the
[OpenCode v2 API](https://opencode.ai/v2/docs/api). Everything not listed here uses the documented
call. *Unconfirmed* means the docs did not say either way.

### Claude Code

Not as documented:

- **Output tokens.** They are read from each `assistant` message's `usage`, where `output_tokens`
  is only the placeholder from `message_start`. The real totals are in the `result` message
  (`usage`, `modelUsage`, which also counts subagents and compaction).
- **Steering.** It sends `priority: "now"` with no `origin`, which the SDK documents as an
  interrupt. `origin: {kind: "human"}` reads the message in the same turn, and `next` waits for
  the next tool boundary, as this page says steering does.
- **Approvals.** `--permission-prompt-tool stdio` and the raw `control_request` shapes are not
  documented values; the allow and deny answers match the SDK's `canUseTool`.
- **Usage limits.** The `api/oauth/usage` and `/profile` endpoints and `rate_limit_event` are
  undocumented.
- **Fork** copies messages into a new session instead of `--resume <id> --fork-session`, so the
  fork loses the cache and the CLI's own history.

Not used yet:

- `compact_boundary` (manual or auto, tokens before) and the `get_context_usage` control request,
  which reports the context percentage and the auto-compact threshold.
- `set_permission_mode` and `set_model`, which change mode and model without a respawn; the `plan`
  and `dontAsk` modes.
- `updatedPermissions` from a request's `suggestions`, the documented "always allow".
- The interrupt receipt and `cancel_queued`, which say which queued messages a stop dropped.
- `permission_denied` messages and `permission_denials` on `result`.
- `total_cost_usd`, `tool_progress` heartbeats, `subagent_retry`, and the goal being cleared after
  an unrecoverable error.

Unconfirmed: whether any structured goal event exists. Not passed: `--thinking-display summarized`,
which holds back a reply's text after a think and sends it in one burst as the think ends.

### Codex

Not as documented:

- **Reply text.** It is read only from `item/completed`, so a reply appears whole;
  `item/agentMessage/delta` streams it.
- **Usage limits.** They are read from rollout files on disk, not `account/rateLimits/read` and
  `account/rateLimits/updated`, which also give per-model buckets.
- **Models.** They come from `codex debug models`, not `model/list`, which gives each model's
  efforts, default and input types. `model/list` may not carry the context window (unconfirmed).
- **Subagents.** They are read from `subAgentActivity` items, which the docs do not list; the
  documented item is `collabToolCall`.
- **Approval policy.** It is sent as `on-request`, while the docs spell it `onRequest`.

Not used yet:

- `acceptForSession` and `acceptWithExecpolicyAmendment` on approvals. `item/permissions/requestApproval`
  and `mcpServer/elicitation/request` are refused.
- `thread/fork`, `thread/revert`, and `review/start`.
- `thread/backgroundTerminals/list` and `terminate`, which would let background tasks be stopped.
- `codexErrorInfo` codes (`ContextWindowExceeded`, `UsageLimitExceeded`, `Unauthorized`), so every
  error shows its raw message.
- Reasoning section breaks (`summaryPartAdded`), raw reasoning (`textDelta`), and `tokenBudget` on
  `thread/goal/set`.

Unconfirmed: `developerInstructions` on `thread/start`, `approvalsReviewer`, and the
`<proposed_plan>` convention.

### OpenCode

Every route Patchwork calls exists with the documented method and body. Not as documented:

- **Questions.** `question.asked` and its reply routes are not in the v2 docs, which use forms
  (`form.created`, `POST .../form/{id}/reply`). Both are handled.
- **Plan.** It is read from the `todowrite` tool's input; the docs list no todo route or event.
- **Event stream.** A dropped `/api/event` stream fails the turn. The documented
  `/api/experimental/session/{id}/log?after=` resumes from a cursor.

Not used yet:

- Child sessions (`parentID`), so subagents are not shown.
- `fork`, `revert`, `diff`, and the steering inbox (cancel or re-queue, `delivery: "queue"`).
- `always` and a message on permission replies, and saved permissions.
- `session.usage.updated` and `cost`, `retry.scheduled`, and `step.failed`, so retries do not show.
- `GET /api/agent`, `/api/model` and `/api/provider`.

Unconfirmed: the event field names Patchwork reads (`tokens`, `delta`, the tool events), and
whether OpenCode's own automatic compaction runs alongside Patchwork's.

### API providers

They speak only Chat Completions. For OpenAI's reasoning models the docs point to the Responses
API instead. Through Chat Completions those models return no reasoning text, and their reasoning
is not carried across tool calls. Since GPT-5.4 they also reject tools with any effort but `none`,
and every turn sends tools. No provider speaks Anthropic's Messages API, so thinking signatures,
`pause_turn`, prompt caching, and server-side compaction are out of reach.

## T3 Code

Imports only: `~/.t3/userdata/statev2.sqlite` is opened read-only, and its Claude and Codex
threads are copied in with their session ids, so follow-ups resume the original CLI session.
