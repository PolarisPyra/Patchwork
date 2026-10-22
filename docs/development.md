# Development

How to build Patchwork from source, run it in development, and run the checks before you send a change.

## Requirements

- Rust
- pnpm
- The Tauri Linux prerequisites (webkit2gtk-4.1)

## Run the app in development

```sh
pnpm install
pnpm tauri dev
```

`pnpm tauri dev` runs the desktop app with the UI on Vite and the Rust backend rebuilding on save.

## Build

```sh
scripts/build.sh appimage   # target/release/bundle/appimage/Patchwork_amd64.AppImage
scripts/build.sh install    # release binary, icons, and launcher under ~/.local
```

`scripts/build.sh appimage` builds the AppImage under a fixed name, `Patchwork_amd64.AppImage`, so
it doesn't change with the version. `scripts/build.sh install` builds the release binary and
installs it under `~/.local` with its icons and a launcher named **Patchwork**.
Set `PREFIX` to install somewhere else. The launcher's id is
`dev.patchwork.ade`, so it doesn't replace the launcher of an AppImage you already run. The
install step refreshes the desktop, icon, and KDE caches.

## Tests

```sh
cargo test --workspace                    # unit tests (parsers, importers, persistence, git)
cargo test -p patchwork-server -- --ignored  # live turns against the real agent CLIs
```

The `--ignored` tests start real agent CLIs, so they need Claude Code, Codex, or Antigravity
installed and signed in.

## Other checks

| Check | Command |
| --- | --- |
| Web typecheck | `pnpm typecheck` |
| Format TSX | `pnpm format` (check only: `pnpm format:check`) |
| Rust lint | `cargo clippy --workspace --all-targets` |
| UI measurements and screenshots | `pnpm qa:stripe`, `pnpm qa:shot <out.png> [selector]`; see [`scripts/qa/README.md`](../scripts/qa/README.md) |

## Where things are

See [Architecture](architecture.md) for the repo layout and the data model, and
[Agent CLIs](agent-clis.md) for how Patchwork talks to each agent.
