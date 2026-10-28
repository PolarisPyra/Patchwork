---
name: review-diff
description: Review the uncommitted change in this project before it is kept. Use when the user asks for a review, or before staging a change with git add.
---

# Review a change

Read the change, then say what is wrong with it. Do not edit anything unless the user asks for the
fix too.

1. `git status --short` for the files that changed.
2. `git diff` for the change itself, and `git diff --stat` when it is long. A file the diff leaves
   out is a new file: read it whole.
3. `git log --oneline -5` for what the change is building on.

For each file, answer three things:

- **What it does.** One sentence, in the user's terms.
- **What could break.** The case the change does not handle, named concretely: an input, a state, a
  platform. Do not list hypothetical risks that no input can reach.
- **Whether it belongs.** Debug code, unrelated edits, a dependency added for one call, a
  commented-out block. Say so plainly.

Then give the verdict: keep, keep with a named fix, or drop. Keep it short. No summary of the diff
the user can already see.

Check the parts a reader skips: error paths, whether a new branch is tested, and whether a public
function's doc comment still matches it.
