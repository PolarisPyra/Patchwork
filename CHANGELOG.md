# Changelog

All notable changes to Patchwork are documented here, for the people who use it.
Each release adds its notes at the top; see docs/release.md.

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
