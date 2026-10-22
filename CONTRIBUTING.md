# Contributing to Patchwork

Thanks for helping out. Bug reports, fixes, and new guides are all welcome.

## Before you start

- For a larger change, open an issue first so we can agree on the approach before you write code.
- Read [Development](docs/development.md) to set up the repo and see which checks to run.
- Read [Architecture](docs/architecture.md) to find the code you want to change.

## Making a change

1. Fork the repo and create a branch for your change.
2. Keep the change focused. One fix or feature per pull request is easier to review.
3. Run the checks that match what you changed:
   - Web (`apps/web`): `pnpm typecheck`, and `pnpm format:check` for TSX changes
   - Rust (`apps/server`, `apps/desktop`): `cargo clippy --workspace --all-targets` and `cargo test --workspace`
4. Open a pull request. Describe what changed and why, and include a screenshot for UI changes.

## Commit messages

Changelog entries are generated from commit subjects, so write each subject as a short,
sentence-case summary that starts with a verb, such as `Add ...`, `Fix ...`, or `Change ...`.
See [Releasing](docs/releasing.md) for how the groups are built.

## Adding documentation

Docs live in [`docs/`](docs/README.md). If you find a gap, such as a setup step that tripped you up
or a behavior that isn't written down, add a guide or update the one that covers it. See
[Adding a guide](docs/README.md#adding-a-guide) for the format.

## Working on the agents

Patchwork runs the agent CLIs as separate processes. If you change how one is started, read, or
approved, update [Agent CLIs](docs/agent-clis.md) in the same pull request, since that page is the
reference for those details.

## Questions

Open a discussion or an issue if something here is unclear.
