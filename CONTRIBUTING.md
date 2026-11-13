# Contributing

Patchwork is built and maintained by a small team with a clear direction. Review takes more time
than writing a change, so we accept bug fixes from outside the team, not new features.

Things we close without review:

- New features
- UI overhauls
- New agents or providers
- Architecture overhauls (extensions, plugins, new connection methods)
- Support for additional platforms

If you want one of these, open an
[Ideas discussion](https://github.com/PolarisPyra/Patchwork/discussions/categories/ideas).

## The best way to help

Help us find things we can close.

- **Duplicates.** Comment with a link to the original issue or PR.
- **Fixed bugs.** If an open issue no longer happens on the latest release, say so and give the version.
- **Dead PRs.** If a PR is superseded or abandoned, link what replaced it.

A +1 on an existing PR is better than another PR. Search the open PRs before you start. If one covers
your bug, react to it, test it, and comment with what you saw.

Report bugs in issues. Report security problems through
[a private advisory](https://github.com/PolarisPyra/Patchwork/security/advisories/new).

## Rules for a PR

These rules apply to every PR from outside the team, drafts included. The team is the logins in
[.github/TRIAGE_EXEMPTIONS.td](.github/TRIAGE_EXEMPTIONS.td). A PR that follows the rules can still be
closed. We do not owe a review or a merge.

<a id="prior-approval"></a>

### Fix a bug

A PR must fix a bug. A crash, a regression, and a real performance problem are bugs. A change to how
the app behaves is a feature, even when the PR calls it a fix.

A small fix for an obvious bug needs no issue. For a larger fix, link the issue that shows the bug.

The only way around this rule is a PR that a maintainer asked you to open. Link the request.

<a id="one-problem"></a>

### One problem per PR

Fix one problem. Include every change that fix needs across the server, the app, and the docs.
Cleanup, refactors, and a second fix go in their own PRs.

<a id="verification"></a>

### Show that it works

Say how you found the bug, how you checked the fix, and what you saw. "Tests pass" is not evidence.

UI changes need before and after screenshots. Add a short video when the change is about motion or
timing. Upload them to the PR. Do not commit them.

<a id="changelog"></a>

### Write the changelog line

The PR title becomes the commit subject: short, sentence-case, imperative, and about what users see.
The PR body carries a line starting with `Changelog:` and its group (Added, Changed, Fixed, Removed,
or Security), written by [the changelog style](docs/changelog-style.md). The release notes are built
from it. A change users can't notice says `Changelog: none`. A check on the PR enforces both.

<a id="open-pr-limit"></a>

### 3 open PRs per person

You can have 3 open PRs. A fourth is closed when you open it. Get one merged or closed, then open it
again.

<a id="conflict-window"></a>

### Keep your PR mergeable

A PR that conflicts with its base branch gets a `needs-rebase` label. If it still conflicts after 7
days with no activity, it is closed.

Vouched contributors in [.github/VOUCHED.td](.github/VOUCHED.td) are exempt from this rule and from
the open PR limit.

## Bans

- If we decide that you spam us with automation, you will be banned.
- If you make our lives harder as reviewers and maintainers, you will be banned. Examples are repeated
  pings, a fight over a closure, and a second account to get around a limit.

<a id="closure-and-reconsideration"></a>

## When your PR is closed

A PR that breaks a rule gets a notice that names the rule and says how to fix it. The PR closes 72
hours later. A reply or a push in that time sends it back for another look.

After it is closed, fix the problem and comment on the PR, or open a focused replacement PR that links
the old one.

## Setup

See [Development](docs/development.md) to set up the repo and run the checks, and
[Architecture](docs/architecture.md) to find the code you want to change. If you change how an agent
CLI is started, read, or approved, update [Agent CLIs](docs/agent-clis.md) in the same PR.
