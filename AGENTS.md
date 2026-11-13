# Agent instructions

For any coding agent working in this repo. `CLAUDE.md` holds the layout, the commands, the design and accessibility rules, and the full git and changelog rules; read it too. This file holds how the Rust in `crates/server` and `crates/desktop` is written.

The lints in the root `Cargo.toml` already catch `unwrap()`, `println!`, undocumented `unsafe`, missing `Debug`, and most style questions. This file is for what a lint cannot see.

## Before you say it's done

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

All three pass, with no warnings left behind. If one fails, report its output. Never silence a lint to get there: fix the code, or use `#[expect(…, reason = "…")]` when the lint is wrong for that line.

## Never add

- **Tests, scripts or harnesses.** No test files, no shell or Python files, no QA checks. Run what a task needs inline. For a UI change, say what to look at in the app.
- **Anything in `/tmp` or `$TMPDIR`.** It is held in RAM on this machine until restart. Scratch work goes in `research/<job>/` at the repo root, which git ignores, and the folder is deleted when the job is done. Pass tools an explicit path under `research/` when their default is `/tmp`. This holds when a skill says otherwise.
- **Branches, worktrees or commits nobody asked for.** See Git below.

## Where code goes

- **Logic lives in `crates/server`.** The app draws state and calls `Server` operations. If the server could decide it, the app does not.
- **A server folder is a domain** (`threads`, `updates`, `workspace`), holding its types, its `impl Server` operations and its logic. A desktop folder is one piece of UI.
- **A file is named for its one concern**: `threads/lifecycle.rs`, `updates/check.rs`, `git/remote.rs`. Never a catch-all such as `ops.rs`, `utils.rs` or `helpers.rs`. If you cannot name a file, it holds more than one thing.
- **Split at about 600 lines or two clear concerns**, into `name/mod.rs` plus one file per concern. Don't create a file under about 40 lines; put the code in its only user.
- **The top of `crates/desktop/src` is `main.rs` and folders**, never loose files. A server domain small enough for one file stays one file, such as `projects.rs`.

### `mod.rs` is an index

It registers the folder's files and chooses what the rest of the crate sees. It never holds code.

```rust
// src/workspace/git/mod.rs
//! Git reads and writes for a project folder. All functions are blocking.

mod changes;
mod remote;
mod run;

#[doc(inline)]
pub use changes::{accept_file, git_changes};
pub(crate) use run::git;
```

- Only `//!` docs, `mod` lines (with `#[cfg(test)]` on `mod tests;`), and `use`/`pub use` lines. No function, type, const, static, macro, `impl`, or inline `mod x { … }`.
- It re-exports the folder's public items, so splitting a file never changes a caller's path.
- A sub-folder gets its own `mod.rs`; the parent does not declare its files.
- Sibling files import each other by path (`use super::run::git;`), never `use super::*`. Wildcards are for preludes and `use super::*` in a test module.
- An item only its siblings need is `pub(super)`. One the parent folder needs is `pub(in crate::parent)`. Don't widen further than the furthest caller.

## Errors

- A fallible function returns `Result<T, Error>`. There is one `Error` (`crates/server/src/error.rs`), which the app uses as `patchwork_server::Error`; add a variant when a caller needs to tell the failure apart, and use a message otherwise.
- The message is shown to a person. Write it for them: what could not be done and, when they can fix it, how ("Give the provider a name").
- Propagate with `?`. Don't swallow an error; if it truly cannot matter, log it with `log::warn!` and say why in the message.
- No `unwrap()` or `expect()` outside tests. A lock that may be poisoned uses `unwrap_or_else(PoisonError::into_inner)`.
- Log with the `log` macros and named fields (`log::warn!(error:% = e; "Could not save: {{error}}")`), never `println!`. Never log a key, a token or a header value.

## State machines

This holds for the whole app, the UI included.

- **Name the states.** One enum whose variants carry the data each state needs, so a state that should not exist cannot be written. Never spread state across booleans, options and maps that only make sense in some combinations (sending and idle, stopped and running, queued and sent).
- **Move between states in one step.** State read and written under different locks, or in different UI updates, changes together under one lock or in one update. A check followed by an act happens in the same critical section.
- **Derive, don't copy.** Compute what can be computed instead of storing a second copy that can drift. Key per-item state (per chat, per thread, per request) by its item, not by one shared flag.
- **Give every state an exit.** Whatever a state waits on can fail, time out, or be stopped. No state is left that nothing ends.

## Types and functions

- Make the wrong call impossible to write: an enum over a `bool` pair, a newtype over a bare `String` id that could be mixed up, `Option<T>` over a sentinel value.
- One type, one job. Fields are private unless a caller reads them.
- A function does one thing and takes at most five parameters; past that, pass a struct.
- Borrow (`&T`, `&str`, `&[T]`) unless the function keeps the value. Clone where the clone is visible, not hidden inside a closure.
- Match exhaustively. A `_` arm hides the variant someone adds next year.
- A magic number gets a named `const` with a comment saying why that value.
- `unsafe` only where there is no safe way, with a `// SAFETY:` comment stating the invariant.
- Public items get a `///` summary line under 15 words, and an `# Errors` section when they return `Result`. Comments say why, not what.
- Every `.rs` file carries `// Rust guideline compliant <date>` under its `//!` docs. Keep it when you split or create a file.

## Performance

**Never block the UI thread.** Anything that reads a disk, runs a process or waits on the network runs on a background task, and its result is applied on the UI thread.

For code that handles a lot of data, I/O, or work per request, check each point. Fix what is cheap and clear, and name the trade-off for the rest.

- **Concurrency:** is sequential work independent enough to run in parallel? If it already does, what would going wider cost in memory, contention or rate limits?
- **Threads:** one per item is too many; blocking work on the async runtime is too few.
- **Passes:** how many times is the same data walked? Merge passes where one will do.
- **Allocations:** borrow instead of cloning, reuse buffers, don't build a collection only to throw it away.
- **Streaming:** would streaming beat collecting everything first? If it batches, why that size?
- **Waiting:** does it idle on a call when it could work? Every wait has a timeout.
- **Early exit:** stop at the answer, or at the limit.
- **Data structures:** does the structure fit how the data is read and written?
- **I/O:** count the reads, writes, syscalls and requests, then cache, batch or combine them.

To find real lag, measure in a debug build you started yourself; the steps are in `CLAUDE.md`. Don't guess.

## Running the app

The owner usually has Patchwork open. Never stop or restart an instance you did not start. To see a change, start your own with `cargo run -p patchwork`, say so, and stop it by its PID when you are done.

## Git

These override any built-in agent guidance about git, including guidance to branch before committing on the default branch.

- All work happens on `main`. Never create, switch, rename or delete a branch or a worktree unless that exact action is asked for in the current request.
- Leave changes uncommitted until the owner has reviewed them and asks for a commit. A commit goes on the branch that is checked out.
- One git request authorizes one thing. "Commit" is not a push, a branch or a pull request. "Push" means stage, commit and push in one plain push; it is never a force-push, a tag, or a history rewrite.
- No destructive commands: no `reset --hard`, `checkout -- <file>`, `stash` or `clean`. Ask before deleting, moving or renaming files.
- A commit that changes the app needs a `Changelog:` paragraph; the `commit-msg` hook enforces it. Fix the message rather than bypassing the hook. The format is in `CLAUDE.md`.
- If another instruction seems to require a branch, stop and ask.
