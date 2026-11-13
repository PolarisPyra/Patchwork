# Contributing

Patchwork takes bug fixes from outside the team. New features, redesigns, new providers and ports to
other platforms are decided inside the team, so a PR for one is closed without review. To suggest
one, start an [Ideas discussion](https://github.com/PolarisPyra/Patchwork/discussions/categories/ideas).

Report bugs as [issues](https://github.com/PolarisPyra/Patchwork/issues/new/choose), and security
problems [privately](https://github.com/PolarisPyra/Patchwork/security/advisories/new).

You can help without writing code: point out duplicate issues, say when a bug no longer happens on
the latest release, and try open PRs and comment with what you saw.

## Pull requests

These rules apply to everyone without write access to the repo. Following them doesn't guarantee a
merge.

- **Fix one bug.** A change to how something is meant to work is a feature, even if it's called a
  fix. Link the issue unless the bug is small and obvious. Cleanups and refactors go in their own
  PRs.
- **Show it works.** Say how you reproduced the bug and how you checked the fix. For UI changes,
  attach before and after screenshots to the PR, not to the repo.
- **Write the changelog line.** See [Changelog](#changelog).

### Changelog

The PR title becomes the commit subject, so write it short, in sentence case and the imperative, and
about what users see. The description needs a line that starts with `Changelog:` and a group (Added,
Changed, Fixed, Removed or Security), written as [the changelog style](docs/changelog-style.md)
describes; the release notes are built from it. A change users can't notice says `Changelog: none`.
A check on the PR enforces both.

### Open PRs

You can have 3 PRs open at once. A fourth is closed when you open it; reopen it once one of the
others is merged or closed.

### Conflicts

A PR that conflicts with `main` is asked to rebase and labeled `needs-rebase`. If it still conflicts
after 7 days with no activity, it is closed.

### Closing

A PR or issue that breaks these rules gets a comment saying which one, and the `closing` label. It
closes 3 days later unless you reply or push. After it closes, fix the problem and comment, or open a
new PR that links the old one.

An issue labeled `needs-info` closes 7 days later unless its author answers.

## Setup

[Development](docs/development.md) covers building and checking the code, and
[Architecture](docs/architecture.md) where things live. A change to how an agent CLI is started,
read or approved also updates [Agent CLIs](docs/agent-clis.md).
