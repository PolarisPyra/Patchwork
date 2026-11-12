# Changelog style

`CHANGELOG.md` is for Patchwork's users. The app shows it in Settings → Updates, before and after
an update. The `commit-msg` hook (`scripts/hooks/commit-msg`) checks every entry against the
rules below. Each entry is a commit's `Changelog:` paragraph, word for word, so it is
written for users when the commit is.

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
- Keep the tone plain and professional, like release notes from a company you trust: no jokes,
  exclamation marks or drama.

## Fixed entries

- Start with what went wrong for the user, in the past tense, then say what happens now. Write
  "Claude chats waited until the model's context was nearly full before compacting, which used up
  the usage limit much faster. They now compact at 200k tokens", not "Claude chats compact at 200k
  tokens again."
- Don't describe a fix as a return, with words like "again", "back", "restored" or "once more".
  A user who never saw it work reads that as a feature someone removed on purpose and then put back.
- Give the cause only when the user can act on it, such as a setting worth checking.

## Before committing

Run the draft entry through the declauding skill, use its rewrite, and then check the result
against the rules here. The skill takes out phrasing that reads as machine-written; these rules
cover what a Patchwork entry has to say.

## Never

- File paths, file names, function, component, command or setting key names, or anything in
  backticks.
- Pixel sizes, internal limits, or other implementation details.
- Developer words: refactor, component, state, render, hook, IPC, backend, frontend, crate, CSS.
- Credit or blame for agents, tools or libraries used to build Patchwork.
- Changes users can't notice, such as tests, builds, release tooling, or code tidying. They are left
  out, not described.

## Groups

A paragraph names its group first, as in `Changelog: Fixed: …`. A release lists the groups in
this order:

| Group | For |
| --- | --- |
| Added | Something new the user can do or see |
| Changed | Something that works or looks different |
| Fixed | Something that was broken and now works |
| Removed | Something that is gone |
| Security | A fix that protects the user's data or machine |

## One change, several commits

A release often has several commits for one change: the change, a follow-up, a fix for a bug the
change itself introduced. Nothing merges them, so give the change its entry once, describing the end
result, and give the rest `Changelog: none`. A fix for a bug that never reached a release is not a
Fixed entry.
