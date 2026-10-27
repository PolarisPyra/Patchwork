# Changelog style

`CHANGELOG.md` is for Patchwork's users. The app shows it in Settings → Updates, before and after
an update, and it is the text of each GitHub release. `scripts/changelog.sh` checks every entry
against the rules below, and `scripts/release.sh` gives this file to the model that drafts each
release's notes.

## Each entry

- One bullet per change a user can see. Say what they see or can do now, where, and what is
  different from before.
- Write for someone who uses Patchwork but has never seen its code. Everyday words, full sentences,
  present tense.
- Name places in the app the way the user finds them: "Settings → Skills", "the sidebar", "the
  chat box", "a chat's right-click menu".
- Say what a change is for when it isn't obvious. Not "Use the custom spinner everywhere" but
  "Every loading indicator now uses the steady spinner: plan steps, background tasks, … The old one
  rotated and looked like it shook."
- An exception gets its own bullet in the same plain words.
- One to three sentences. No line breaks inside an entry.

## Never

- File paths, file names, function, component, command or setting key names, or anything in
  backticks.
- Pixel sizes, internal limits, or other implementation details.
- Developer words: refactor, component, state, render, hook, IPC, backend, frontend, crate, CSS.
- Credit or blame for agents, tools or libraries used to build Patchwork.
- Changes users can't notice, such as tests, builds, release tooling, or code tidying. They are left
  out, not described.

## Groups

Each entry goes in one group, in this order:

| Group | For |
| --- | --- |
| Added | Something new the user can do or see |
| Changed | Something that works or looks different |
| Fixed | Something that was broken and now works |
| Removed | Something that is gone |
| Security | A fix that protects the user's data or machine |

## Merging

A release often has several commits for one change: the change, a follow-up, a fix for a bug the
change itself introduced. They become one entry describing the end result. A fix for a bug that
never reached a release is not a Fixed entry.
