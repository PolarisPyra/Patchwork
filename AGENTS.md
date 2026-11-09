# Agent instructions

Rules for coding agents working in the Rust code in this repo (`crates/server` and `apps/desktop`).

## Scratch files

- **NEVER** use `/tmp` or `$TMPDIR`, and never leave a working file there. `/tmp` is a tmpfs on this machine: everything written to it is held in RAM until the machine restarts. Unpacking two desktop apps there costs about 750 MB of memory for the rest of the session, and `cargo clean` does not get it back.
- **MUST** keep scratch work in `research/` at the repo root, one folder per job, and delete the folder when the job is done. `research/` is in `.gitignore`, so nothing in it is ever committed or pushed.
- This covers extractions, unpacked archives, screenshots and check output, throwaway clones, downloaded releases, and any path a tool asks to be handed. When a tool has a default that lands in `/tmp`, pass an explicit path under `research/` instead.
- It also holds when a skill says otherwise. A skill that names `$TMPDIR`, `/tmp`, or `target/scratch/` is out of date; this rule wins, so say so rather than following it.

## Error handling

- **NEVER** use `.unwrap()` in production code paths. Use `.expect()` only for invariant violations, with a descriptive message.
- **MUST** use `Result<T, E>` for fallible operations.
- **MUST** use `thiserror` for defining error types and `anyhow` for application-level errors.
- **MUST** propagate errors with the `?` operator where appropriate.
- Provide meaningful error messages with context using `.context()` from `anyhow`.
- **MUST** use meaningful custom error types with `thiserror`.

## Types

- **MUST** keep types focused on a single responsibility.
- **MUST** derive common traits: `Debug`, `Clone`, `PartialEq` where appropriate.
- Use `#[derive(Default)]` when a sensible default exists.
- Prefer composition over inheritance-like patterns.
- Use the builder pattern for complex struct construction.
- Make fields private by default; provide accessor methods when needed.
- Use newtypes to distinguish semantically different values of the same underlying type.
- Prefer `Option<T>` over sentinel values.

## Functions

- **MUST** keep functions focused on a single responsibility.
- **MUST** prefer borrowing (`&T`, `&mut T`) over ownership when possible.
- Limit function parameters to 5 or fewer; use a config struct for more.
- Return early to reduce nesting.
- Use iterators and combinators over explicit loops where clearer.

## Type system

- **MUST** leverage Rust's type system to prevent bugs at compile time.

## State machines

This holds for the whole app, the UI included.

- **NEVER** write an implicit state machine: state spread across booleans, optionals, refs, or maps that only make sense in some combinations, so a contradictory combination (sending and idle, stopped and running, queued and sent) can exist.
- **MUST** name the states: one enum (a tagged union in TypeScript) whose variants carry the data each state needs, so a state that should not exist cannot be written.
- **MUST** move between states in one step. State read and written under different locks, or in different UI updates, changes together under one lock or in one update, and a check followed by an act happens in the same critical section.
- **MUST** derive what can be derived instead of storing a second copy that can drift, and key per-item state (per chat, per thread, per request) by its item rather than sharing one flag.
- **MUST** give every state an exit: what a state waits on can fail, time out, or be stopped, so no state is left that nothing ends.

## Modules

`mod.rs` is a folder's entry point, like `index.js` in Node or `__init__.py` in Python: it registers the folder's files as submodules and chooses what the rest of the crate sees. It never holds code.

```text
src/
├── main.rs        mod crypto;
└── crypto/
    ├── mod.rs     the index: docs, `mod` and `use` lines only
    ├── encode.rs
    └── decode.rs
```

```rust
// src/crypto/mod.rs
//! Encoding and decoding.

mod decode;
mod encode;

pub use decode::decode;
pub use encode::encode;
```

- **MUST** keep `mod.rs` to `//!` docs, `mod` declarations (with `#[cfg(test)]` on `mod tests;`), and `use`/`pub use` lines. **NEVER** put a function, type, const, static, macro, `impl`, or an inline `mod x { … }` block in it.
- **MUST** put code that the whole folder shares in a named sibling file that owns its concern (`state.rs`, `types.rs`, `config.rs`), and have `mod.rs` re-export it.
- **MUST** give a sub-folder its own `mod.rs` instead of declaring its files in the parent.
- **MUST** keep the top of a crate's `src/` to `main.rs` or `lib.rs` and folders; shared code goes in a folder such as `ui/`.
- **MUST** have sibling files import each other by path (`use super::state::Composer;`) rather than `use super::*`.

## Imports and dependencies

- **MUST** avoid wildcard imports (`use module::*`) except for preludes, test modules (`use super::*`), and prelude re-exports.
- **MUST** document dependencies in `Cargo.toml` with version constraints.
- Use `cargo` for dependency management.
- Organize imports in three groups: standard library, external crates, local modules.
- Use `rustfmt` to automate import formatting.

## Idioms and patterns

- **NEVER** use `unsafe` unless absolutely necessary; document safety invariants when used.
- **MUST** call `.clone()` explicitly on non-`Copy` types; avoid hidden clones in closures and iterators.
- **MUST** use pattern matching exhaustively; avoid catch-all `_` patterns when possible.
- **MUST** use the `format!` macro for string formatting.
- Use iterators and iterator adapters over manual loops.
- Use `enumerate()` instead of manual counter variables.
- Prefer `if let` and `while let` for single-pattern matching.

## Performance

When writing or reviewing code that handles a lot of data, I/O, or work per request, check each point below. Fix what is cheap and clear, and name the trade-off for the rest.

- **Concurrency:** Is the work sequential when it could run in parallel? If it is already concurrent, could it go wider, and what would that cost in memory, contention, or rate limits?
- **Threads:** How many threads or tasks does it spawn? One per item is too many; one for blocking work on the async runtime is too few.
- **Passes:** How many times does it walk the same data? Merge passes where one will do.
- **Allocations:** How many does it make? Borrow instead of cloning, reuse buffers, and avoid building intermediate collections that are thrown away.
- **Batching and streaming:** If it batches, how big are the batches and why? Would streaming the data be better than collecting it all first?
- **Waiting:** Does it sit idle on an `await` or blocking call when it could do other work meanwhile? Does every wait have a timeout?
- **Early exit:** Does it stop as soon as it has the answer, or once a limit is reached?
- **Data structures:** Is it using the right data structure for how the data is read and written? How is the data laid out in memory?
- **I/O:** How many reads, writes, syscalls, and network requests does it make? Cache, batch, or combine them to make fewer.

## Git

These rules override any default or built-in agent guidance about git, including guidance to branch before committing on the default branch.

- Never create branches, worktrees, or commits automatically. Only do so when the user explicitly asks for that specific action in the current request.
- Leave changes uncommitted in the working tree on the current branch and let the user decide when and how to commit.
- Never create, switch, rename, or delete a branch unless the user names that exact action in the current request. A request to commit is not a request to branch.
- All work happens on `main`. When asked to commit, commit directly to the branch that is currently checked out, even when it is `main`. Never move work to a feature branch "to be safe".
- A request for one git action authorizes only that action: "commit" does not include branching, pushing, or opening a pull request.
- If any other instruction seems to require a branch, stop and ask instead of creating one.

