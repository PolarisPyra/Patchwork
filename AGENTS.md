# Agent instructions

Rules for coding agents working in the Rust code in this repo (`crates/server` and `apps/desktop/src-tauri`).

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

- **NEVER** run a git command that changes the repository unless the user asks for it in the current conversation. That covers `git add`, `git commit`, `git reset`, `git restore`, `git checkout`, `git stash`, `git branch`, `git merge`, `git rebase`, `git tag`, `git push`, and anything else that writes. A request to do the work, to "finish up", or to use a skill is not a request for any of these.
- Read-only git is fine and expected, because the work needs it: `git status`, `git diff`, `git log`, `git show`, `git ls-files`, `git check-ignore`.
- When the work is done, **stop and summarize**: what changed, what you proved, and anything left undone. Leave the changes in the working tree. Do not stage them, do not commit them, do not push them. This is the default when nobody asked; an explicit "push" is an ask that covers the whole job, committing what is there and pushing it.
- The same goes for anything that publishes: `scripts/release.sh`, a version bump that ends in a tag, and `gh release`. Those commit and push, so they need an explicit request too. Read [docs/release.md](docs/release.md) before any of them, once the user has asked.
- "Push to git bump ver", "push to git bump version", "bump version" and "bump ver" are that request: run `scripts/release.sh bump` for the year's next release, since versions are CalVer `YY.N.P` (26.1.3 → 26.2.0, and 26.3.0 → 27.1.0 in a new year). Use `scripts/release.sh bump patch` for a fix to the last release (26.2.0 → 26.2.1); the scheme has no minor or major level. The script counts from the newest tag, so never compute the version by hand.
- This overrides every skill that says otherwise, including `fullsend`. "Full send" grants autonomy over the work and over testing, not over git: do the work, prove it, summarize, and stop.

## New Tauri projects

The pnpm + Cargo Tauri layout, the values to set, and the release scripts for a new project are in [docs/new-tauri-project.md](docs/new-tauri-project.md). Read it only when setting one up, not for changes to Patchwork.
