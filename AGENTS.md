# Patchwork: Git rules

Run all Git commands from the repository root.

## Ground rules

- **Push finished work.** Once a change is verified, open its PR and turn on auto-merge without waiting to be asked. Leave it uncommitted only when the user says to hold it.
- **Every change reaches `main` through a squash-merged PR.** The ruleset on `main` requires a PR and passing CI. Only the Release workflow pushes to `main` directly.
- **One PR per change.** Keep unrelated changes separate.
- **Branch every PR from fresh `origin/main`**, never from another PR's branch or from local `main`.
- **No destructive Git operations without explicit confirmation:** `reset --hard`, `checkout -- <file>`, `restore`, `stash`, `clean -f`, force-push, moving or deleting pushed tags, or rewriting pushed history.
- **Ask before deleting, moving, or renaming files.**
- **No AI attribution.** Never add `Co-Authored-By: Claude` or "Generated with" lines to commits or PRs.
- **Don't watch GitHub Actions runs** unless explicitly asked.