# Patchwork: Git rules

Run all Git commands from the repository root.

## Ground rules

- **Leave changes uncommitted until reviewed.** No commits, branches, worktrees, or pushes unless the user explicitly asks ("push", "push to git", "release").
- **Every change reaches `main` through a squash-merged PR.** The ruleset on `main` requires a PR and passing CI. Only the Release workflow pushes to `main` directly.
- **One PR per change.** Keep unrelated changes separate.
- **Branch every PR from fresh `origin/main`**, never from another PR's branch or from local `main`.
- **No destructive Git operations without explicit confirmation:** `reset --hard`, `checkout -- <file>`, `restore`, `stash`, `clean -f`, force-push, moving or deleting pushed tags, or rewriting pushed history.
- **Ask before deleting, moving, or renaming files.**
- **No AI attribution.** Never add `Co-Authored-By: Claude` or "Generated with" lines to commits or PRs.
- **Don't watch GitHub Actions runs** unless explicitly asked.

# Patchwork: Running things

Run nothing the user already has running.

- **Don't start the dev server.** The user already has one running (`pnpm dev`, Vite on :1430 with the
  server from source). Starting another takes the port, spawns a second Electron window, and can grab
  the app's single-instance lock so the user's own launch quits with no message. To check the UI, open
  or reload the window that is already up.
- **Don't launch Electron.** Unless the user asks, drive the window that exists — over its debugging
  port if it has one, `computer_screenshot` otherwise. A launch of your own is a second window over a
  second server and shows you the wrong app.
- **Clean up what you do start.** A server, a Vite process or an Electron window you started must be
  stopped before you finish, and a stale `~/.config/Patchwork Next/Singleton*` left by a killed window
  must be removed, or the user's next launch exits silently.


# Patchwork: UI

## Context menus

- **One menu for all.** Right-click menus, chip dropdowns and pickers (model, agent, permissions,
  `@` mentions) use the shared `Menu` in `crates/design/components/src/menu.rs`. Don't draw a
  custom card, a search box, tabs or a slider for a list of choices.
- **Normal width.** A menu is as wide as its rows need, not a wide panel.
- **Rows:** a muted icon, the label (cut short with "…"), then the bound keys small and muted on
  the right. A row that opens a submenu ends in a `›` chevron. A row that picks one value of
  several shows a check on the current one (`radio`).
- **No row subtext or tooltips.** A row is one line. Short muted text beside the label
  (`detail`, such as an agent or provider name) is allowed only to tell rows with the same label apart.
- **Groups.** Lines separate the groups. Most-used actions go first, then the rest. A
  destructive action goes last, in its own group.
- **Submenus over crowding.** Put a long or nested list (a project's tasks, an agent's models,
  older models, effort levels, Split, Swap with) in a submenu instead of a flat list.
- **Submenu placement.** It opens beside its row with a small gap and its first row level with
  that row, and flips left when there is no room on the right. The row that opened it stays
  highlighted while it is open.
- **Pointer.** The highlight follows the pointer and clears when the pointer leaves the menu.
  A pointer moving diagonally toward an open submenu doesn't switch rows on the way; a row it
  rests on for about 250ms takes over. Draw no trail or triangle for this.
- **Keys.** Up and Down move the highlight. Right or Enter opens a submenu. Left or Escape in a
  submenu closes just that submenu. Escape on the top menu closes it all. A long list takes a
  filter, so typing narrows it.
- **Closing.** Picking a row closes the whole menu. A click inside a submenu doesn't close its
  parent; a click outside closes both. The press that closes a menu from outside does not
  reopen it from its own trigger, and focus goes back to where it was before the menu opened.

# Patchwork: UI text

- **No subtext.** Put no description, hint, blurb or helper line under a title, heading,
  section or row. That covers Settings pages, sections and rows, and new UI anywhere. A title
  stands alone. Add subtext only when the user asks for it in that request.


# Patchwork: Tests

- **Never write tests.** Add no smoke, unit, integration, regression, snapshot, or end-to-end (E2E)
  tests, test helpers, or fixtures. The repo has none; verify changes with `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, and in the running app.
