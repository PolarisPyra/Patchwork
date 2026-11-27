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


# Patchwork: UI text

- **No subtext.** Put no description, hint, blurb or helper line under a title, heading,
  section or row. That covers Settings pages, sections and rows, and new UI anywhere. A title
  stands alone. Add subtext only when the user asks for it in that request.


# Patchwork: Tests

- **Never write tests.** Add no smoke, unit, integration, regression, snapshot, or end-to-end (E2E)
  tests, test helpers, or fixtures. The repo has none; verify changes with `scripts/check.sh` and
  in the running app.
