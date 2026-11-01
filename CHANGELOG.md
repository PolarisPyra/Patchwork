# Changelog

All notable changes to Patchwork are documented here, for the people who use it.
Each release adds its notes at the top; see docs/release.md.

## [26.3.9] - 2026-11-01

### Changed

- You can now switch off every agent and API provider in Settings → Providers, including the last one, instead of the last switch staying locked. A new chat started with nothing switched on can't be sent and says "No agents connected. Turn one on in Settings → Providers." instead of quietly using Claude Code.

### Fixed

- Clicking Usage at the bottom of the sidebar no longer turns the whole window blank when no agent with plan limits is switched on, such as when you use only API providers. The Usage popover now lists only the agents switched on in Settings → Providers, and says "Unable to get usage stats" when the ones you use have none to show.
- Errors in the Window log now say what went wrong, not only where it happened.

## [26.3.7] - 2026-11-01

### Fixed

- The agent menu in the chat box no longer runs off the top of the window when a project has many agents. It shows about a dozen at a time and scrolls the rest.

## [26.3.6] - 2026-11-01

### Added

- The agent button in the chat box's toolbar lists the agents a chat can summon, including the project's own. Pick one to make it this chat's agent, or pick No agent to remove it. Each agent's pencil opens its file in your editor.

## [26.3.5] - 2026-11-01

### Changed

- The task list above the message box shows its progress as one bar that fills as tasks finish, instead of a strip of dots that slid along with the current task.

### Fixed

- Stopping a reply keeps your queued messages queued instead of marking them as sent. A message the agent never answered comes back as queued, and a message sent just as a reply starts is refused with a notice while your draft comes back.
- If a message fails to send, your draft and attachments come back in that chat, where the error shows. Resuming a queued message keeps its pictures and dropped files, and restoring a checkpoint keeps a draft you have started writing.
- Switching projects opens a chat from the project you picked, and a new chat is no longer replaced when an older chat finishes loading.
- Tab and Space work on the buttons, links, and menu items that have focus. They no longer move focus to the message box or toggle the sidebar while one of those has focus.
- An attached picture that can no longer be shown, because its file is gone or its preview fails, shows its file icon instead, and is tried again the next time it appears.
- Closing a terminal now fully ends its shell, instead of leaving it behind as a finished process.
- A key press the agent makes with an unknown key name no longer leaves Shift, Control, or Alt stuck down.
- A scheduled task whose time falls in the hour the clocks jump forward now runs an hour late, instead of switching the schedule off. A chat started by a scheduled task shows the task's name in the sidebar right away.
- A plugin hook that never reads its input now stops at its timeout, instead of holding the agent's tool call past it.
- Servers from an older MCP settings file are moved over even when an earlier move was cut short or a server name is unusual.

## [26.3.4] - 2026-11-01

### Added

- Screen readers and desktop automation tools can now read and use every part of the app: every button and field has a name, menus, tabs and dialogs say whether they are open or selected, errors are announced, and the sidebar, file panel and terminal panel resize with the arrow keys. Buttons that appear on hover, file mentions in your messages, terminal tabs and subagent details can all be reached with the keyboard, and the loading indicators in onboarding, API provider settings and the usage popup now use the steady spinner.

### Changed

- Agents connected through an API provider hold up better on long tasks: condensing a long conversation keeps the most recent exchanges word for word, very long command output is saved to a file the agent can read back instead of filling the conversation, a request that fails for a moment is tried again, and an agent that keeps repeating the same call is told to stop. Sending /compact while a reply is running now condenses the conversation without ending the reply.

### Fixed

- The Tasks bar above the chat box now highlights the task the agent is working on, and its dots slide along so that task is always in view, with a count of the tasks before and after it. Before, it could highlight an earlier task, light up two tasks at once, or name the wrong task once a reply stopped.

## [26.3.2] - 2026-10-31

### Fixed

- A long reply no longer stops early when the agent condenses its conversation. The agent keeps its open task list through the condense, so it finishes those tasks instead of stopping with them unfinished.

## [26.3.1] - 2026-10-31

### Removed

- A reply no longer stops with "The agent ran longer than 4 hours and was stopped." The agent now works until it finishes or you stop it.

## [26.3.0] - 2026-10-31

### Fixed

- With a vision model set in Settings → API Providers, the model you chose for the task now does the work when you send an image. The vision model first describes the image, copying any text in it, and the task's model answers from that description; before, the whole reply went to the vision model and the task's model never saw the image.

## [26.2.9] - 2026-10-31

### Changed

- Once every step in the task list above the chat box is done, the list shows it complete for a moment and then folds away; a new list brings it back. Background tasks now sit above the task list instead of below it.

### Fixed

- The task list above the chat box now follows the agent's progress step by step. Claude Code agents on newer models keep it with the task tools they actually have, instead of looking for one they lack, and every agent is told to mark each step in progress and done as it goes.
- Pasting a message of short lines, file paths, names or Markdown lists into the chat box keeps it as text with its line breaks, instead of turning it into a code block. Words quoted in backticks paste as inline code.
- The model menu no longer fails with "GitHub answered 404 Not Found". Claude Code, Codex and Antigravity models now come straight from each agent's installed CLI, so the list matches what that agent can run, with Claude Code's older models still under their own heading.
- Chats with API providers now compact their context when it fills up, also after you change the compact window while the agent is replying, and compacting a long chat takes seconds instead of minutes. While it runs, the line at the top of the reply says "Compacting context" with the same shimmer as "Working for"; afterwards a "Context compacted" line marks the spot, and clicking it shows the summary the agent carried on from.

## [26.2.8] - 2026-10-30

### Fixed

- The Usage popover at the bottom of the sidebar now lists OpenCode when it is switched on in Settings. Before, OpenCode never appeared there. Its panel says OpenCode has no plan limits to show.

## [26.2.7] - 2026-10-30

### Changed

- A reply's reasoning shows again as "Thinking" and "Thought" rows beside its tool calls; click one to read the full reasoning. The line at the top of a reply now says "Working for 12s" while the agent works and "Worked for 12s" when it is done, instead of "Thinking for" and "Thought for".
- An HTML or SVG artifact that draws nothing — a bare broken image, or a script that failed before it changed the page — now shows "This artifact rendered nothing visible." in a tidy box instead of an empty frame with a broken-image icon.
- Each of Patchwork's own MCP servers is now kept in its own file, named after the server, in an MCP folder inside Patchwork's config folder, instead of all in one shared file. Your servers move over on the next start, and Settings → MCP opens each server's own file.
- The system prompt you give an API provider in Settings → Providers now comes after the project's own instructions file, so a project can no longer override it, as with the other agents.
- While an agent works, its reply now shows "Thinking for 12s" with a grey shimmer that becomes "Thought for 12s" when done, and the separate reasoning rows are gone. Text now appears word by word even when it arrives together with a tool call, and the chat no longer jumps as a reply starts, grows or ends. The plan above the chat box now opens collapsed and keeps naming the step a stopped reply was on.

### Fixed

- The system prompt you set for Antigravity in Settings → Providers now reaches it at the start of each new chat. Before, Antigravity never saw it, and a plugin hook that runs when a chat starts could stop Patchwork's permission rules from applying to Antigravity.
- Permission rules for editing files now also apply when Antigravity writes a file. Before, its file writes slipped past those rules.
- A change to Claude's system prompt in Settings → Providers now applies to chats you already started, from their next message. Before, a chat kept the prompt it began with.
- In the changed files card under a reply, each file's added and removed line counts now line up in columns against the right edge, and a note such as "No changes" sits at that same edge.
- The icons on the steps in a reply now line up with the words of the line above them, instead of sitting indented to the right.
- Claude's reply now streams in word by word after a long think, instead of appearing all at once when the thinking ends.

### Removed

- Long agent turns on API providers no longer stop with "Stopped after 400 model requests in one turn". A turn now runs until the agent finishes or you stop it.

## [26.2.0] - 2026-10-30

### Changed

- A step that is still running in a reply, such as Thinking, now shows a smooth grey shimmer across its label instead of a spinner beside it.

### Fixed

- A new chat now appears in the sidebar the moment you add it, instead of after a pause of a second or two, and the compaction ring in the chat box no longer turns into an arrows icon for a moment when you add a chat.
- Holding Ctrl+W in the sidebar now archives chat after chat until the list is empty, instead of stopping partway and moving the cursor into the chat box.
- Long chats you have already opened now open as quickly as short ones, and their file cards and images show at once instead of popping in.
- The progress chips in the Tasks box above the chat box now always sit at its right edge, also before the agent has started a step.

## [26.1.9] - 2026-10-30

### Changed

- In Settings → Providers, the Vision model picker now sits under the API providers, and Default for new tasks is a plain dropdown. Model pickers in Settings call their empty choice Default, and dropdowns there no longer darken or flash blue when clicked. The Computer use switch now decides whether the agent may use the screen without asking, and Permissions lists only Run commands for now.

## [26.1.8] - 2026-10-30

### Changed

- In Settings → Providers, the Prompt button is now System Prompt, for agent CLIs and API providers alike. Its editor has a Clear button that empties the text, so you can remove a system prompt; nothing changes until you press Save.

## [26.1.7] - 2026-10-30

### Changed

- Settings → Providers: each API provider, such as DeepSeek, now has a Prompt button next to Edit, like the agent CLIs, instead of its system prompt sitting in the Edit form. Prompt opens the field under the provider with Save and Cancel under it, and clicking away no longer saves.
- In the Usage popover, "Up to date" now swaps back to the plan badge at once when it is done, instead of fading out slowly.

## [26.1.6] - 2026-10-29

### Fixed

- Refreshing Antigravity in the Usage popover now briefly shows "Up to date" when there is nothing new, like Claude Code and Codex. Before, it showed nothing at all.
- The spinners in a running reply now sit level with their text instead of a little too high. The one beside "Working for" is now the same size as the others, sits just as close to its text, and lines up with the step icons below it.

## [26.1.5] - 2026-10-29

### Changed

- The Usage popover at the bottom of the sidebar now always shows Claude Code, Codex and Antigravity when they are installed, so Antigravity no longer disappears, and it no longer lists API providers, which have no plan limits. Its refresh button refreshes only the provider you are looking at, and briefly shows "Up to date" when there is nothing new. When Anthropic is rate-limiting usage checks, Claude Code's panel shows a red "Rate limited" with the minutes left until it checks again.

### Fixed

- The Tasks list above the chat box now fills in for Claude Code, Codex and OpenCode as they plan and work through a task, not only for API providers such as DeepSeek. Their task updates no longer show up as separate steps in the reply, since the list already shows them.
- While a reply is running, the spinner and text of the "Working for" line now line up with the steps below it, such as "Thinking". They used to sit a little to the left.

## [26.1.4] - 2026-10-29

### Fixed

- The compact button under the chat box now shows how full the context is when it is set to When full. Its ring used to stay empty there, and now it fills against how much the chat's model holds, for Claude Code and Codex models. When Patchwork can't tell how much the model holds, the button shows a compress icon instead of an empty ring.

### Removed

- The compact button under the chat box and Settings → Tokens no longer offer 1M and 2M. A chat already set to one of them keeps it.

## [26.1.3] - 2026-10-29

### Added

- Settings → Providers can now give each provider its own system prompt: a Prompt button on the rows for Claude Code, Codex, OpenCode and Antigravity, and a System prompt field in each API provider's form, such as DeepSeek's. A provider's prompt is sent when a task on it starts, after Patchwork's own instructions. Every prompt is blank until you write one.

### Changed

- A plugin that only runs hooks now needs just one file in its folder, holding its hooks, and is named after the folder. The separate manifest file is no longer needed, and Settings → Hooks says so when there are no plugins yet.

### Fixed

- A failed command that runs several programs in a row is now labelled "Failed command" in the chat. Before, it was named after its first program, so a failure in a later part read as, for example, "Failed git" even when git had worked.
- In the Changes list, every file's added and removed line counts now sit right next to its diff button. Before, files with few removed lines left a gap there, so some counts looked far from the button.
- Agents are now told the shell their commands really run in. Claude Code only runs bash or zsh and Codex always uses your login shell, so picking another shell in Settings used to tell them the wrong one. When your shell is fish, plugin hooks and API provider agents now run their commands in bash, which they are written for, and agents on zsh are warned about the quoting zsh needs.

## [26.1.2] - 2026-10-29

### Changed

- Patchwork's config folder now holds only what you edit: your skills, agents, plugins, MCP servers and API providers. Chat history goes to the system's data folder, and files Patchwork rebuilds on its own go to the cache folder. Logs go to the system's log folder. Your existing files move there by themselves the first time this version opens, and leftovers from older versions are removed. A plugin's hooks can now be written inside the plugin's own manifest instead of a separate file.
- The spinner beside "Working for…" above a reply now starts at the left edge of the line, where the finished "Worked for…" text starts. Before, it sat indented.
- The button in the chat box that sets how hard the agent thinks is now named Effort when you point at it, instead of Thinking.

## [26.1.1] - 2026-10-29

### Added

- The chat box has a new size next to Thinking that sets when the chat's context is summarized, from 128k up to 2m or only when full, shows how full it is, and can compact it now; each agent's default is in Settings → AI Tokenomics. API providers, Codex, OpenCode and Antigravity now compact on their own as well, and a local model, such as one in LM Studio, compacts before its loaded context fills up instead of losing the middle of the chat. While a chat compacts, its divider reads Compacting context and then Context compacted once it is done.

### Changed

- Patchwork is now free software under the GNU Affero General Public License, version 3 or later. Settings → Updates shows the license and links to it, and a commercial license is available for anyone the AGPL does not suit.
- Settings → Hooks now shows each entry on its own row that says in plain words when it runs and what it does, such as "Before shell commands" and "Refuses waits with no time limit", instead of raw event names and patterns. Click a row to see its full command and timeout, and a row now says which agents skip it, such as OpenCode.

### Fixed

- A reply no longer bounces up and down while it streams in. New lines now appear where they belong and the chat moves only as much as the words that arrive, and if you scroll up during a reply it stays where you put it.
- When an agent leaves a command running after its reply, such as a dev server, the chat box now shows Stop to end it. Before, only Send was there, so the chat looked stuck until the command ended on its own.

## [26.1.0] - 2026-10-29

### Changed

- Behind-the-scenes improvements, with nothing new to see.

## [0.11.3] - 2026-10-29

### Fixed

- Permission rules now apply as you change them. A rule you add, switch off, or edit decides an agent's next call straight away in every access mode, and a request already waiting is answered by the rule you just saved. Until now a running agent kept the rules it started with, and in full access a refusal could be missed altogether.
- A message you send while an agent is still working now leaves one working reply, not two: the reply your message interrupted is marked finished as the agent turns to what you said.

## [0.11.0] - 2026-10-28

### Changed

- When a lot of a reply arrives at once it now appears in one go instead of fading in word by word, so a fast answer no longer trails behind. A screen reader reads the reply as whole text, not one fading word at a time.
- A reply's status line now stays at the top of it, saying what is happening while it works and what it took once it is done, and it says how many steps were folded away. Icons beside running work stay still, details grow open instead of appearing, and a finished thought says how long it took instead of showing a line of its own text.

### Fixed

- Claude now finds your skills on Windows too, where they used to be missing. Every agent is also given exactly the skills you have switched on, so a skill you turn off stops being offered straight away.

## [0.10.40] - 2026-10-28

### Added

- Settings → Providers now has a Download button for Claude Code, Codex and the OpenCode CLI, like the one the Antigravity CLI had. Each runs the agent's official installer and puts it where that installer normally does, and the Antigravity CLI now installs there too instead of in a folder of Patchwork's own. On Windows, only the Antigravity CLI can be downloaded this way for now.

### Changed

- The folders Patchwork builds from your agents and your enabled skills, in its config folder, now have names that say what they hold and sit next to the folders they come from.
- A reply now appears a word at a time at a steady pace while it streams, with each new word fading in, instead of jumping forward in chunks, and each new step in a reply settles into place rather than popping in. Turning on reduced motion in your system settings switches this off.

### Fixed

- A background task Claude started, such as a command it left running, no longer stays listed as running in the chat after it has finished when Claude's report of its end goes missing.

## [0.10.39] - 2026-10-28

### Fixed

- On Windows, chats and agents no longer open a stream of command prompt windows, Open in editor finds VS Code, VS Code Insiders, and Cursor, and the model picker lists every model instead of only Default. Settings → Providers notices an agent CLI installed or removed while Patchwork is open, Settings → Environment offers Git Bash, PowerShell, and cmd for the commands agents run, and Stop ends an agent along with everything it started. Permission rules now apply to Codex and Antigravity on Windows, and the Antigravity CLI can be downloaded from Settings → Providers there too.

## [0.10.38] - 2026-10-28

### Added

- Codex chats can now be compacted from the chat box, as Claude and OpenCode chats already could, to free up context in a long conversation.
- Every finished reply has a Fork button that opens a new chat with the conversation up to that reply, so you can try a different direction without losing the original chat.
- Settings → Claude CLI now lists the feature flags and models the installed Claude Code is using, like the Codex CLI page, and the thinking picker in the chat box now offers only the levels the model you picked accepts.
- A project's right-click menu in the sidebar has "Open folder", which shows the project's folder in the file manager.
- Background work the agent starts and leaves running, such as a command, a subagent, or a workflow, now appears above the chat box with how long it has run, until it finishes. You can stop it there when the agent allows it; Codex commands show without a stop button. Claude waits for its background work to report back, so it can carry on with the result.
- Formulas in chat replies now render. Wrap one in double dollar signs, inline or on a line of its own, and copying it gives back the formula as typed. Single dollar signs stay plain text, so prices and shell variables are not read as formulas. Code in chat and the Canvas is coloured more accurately, and added and removed lines in a diff show in green and red.

### Changed

- Stop now interrupts a Claude or Codex reply instead of shutting the agent down, so Claude answers your next message straight away in the same session instead of starting up again.
- Settings → Agents now has one Providers page instead of separate CLI Providers and API Providers pages. Installed agents and your API providers are listed together, and an API provider's form opens on the same page.
- Settings → Scheduled shows each task as a card with its schedule, its project or scratch chat, its agent, its recent runs, and when it runs next. Paused tasks are marked Paused. New task and Refresh sit in one row at the top, and the task form labels every field.

### Fixed

- A chat or task that starts with an attached file or a file mention is now titled from the message after it, not from the file's name. A message that is only files gets no title.
- On Wayland, a file, folder, link, or editor that Patchwork opens now comes to the front instead of staying behind your other windows. Showing a file in the file manager does too.
- Fetch models in Settings → Providers now explains why a provider returned no models, instead of showing an empty list. Local servers such as LM Studio need /v1 at the end of the Base URL, and the message says so.

## [0.10.37] - 2026-10-28

### Added

- When an agent makes an image, such as a logo or a screenshot, its reply now shows a thumbnail you can click to open in the side panel. Other files it makes, such as a PDF, show as a card with their type and size and a button that opens the file in its usual app. Text files still appear only in the reply's list of changed files.
- A plugin in Settings → Hooks can now run commands in your project when you send a message and when a reply finishes or is stopped. Each plugin can be switched on or off there, and a hook that fails never stops the reply.
- With computer use on in Settings → Permissions, the panel under the chat has a Screen tab that shows the screen as the agent sees it. Capture takes a new picture, and Live keeps taking new ones. It only shows the screen: every click and keystroke still goes through the agent with your approval.
- Each task in Settings → Scheduled now shows its last few runs as dots under its schedule, with the newest one spelled out. Hover over them to see when each run happened, failed, or was skipped, and why.
- Mermaid diagrams in a reply are now drawn as diagrams instead of shown as code, whichever agent wrote the reply. HTML and SVG artifacts are shown running too, in a frame that cannot reach the rest of Patchwork. A button on each one switches between it and its source, and another copies the source.
- Settings has a new Scheduled page where you give a prompt a name and a time, such as every few hours, every day, on weekdays or on the days you pick, and Patchwork starts it on its own in the project you choose or as a chat. You can switch a task off, start it now with Run, edit or delete it, and see when each task last ran and when it runs next. Tasks only run while Patchwork is open.
- Settings has a new Plugins page that lists each plugin bundle with the skills and tools it adds. Patchwork comes with one bundle that has two starter skills, one for reviewing a change and one for writing a scheduled report. Bundles made for Codex work as they are.
- Settings has a new Codex CLI page that shows the version of Codex you have installed, its feature switches and its models. From there you can also run Codex's own health check and list its servers and add-ons.
- Settings → Permissions has a new Computer use switch. When it is on, Patchwork's own agent can take screenshots and move the pointer, click, type, press keys and scroll to work in other apps, and screenshots are made smaller first so they use less of the agent's context. It is off by default, is never offered while planning, and asks before each use unless one of your rules allows it.
- There is a new Vision model setting beside the model choices in Settings. Messages with pictures go to the model you pick there, so a model that only reads text no longer fails when you attach an image.

### Changed

- Computer use screenshots are now taken at the screen's full size instead of being shrunk first, so the agent sees the screen as it is and clicks where it sees things. Settings → Permissions no longer mentions the old smaller size.
- Settings → Plugins is now called Settings → Hooks.
- A scheduled run missed while Patchwork was closed still starts when Patchwork opens, but only if it was due within the last day. An older one is now skipped and shows up in the task's history.
- A scheduled task for a project now waits until a reply already running in that project has finished, instead of starting at the same time.
- Claude and Codex are now asked to keep a checklist as they work through a task with several steps, so the Tasks badge above a reply fills in more often. OpenCode's own checklist now shows in the Tasks badge too.
- When you type a slash after other text in the chat box, the slash command menu now lists Patchwork's own commands, such as model and plan, not only the agent's. Picking one of Patchwork's commands there clears everything you had typed, because those commands only work as a message on their own.
- If a reply was cut off when Patchwork closed, it now ends with an error. Before, Patchwork sent the agent a message on the next launch telling it to carry on.

### Fixed

- When you copy a reply that contains a diagram or an HTML artifact, the copied text now includes each one's source. Before, they were left out of what you pasted.

### Removed

- Plugins no longer add skills or MCP servers. Add those in Settings → Skills and Settings → MCP Servers instead.
- Patchwork no longer comes with the Patchwork Review plugin. If you already had it, its review-diff and scheduled-report skills are gone.
- Chats and tasks are no longer compacted on their own while they sit idle. This used to add a compact message you never sent. The Compact idle tasks and Compact above settings are gone.

## [0.10.32] - 2026-10-28

### Added

- Claude Code, Codex and OpenCode can now start your custom agents from Settings → Agents as subagents by name, each with its own instructions, so an agent like a docs lead can hand work to your other agents. A subagent uses the model its agent names when the chat's Claude Code, Codex or OpenCode has that model, and the chat's model otherwise.
- Settings → Agents now has All and Primary filters at the top, so you can show only your primary agents.

### Changed

- Settings → MCP Servers, Agents and Skills now show one row per item instead of a grid of cards, so long lists stay easy to scan. Each row shows its badge, name and a one-line description next to its actions. A skill's file location and an agent's own name now show when you hover over its name. Add server, New agent and Open folder moved to the top, next to search.
- Agent Management in Settings is now called Agents, and it sits right under MCP Servers.

### Fixed

- Subagents started by Codex now show in the chat as Agent steps, each with a thread of its own, the way Claude's do. Before, a Codex subagent didn't show at all, its reply could end up in the main reply, and its finishing could end the whole turn early.

## [0.10.31] - 2026-10-28

### Fixed

- Choosing a section in Settings no longer makes its highlight shrink. The side menu now always keeps room for its scrollbar, so its rows keep the same width from the start instead of narrowing after you pick one.

## [0.10.30] - 2026-10-28

### Added

- You can now write your own agents and summon one by typing & and its name in the chat box, such as &reviewer, so that message follows the agent's instructions. You can summon several agents in one message, and a project can keep its own agents, which take the place of any with the same name.
- An agent can name a model, and summoning it switches the chat to that model if the chat's coding agent offers it. An agent marked primary stays the chat's agent for every message until you remove it with the × next to its name in the chat box.
- The new Settings → Agent Management lists your agents with each one's title, description and model, and tags the primary ones. The list stays up to date as you add, edit or remove agents. It opens an agent for editing, can create a new one for you to fill in, and shows where agents are kept, both for all chats and inside a project.
- Settings → Shortcuts has a new Sigils tab where you can change the symbol that opens each menu in the chat box: @ for files, $ for skills, & for agents and / for commands. Pick another symbol, such as # or !, and the menus and Settings show it, while your messages still reach the agent with the usual symbols.

### Fixed

- Settings no longer stutters while an agent is replying in the background. Before, every word that streamed in made the open settings page redraw, so hovering and clicking could lag.
- Every spinner now turns in step with the others, also when it shows again after being hidden, such as a working chat's spinner in the sidebar when you hover its row or expand its folder. Before, a spinner that showed again after being hidden, or that was added while hidden and shown later, could turn out of step with the others.

## [0.10.29] - 2026-10-27

### Changed

- In Settings → Tokens → Claude compacts at, the first option is now called Default instead of Claude's own. It works the same way as before and leaves compacting to Claude.

### Fixed

- Selecting a whole code block in a reply, for example with a triple-click, and copying it now pastes just the code. Before, the code could come out wrapped in code fences.

## [0.10.28] - 2026-10-27

### Added

- Settings → Chats and threads is a new page under General. "Click answers twice to send" is there now. It used to sit at the bottom of CLI Providers.

### Changed

- Settings → Updates now lists only the three newest releases instead of every release.

### Fixed

- When you select code in a reply and copy it, only the code is pasted now, just like with the Copy button. Before, the pasted code could come wrapped in code fences.

## [0.10.27] - 2026-10-27

### Added

- Settings → Agents → Click answers twice to send brings back the old way of answering an agent's question, where the first click only picks an answer.

### Changed

- Clicking an answer to an agent's question now sends it at once. Before, it took two clicks.
- Ask rules in Settings → Permissions now make the agent wait for your answer in every access mode except Full Access, even for calls the mode would otherwise run on its own. Before, they only took effect when the agent stopped to ask you first.
- Agents can read their skills folders without asking, so your skills keep working when reading other folders has to be asked about or is blocked.
- An agent's questions and permission requests now show up in its reply at the point where they were asked, instead of collecting at the bottom. Answered questions stay in view when a finished reply folds up.
- Claude now compacts a working task once its context reaches 200k, instead of waiting until the model's window is almost full. Settings → Tokens → Claude compacts at → Claude's own brings back the old behavior.

### Security

- Deny rules in Settings → Permissions, and a deny default you set for a kind of request, now block matching tool calls in every access mode, Full Access included. Before, in Full Access or any mode that ran a call on its own, a command, edit or read you had blocked could still run. Now the agent is told the call isn't allowed, and its reply carries on without it.

## [0.10.26] - 2026-10-27

### Changed

- An image you paste into the chat box now keeps the name it was copied with, such as a screenshot's file name, the same as when you drag it in. Before, every pasted image was called "Pasted image". On Wayland, this also covers images from apps like KDE's screenshot tool that suggest a file name when you copy.

## [0.10.25] - 2026-10-27

### Added

- You can now paste files and folders you copied in your file manager into the chat box. They go in at the cursor, the same as when you drag them in.

### Fixed

- Pasting a screenshot or another copied image into the chat box now attaches it on macOS, Windows and Linux. Before, this only worked on Linux under Wayland.

## [0.10.24] - 2026-10-27

### Fixed

- On Linux in a Wayland session, pasting a screenshot or other copied image into the chat box now attaches it, the same as dragging it in. Before, the paste did nothing.

## [0.10.23] - 2026-10-27

### Fixed

- Sending two steers right after each other no longer leaves an empty extra reply stuck on Thinking under the second one. When the agent picks up both steers at once, the chat now shows one reply that answers them both.

## [0.10.22] - 2026-10-27

### Changed

- Settings → Updates now lists every release instead of only the newest one. Each release has a row showing its version and date. The newest release starts open, and you can click any row to show or hide its notes.

## [0.10.21] - 2026-10-27

### Changed

- Behind-the-scenes improvements, with nothing new to see.

## [0.10.20] - 2026-10-27

### Added

- Ctrl+1 opens the sidebar on Tasks, Ctrl+2 on Changes and Ctrl+3 on Archived (Cmd on a Mac). They are listed in Settings → Shortcuts on the Sidebar page, where you can change them, and the icons on the left show their key when you hover them.
- Settings → Permissions has a Restore defaults button on the Everywhere rules. It puts back the starting rules you deleted, such as the one that lets ls run without asking, and resets the ones you changed, such as rm switched to Allow. Rules you added yourself stay as they are. The button is greyed out when there is nothing to restore.
- Restore defaults leaves out a starting rule when a rule you added yourself already covers the same command, so your own rule stays in charge.

### Changed

- Copying part of a reply now copies it as Markdown, so headings, lists, quotes, tables, code blocks, bold text, inline code and links keep their formatting when you paste them into the chat box or another app. Pasting into another app, such as an editor or a terminal, brings the Markdown marks along: the hash signs before headings, the dashes before list items, the asterisks around bold text, and links written out with their address. In the chat box, lists, tables and code come in as blocks, and headings and quotes as the lines they were written as. Before, a copy kept only the plain text.
- Copying from inside a code block in a reply still copies the code exactly, without any marks.
- The list on the left of Settings is now in four groups with small headings: General, Agents, Projects and System. Before, all fifteen pages were in one long list. The list scrolls when the window is too short for it.
- The small Copy button, on code blocks and in other places that use it, now says "Couldn't copy" when a copy fails, instead of doing nothing.

### Fixed

- On Linux with Wayland, the Copy button on code blocks now copies when another app was in front. Before, the click only brought Patchwork forward and the copy was refused without a word. The same fix covers copying a plan, copying in the file panel, and copying in the terminal.

## [0.10.19] - 2026-10-27

### Added

- The Allow button on an approval now has a menu: Allow once, Always allow in this project (or in Chats, for chats without a project), or Always allow everywhere. It shows what it will save, such as the folder or the command, and the button offers the choice you picked last time for that kind of request. Once a folder, command, website or MCP server is always allowed, Patchwork answers later requests like it for you, including ones already waiting, so you no longer click Allow again and again for the same folder.
- Settings → Permissions lists the rules for everywhere, for chats, and for each project. You can switch each rule between Allow, Ask and Deny, edit it, move it to another project or to everywhere, delete it, or add your own. You can also set what each kind of request gets when no rule matches: editing files, running commands, working outside the folder, using the web, and MCP tools.
- Requests a rule answered show as a single "Auto-allowed" line in the chat instead of another card. Click "Auto-allowed" or "Always allowed" to open the rule in Settings → Permissions or to have it ask again next time. Click "Allowed" on an approval you answered by hand to save a rule for it afterward.

### Changed

- Commands that only read, such as ls, cat, grep and git status, now run without asking from the start. Commands that delete, run with more access, or rewrite history, such as rm, sudo, git push and git reset, keep asking even when another rule allows them. These are starting rules in Settings → Permissions that you can change or delete.
- Commands that send their output to a file or run another command inside them still ask each time, even when a rule allows the command.
- Repeated approvals of the same thing now fold into one card in the chat, such as "Denied ×3".

## [0.10.18] - 2026-10-27

### Fixed

- Spinners that show at the same time now turn together, such as when two background tasks are running in a chat. Before, each spinner started its turn when it appeared, so they were out of step with each other.

## [0.10.17] - 2026-10-27

### Added

- Files you mention with @ in a message you've sent now show in the chat as the same highlighted chip you see in the chat box, and clicking one opens your file manager with that file selected. This works for files inside and outside the project. Before, the mention was plain text you couldn't click.

### Changed

- In Settings, Skills now sits right below MCP Servers in the sidebar. Before, it was further down, between Terminal and Projects.

## [0.10.16] - 2026-10-27

### Added

- When a newer version of Patchwork is available, Settings → Updates now shows what is new in it, above the notes for the version you have installed. You can read what changes before you restart to update.

## [0.10.15] - 2026-10-27

### Added

- Settings → Skills now shows each skill as a card with its name, file, description, an Edit button that opens it in your editor, and an on/off switch. Filter by On or Off, or search by name and description. Skills you add, edit, or delete in the skills folder show up right away, without restarting Patchwork.

## [0.10.14] - 2026-10-27

### Changed

- Every loading indicator now uses the steady spinner from the thread list: plan steps, background tasks, the tasks badge, steps in a reply, plans loading, and the setup screens for agents and imports. The old one rotated and looked like it shook.

## [0.10.12] - 2026-10-27

### Added

- Settings → Updates now shows what changed in the version you have installed.

## [0.10.11] - 2026-10-27

### Added

- Right-click a chat or task and choose Export to… to save its whole conversation as a Markdown file wherever you like.

### Changed

- The spinner beside a working chat in the sidebar and in the status bar no longer looks like it shakes. The ring stays still and only an arc runs around it.
- Chat titles in the sidebar now line up with the Chats label above them, and longer titles show more before they are cut off.
- Empty sidebar sections, such as No chats yet or Nothing archived, take less room, and the Archived section has the same header style as Changes.
- The sidebar's section labels sit closer to the lists under them, so more of each list fits.

### Other

- When several rows in the sidebar are highlighted one after another, a small gap now keeps them apart, so each row is easy to tell from the next.

## [0.10.10] - 2026-10-27

### Changed

- Update from updates.patchworkeditor.com and check each update's signature

## [0.10.9] - 2026-10-27

### Added

- Draw the window with the GPU again, cutting CPU use during animations
- Add Criterion benchmarks for parsing and saving streamed events

### Changed

- Update chats in the background once a second while they stream

### Fixed

- Stop the composer and bars re-rendering while a reply streams

### Other

- Batch terminal output so noisy commands stop flooding the window
- Skip the notification settings lookup for each streamed token
- Turn off the webview's Reload menu on right-click
- Save tool events without a disk sync each time

## [0.10.8] - 2026-10-27

### Added

- Add a terminal tab list beside the terminal

## [0.10.7] - 2026-10-27

### Added

- Let a terminal tab be killed from its right-click menu

### Fixed

- Fix desktop notifications showing no Patchwork icon

## [0.10.6] - 2026-10-27

### Fixed

- Stop the open chat's highlight border being cut off on the right

## [0.10.5] - 2026-10-27

### Added

- Let picked files go in line in the message box

## [0.10.4] - 2026-10-27

### Added

- Show images and dropped files above the sent message again
- Let API agents look at images with read_file

## [0.10.3] - 2026-10-27

### Added

- Let images go anywhere in a message
- Let DeepSeek read attached images

## [0.10.2] - 2026-10-27

### Fixed

- Stop skill and goal chips making the message box taller

## [0.10.1] - 2026-10-27

### Added

- Let skills, commands and dropped files go anywhere in a message

## [0.10.0] - 2026-10-26

### Added

- Add MCP servers for API and local models, and show the ones Claude Code and Codex use

## [0.9.9] - 2026-10-26

### Fixed

- Stop the terminal spiking CPU and ghosting while the window resizes

## [0.9.8] - 2026-10-26

### Added

- Add generated chat and task titles, with a setting to turn them off

### Changed

- Make the sidebar tree more compact and readable

### Fixed

- Fix queued messages waiting for Send after a turn finishes

## [0.9.7] - 2026-10-26

### Fixed

- Stop the terminal from lagging and redrawing while the window resizes

## [0.9.6] - 2026-10-26

### Other

- Write the window log to a window folder

## [0.9.5] - 2026-10-26

### Added

- Add a terminal and Output panel, and keep debug logs in a log folder per session

## [0.9.4] - 2026-10-26

### Added

- Let OpenCode's questions be answered again
- Add a Logging section with an agent activity log

## [0.9.3] - 2026-10-26

### Added

- Add a Default choice to Compact above that follows Claude's compact point

### Other

- Give every default model dropdown the same width

## [0.9.2] - 2026-10-26

### Changed

- Use DeepSeek's documented base URL without /v1

### Other

- Open the sidebar width menu from anywhere on the sidebar

## [0.9.1] - 2026-10-26

### Other

- Keep Claude running between messages so its context stops filling up

## [0.9.0] - 2026-10-26

### Added

- Show what each reply's turn changed in its files card, after a commit too
- Let the sidebar be resized from its right edge, remembered and locked

### Changed

- Align the Changes sidebar's file rows, counts and icons

## [0.8.9] - 2026-10-26

### Added

- Add skills from a Patchwork skills folder that every agent loads

### Fixed

- Fix the editor opening behind other windows from Open in editor on KDE

## [0.8.8] - 2026-10-26

### Other

- Match the API provider model checkboxes to the app's own checkbox style

## [0.8.7] - 2026-10-26

### Fixed

- Stop a turn when the agent repeats the same tool call 8 times in a row

## [0.8.6] - 2026-10-26

### Added

- Allow plain http:// base URLs for API providers on any host

## [0.8.5] - 2026-10-26

### Added

- Show OpenCode tool calls with their real start time
- Show each file a reply changed, open it from the chat, and mark deleted files

### Changed

- Improve file watching and git refreshes, and credit a reply only with files its own tool calls changed

### Fixed

- Fix reply file paths that kept a leading ./ or a project root with a trailing slash

## [0.8.2] - 2026-10-25

### Other

- Keep the reply's changed files and the Changes view current while the agent works

## [0.8.1] - 2026-10-25

### Added

- Let users set how often Patchwork checks for updates

## [0.8.0] - 2026-10-25

### Changed

- Tighten the version badge corners

### Other

- Round the version badge into a rectangle instead of a pill
- Attach multiple files and folders from the composer

## [0.7.1] - 2026-10-25

### Removed

- Remove default models from the DeepSeek preset

## [0.7.0] - 2026-10-25

### Added

- Add API providers for DeepSeek and OpenAI-compatible endpoints

## [0.6.10] - 2026-10-25

### Other

- Keep an updated AppImage's old library paths out of agents
- Import OpenCode 2.x conversations and log import scans

## [0.6.9] - 2026-10-25

### Fixed

- Fix release builds failing to sign the update files

## [0.6.8] - 2026-10-25

### Changed

- Update Patchwork from GitHub releases without a signing key

## [0.6.7] - 2026-10-25

### Other

- Ease the update card into place

## [0.6.6] - 2026-10-25

### Added

- Show a card in the top right when an update is ready

### Other

- Run agents, git, and links with the user's own environment
- Stream Claude's reasoning and show when it retries a request

## [0.6.4] - 2026-10-25

### Changed

- Update Patchwork on its own from GitHub releases

## [0.6.3] - 2026-10-24

### Other

- Offer and run OpenCode's slash commands from the composer

## [0.6.2] - 2026-10-24

### Other

- Keep the token totals line inside the composer's edge

## [0.6.1] - 2026-10-24

### Added

- Show the thread's context and token totals once, above the composer

## [0.6.0] - 2026-10-24

### Added

- Add a choice of app icon color ways

## [0.5.2] - 2026-10-24

### Added

- Show the archive as a project tree that keeps each task's folder and place

## [0.5.1] - 2026-10-24

### Other

- Ship the app icon as a single logo.png plus the Windows icon.ico

## [0.5.0] - 2026-10-24

### Changed

- Replace the app logo with the patchwork cube

## [0.4.13] - 2026-10-24

### Changed

- Make pasted sentences go in as paragraphs instead of a code block

### Other

- Send an agent question's answer by clicking the highlighted option again

## [0.4.12] - 2026-10-24

### Fixed

- Fix pasted text getting the wrong syntax highlighting language

## [0.4.11] - 2026-10-24

### Other

- Send an agent question's answer with a double-click

## [0.4.10] - 2026-10-24

### Fixed

- Fix the code block language picker being cut off in the composer

## [0.4.9] - 2026-10-24

### Added

- Show why a reply was blocked by a safety classifier
- Let a project's folder be changed when it was moved or deleted
- Show a finished Claude goal as complete, and clear the banner on Clear goal
- Show the app version in the toolbar, and keep package versions in step with releases

### Fixed

- Stop agents being killed for memory the desktop was low on

### Other

- Clear goals quietly, without a working row or chat bubble
- Underline reply links and open them in the system browser
- Flash a project when its folder is added again, and tell same-named projects apart
- Paste terminal output as one code block, and fix composer select-all and bubble spacing
- Detect the language of pasted code, and attach long pastes as files

### Removed

- Drop a finished goal's banner when the next message is sent

## [0.4.2] - 2026-10-23

### Added

- Allow connecting both agents; fix icon button alignment
- Add chats without a project
- Show threads as a tree: standalone chats plus per-project tasks
- Add Claude and Codex conversation import
- Add Prettier with format scripts for apps tsx files; add commit rule
- Add per-thread access modes with approval prompts; redesigned thread cards
- Add Tailwind v4 alongside the existing CSS
- Add -webkit-user-select so the UI stops being selectable on WebKitGTK
- Show one highlight in the model picker
- Add build:appimage script to the root package.json
- Show changed files in the sidebar's Changes view, not the chat
- Show subagent lineage in the chat's top-right corner
- List every file of a reply in Show changes, not only uncommitted ones
- Show the lineage card while the reply is working
- Let the browser keep the scroll position when rows above change height
- Show the agent as running only while its subagents run, and nest subagents under their spawner
- Render more chat rows ahead of the viewport
- Show Claude usage from its own rate-limit reports, not only the endpoint
- Add folders inside projects; drag tasks into them
- Animate sidebar open/close; draw drop lines between rows
- Reopen the last thread; ghost-free sidebar tweaks; chunk splitting
- Remember which projects and sub-folders are collapsed
- Render replies as Markdown and compose in Tiptap; add choice panel
- List Antigravity models from the manifest
- Show agent reasoning, live actions, and how long replies ran
- Track Claude token usage per reply and warn on large cold sessions
- Show compaction in the chat and keep drop highlight over folders
- Show composer quote lines as a bar with the marker hidden
- Add UI checks that measure and screenshot the running dev session
- Animate archiving and mark running goals in the sidebar
- Let Ctrl+W step through chats the same way as tasks
- Add build and release scripts and a fixed-name AppImage
- Show the Patchwork icon in the local build's title bar
- List the Space side panel shortcut under Sidebar
- Show replies as an ordered timeline and let agents ask questions
- Show the OpenCode logo in the import picker
- Add a magic wand to sort a project's tasks into folders by provider, with undo
- Draw only the visible part of large diffs and files
- Add a heavy-load profiling check
- Add legacy models to the model picker and show the prompt mode in the status bar
- Draw the dev session frame without slowing animations
- Add restorable file checkpoints and continue turns cut off by a restart
- Add a Clear goal button and mark the chosen send mode
- Let a chat go idle while commands the agent left running keep going
- Add OpenCode as a fourth agent
- Add a project by dropping its folder on the sidebar

### Changed

- Rename AgentIcon to ProviderLogos and wire branded SVG logos
- Rename the AppImage script to appimage
- Align the composer with the message column by the scrollbar width
- Make the composer span the full width of the chat
- Describe access modes without comparing them to another app
- Sidebar: sub-folders, folder task button, and spacing
- Sidebar: tighten the gap between a project and its first sub-folder
- Sidebar: drop a task above the first task of a sub-folder
- Sidebar: search bar, usage separator, and shortcut legibility
- Polish the sidebar change list and status bar
- Sidebar: align sub-folders with projects; tighten thread cards
- Choice panel: highlight with number or arrow keys, send with Enter
- Improve composer markdown, command palette, and reload
- Tighten spacing above sub-folder tasks
- Move the project dropdown into the header and drop the branch label
- Use logo.png as the app icon
- Rename the Gemini agent to Antigravity and drop the Gemini CLI fallback
- Move the sidebar time and spinner next to the archive button
- Align the sidebar project chevron with the Projects heading
- Improve selecting and reordering in the Projects tree
- Use the login shell as the default shell for agents
- Improve chat queue and goal workflows
- Make the settings dialog bigger

### Fixed

- Stop the chat following streamed output to the bottom
- Fix /goal across agents; add Antigravity CLI, thinking levels, and live tool/subagent rows
- Fix chat list rows, sidebar overflow, and composer drop line
- Fix sidebar row squashing and add hover and spacing to tasks
- Stop Antigravity usage from stalling other refreshes
- Stop Ctrl W from removing the open chat and update the shortcut wording
- Stop any thread's turn and leave its goal paused
- Fix the Windows and macOS release builds

### Other

- Scaffold Patchwork: Tauri 2 + React shell, shared contracts, theme tokens
- Build Patchwork ADE: agent runner, git workspace, onboarding, IDE workspace, canvas
- Keep agent order fixed when changing the default agent
- Pick models per agent from one grouped picker
- Fixed settings installed status
- Reorganize app into apps/desktop pnpm workspace
- Split into web/server/desktop apps; add Gemini, imports, usage, SQLite
- Tidy sidebar: search moves into it, larger icons and text
- Center new-thread hero with composer, live Claude plan, roomier sidebar cards
- Truly center the new-thread layout; even composer padding
- Import chats without a project as chats, not projects
- Right-click to rename or remove projects, tasks and chats
- Shorten project remove confirmation
- Import conversations whose folder is gone as chats
- Hide sub-agent conversations from imports; show a count instead
- Don't count Patchwork's own chats as importable conversations
- Ask before removing a project with the native dialog; cache the live Claude plan
- Confirm before removing a chat or a task
- Brighten sidebar section labels and empty-state notes
- Put element base rules in the Tailwind base layer
- Convert component CSS to Tailwind utilities
- Fetch the model manifest from the private repo instead of bundling it
- Read Codex models from the model manifest too
- Read the manifest token from a git-ignored .env
- Start the model picker on the current model and make results padding symmetric
- Attach files, folders, and images by dragging them into the chat
- Pin a reply's files when opening Show changes; center the chat column
- Nest subagent actions under their agent, add image thumbnails and bulk apply/reject, and test the web app
- Keep a reply's committed files in the Changes view's pinned group
- Stream Claude replies token by token instead of in whole blocks
- Virtualize the chat list with TanStack Virtual, following the example
- Position virtualized chat rows with their measured offset
- Revert the virtualized chat list; the chat pane rendered blank
- Virtualize the chat list following the TanStack virtual chat example
- Stream agent text to the UI without a database write per token
- Stabilize the virtualized chat list while scrolling and streaming
- Hide the lineage card once no subagent is running
- Keep virtualizer callbacks stable and place rows with top, not transform
- Share one empty changes list so streamed replies don't re-render rows
- Revert "Let the browser keep the scroll position when rows above change height"
- Read Codex rate limits from the end of each rollout
- Stream Codex rollouts when scanning imports
- Steer a running turn when a message is sent during it
- Answer steering messages below them, queue what can't steer, add /goal
- Rebuild the lineage card, with per-subagent stop
- Recall earlier messages in the composer with Up and Down
- Keep steering open while Claude waits on a background subagent
- Drag to sort sidebar rows; left-align chat; fix hover size in Changes
- Nudge the header project dropdown 2px left
- Give composer quote lines more space before their text
- Format markdown as it is typed and add a composer format bar
- Reload the canvas when project files change on disk
- Restyle the sidebar's Collapse all and focus the composer on Tab
- Name the local AppImage without the version
- Install the local AppImage to a stable path with a launcher
- Pick the shell agents run commands in from a new Environment settings section
- Label shell commands with the chosen shell and tell Claude which shells exist
- Queue messages behind a running turn and add a steer or queue toggle
- Archive chats instead of deleting them and add Ctrl N for new chats or tasks
- Tidy sidebar rows, hide the composer formatting bar when narrow, and fit chat badges
- Paste images from the clipboard into the chat
- Lighten the top-level project folder icons and dim the sub-folder icons
- Highlight a slash goal only at the start of the draft
- Refocus the composer when a goal or turn ends
- Focus the composer when a chat opens
- Hide the composer caret when the composer is not focused
- Select and delete rows with Ctrl A and Ctrl click, and add a shortcuts page
- Archive the open chat with Delete and unfocus the composer on Esc
- Keep the chat jump button hidden at the bottom and stop sidebar shifts
- Archive the open chat with Ctrl W alongside picked chats
- Archive the open chat with Ctrl W only while the compose box is not active
- Pad and indent the sidebar project rows and folders
- Give the selected Tasks tree row more top and bottom padding
- Select the next task when archiving and make the tree selection visible
- Lock the composer while the choice picker is open
- Close to the system tray and notify when goals and tasks finish
- Formatting
- Removed app image build from scripts
- Title threads started with /goal by their objective
- Widen the sidebar and startup window and tighten task highlights
- Keep runaway agents from taking the app down with them
- Toggle the side panel with Space outside text fields
- Cap agent memory and stop stuck goals and replies
- Keep subagents alive through a steer and add subagent and plan cards
- Restore the 16px gap between the composer and the status bar
- Paste multi-line text into the composer as a code block
- Match the Plan chip icon size to the other composer chips
- Keep the chat at the bottom when a reply starts after a send
- Give agents the login shell's PATH
- Cut CPU use while a reply streams
- Open chats at the bottom without bouncing
- Name the installed launcher Patchwork
- Keep dismissed goals and queued messages from returning
- Smooth composer strips and fix formatted message recall
- Format the tasks badge so the release format check passes
- Steer the queued message with Ctrl+Enter while its strip is open
- Smooth the running spinner and goal borders and add a frame-rate check
- Import OpenCode conversations as threads
- Position context usage below turn duration
- Constrain slash command menu and improve navigation
- Start a new Codex thread when the saved one never got a first turn
- Load each task's full history only when it opens
- Re-parse only the growing end of a streaming reply
- Keep focus in the task list when a task opens from it
- Collapse very long pasted messages
- Open settings instantly, even right after startup
- Walk multi-line composer drafts line by line with Up and Down
- Run shortcuts as commands with context rules and remap them in Settings
- Bring back the paged shortcuts pane with descriptions and click targets on the keys
- Label the prompt mode in the status bar and even out its spacing
- Split persistence into modules and keep migrations as numbered SQL files
- Frame the dev session in yellow and black hazard stripes
- Smooth opening and closing large sidebar folders
- Keep Esc and Ctrl+A out of the remappable shortcuts
- Set goals from messages with images and follow AGENTS.md in the Rust code
- Open chats at the bottom and show the /goal chip in sent messages
- Redesign the usage popup as provider tabs with ring gauges
- Rewrite the README and add an MIT license
- Open and scroll to a task in the sidebar when it is picked from search
- Step through search results in the order they are shown

### Removed

- Remove duplicate settings button from the toolbar
- Drop toolbar thread title; clearer clickable cards with flush-right logo
- Remove the tests and add a rule against writing them
- Drop git status from the Changes view; mark edited files with an icon
- Drop bulk Apply/Reject under full access; right-align change counts
- Remove the local AppImage install script
- Remove several picked chats, tasks, projects, and folders at once
- Drop the hover tooltip on project sub-folder rows
- Drop the path tooltip on project rows
- Remove the integration toggles that did nothing
