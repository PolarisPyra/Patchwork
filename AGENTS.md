# Agent instructions

Rules for coding agents working in the Rust code in this repo (`apps/server` and `apps/desktop/src-tauri`).

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

## Benchmarks

- **NEVER** run benchmarks in parallel, as the benchmarks will compete for resources and the results will be invalid.
- **NEVER** game the benchmarks. Do not manipulate the benchmarks themselves to satisfy any required performance constraints.
- **NEVER** run benchmarks with `target-cpu=native` or any other `RUSTFLAGS`.
- If benchmarking against another crate or library, ensure the benchmarks are apples-to-apples comparisons.
- Ensure benchmark tests are independent. If the tests are dependent due to a feature (e.g. caching), ensure the feature is disabled.

## Pushing and releasing

- **Pushing to `main`** (no tag) is a plain `git push origin main`. It builds nothing. Only push when the user asks.
- **Tagged builds** are releases. Cut one only when the user asks, with `scripts/release.sh <version>` (for example `scripts/release.sh 0.2.0`) from a clean `main` that matches `origin/main`. The script runs the checks, shows the release notes, and waits for confirmation before it commits `Release v<version>`, creates the annotated `v<version>` tag, and pushes `main` and the tag.
- Pushing the tag starts the **Release AppImage** workflow (`.github/workflows/release-appimage.yml`). It writes the tag's version into the package and Cargo manifests, builds the AppImage on Ubuntu 22.04, and creates a GitHub release named `Patchwork v<version>`.
- To check the build without a release, run the workflow by hand from the Actions tab (**Run workflow**). It builds with the placeholder version and creates no release.
- Never edit `CHANGELOG.md` by hand, and never set version numbers by hand. Tags must start with `v` followed by a digit.
- Full details are in [docs/releasing.md](docs/releasing.md).
