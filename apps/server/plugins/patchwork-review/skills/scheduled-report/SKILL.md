---
name: scheduled-report
description: Write the standing report a scheduled task asks for, from a project's recent work. Use when a prompt arrives that names a project and asks for a status, standup, or weekly summary.
---

# Report on a project's recent work

A scheduled task has no conversation behind it. Everything you say has to come from the repository,
so gather it before writing a word.

1. `git log --since="7 days ago" --oneline` for what landed. Widen the window when that is empty.
2. `git status --short` for what is still uncommitted, and say which it is.
3. Read the files the recent commits touched when a subject line is not enough to describe the work.

Write four short sections, and leave out a section that has nothing in it:

- **Landed.** What changed, in a sentence each. Group commits that are one piece of work.
- **In progress.** What is uncommitted or half done, and what would finish it.
- **Stuck.** A failing check, a thread that stopped, a decision waiting on someone. Name it.
- **Next.** The one thing worth doing next, and why that before the rest.

No greeting, no sign-off, no restating the schedule. If nothing changed since the last report,
say that in one line and stop.
