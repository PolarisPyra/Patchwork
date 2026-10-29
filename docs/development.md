# Development

How to build Patchwork from source, run it in development, and run the checks before you send a change.

## Requirements

- Rust
- pnpm
- The Tauri Linux prerequisites (webkit2gtk-4.1), or on Windows the Microsoft C++ Build Tools
  and Node.js (the install script adds pnpm when it is missing)

## Run the app in development

```sh
pnpm install
pnpm tauri dev
```

`pnpm tauri dev` runs the desktop app with the UI on Vite and the Rust backend rebuilding on save.

## Build

```sh
scripts/build.sh install    # release binary, icons, and launcher under ~/.local
```

`scripts/build.sh install` builds the AppImage and installs it under `~/.local` with its icons and
a launcher named **Patchwork**.
Set `PREFIX` to install somewhere else. The launcher's id is
`dev.patchwork.ade`, so it doesn't replace the launcher of an AppImage you already run. The
install step refreshes the desktop, icon, and KDE caches.

On Windows, from PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\build.ps1 install
```

It builds the installer and runs it silently, which installs Patchwork for your user under
`%LOCALAPPDATA%\Patchwork` with a Start menu shortcut. Quit Patchwork first, since Windows locks the
files of a running app. A Git for Windows install gives agents Git Bash, which they run commands in
by default; **Settings → Environment** can switch them to PowerShell or cmd.

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

## Where things are

See [Architecture](architecture.md) for the repo layout and the data model, and
[Agent CLIs](agent-clis.md) for how Patchwork talks to each agent.
