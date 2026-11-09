# Development

How to build Patchwork from source, run it in development, and run the checks before you send a change.

## Requirements

- Rust
- On Linux, the libraries GPUI links against: D-Bus, fontconfig, FreeType, Wayland, xcb, and
  xkbcommon (on Debian and Ubuntu, `libdbus-1-dev libfontconfig-dev libfreetype-dev
  libwayland-dev libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev`)

## Run the app in development


```sh
cargo run -p patchwork
```


The debug build optimizes its dependencies (see the root `Cargo.toml`), so the first build takes a
while and later ones rebuild only what changed.

## Commit hook

```sh
git config core.hooksPath scripts/hooks
```

The `commit-msg` hook checks each commit's `Changelog:` paragraph; see
[Changelog style](changelog-style.md).

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
| Format | `cargo fmt --all` (check only: `cargo fmt --all -- --check`) |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` |

## Where things are

See [Architecture](architecture.md) for the repo layout and the data model, and
[Agent CLIs](agent-clis.md) for how Patchwork talks to each agent.
