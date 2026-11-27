# Development

How to build Patchwork from source, run it in development (with or without hot reload), and run the checks before you send a change.

## Requirements

- Rust (edition 2024; see `rust-version` in `Cargo.toml`)
- On Linux, the libraries GPUI links against: D-Bus, fontconfig, FreeType, Wayland, xcb, and
  xkbcommon (on Debian and Ubuntu, `libdbus-1-dev libfontconfig-dev libfreetype-dev
  libwayland-dev libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev`)

## Run the app in development

```sh
cargo run
```

`apps/desktop` (`patchwork`) is the workspace's default member, so `cargo run` and `cargo build`
target the desktop app directly (`cargo run -p patchwork` is equivalent). The debug build optimizes
its dependencies (see the root `Cargo.toml`), so the first build takes a while and later ones
rebuild only what changed.

### Hot reload

```sh
cargo run desktop
```

`cargo run desktop` runs the app under `dx serve --hot-patch` (the Cargo runner in
`.cargo/config.toml`, `scripts/cargo-runner.sh`, starts it), with the app's `hot-reload` feature,
which turns on GPUI's own hot reload. Each save is compiled into a patch and applied to the running
app, and every window draws again with the new code, keeping its state. Every GPUI view renders
through a patchable function, so this covers all of the UI, in every workspace crate.

Not every change can be patched in: after changing a struct's fields, or code that only runs at
startup, press Shift+F5 to rebuild and restart the app. If dx fails to build a patch, it rebuilds
and restarts the app by itself.

`scripts/dx-serve.py` runs dx in a pseudo-terminal and gives its shortcuts function keys, in its
terminal and in the app:

| Key | What it does |
| --- | --- |
| F7 | Pause or resume hot reload (dx's `p`) |
| F5 | Apply changes: patch every Rust file saved since the last F5 into the app |
| Shift+F5 | Rebuild and restart the app (`r`) |
| F8 | Verbose logging, in dx's terminal (`v`) |
| F1 | More commands, in dx's terminal (`/`) |

Saves apply by themselves; F5 is the fallback for one that was missed. While hot reload is paused
dx ignores saves: F5 patches them in and stays paused, and resuming patches them too. dx patches
only files it sees change, so `dx-serve.py` rewrites each Rust file saved since the last F5 with the
bytes it already holds, and dx skips the ones it has already patched. While paused it resumes dx
first and pauses it again after.

The toolbar shows a bar before the version while the app runs under dx: what dx is doing (patching,
rebuilding, a failed build, paused) and buttons for the keys above. In the app, F5, Shift+F5 and F7
go to dx even in a terminal tab. GPUI reports dx's status to the app as its `HotReloadStatus`
global, and the app sends the keys to `dx-serve.py` over the Unix socket it names in
`PATCHWORK_DX_CONTROL`.

Options after `desktop` go to `dx serve`. It needs `uv` and the dx CLI at the version GPUI's
`subsecond` pins: `cargo binstall dioxus-cli@0.7.10`.

## Vendored GPUI and icons

- **GPUI** lives under `vendor/` (excluded from workspace members in `Cargo.toml` and linked as path
  dependencies). See [vendor/README.md](../vendor/README.md) for the pinned upstream commit and
  regeneration steps.
- **Icons** live as SVGs under `assets/icons/`. `apps/desktop/crates/tokens` (`patchwork-ui-tokens`)
  packs and compresses them at build time into `icons.pack` and generates the typed `IconName` enum;
  in debug builds, icons are read from disk, so an edited icon shows on the next launch.

## Build the AppImage

```sh
scripts/build.sh
```

It writes `target/release/Patchwork_amd64.AppImage`; see [Releasing](release.md). The AppImage
carries the helpers (`apps/remote`, `patchwork-remote`) Patchwork copies to SSH hosts, which the
script cross-compiles, so it needs `zig` and `cargo-zigbuild` on `PATH`, and these targets in
rustup:

```sh
rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl x86_64-pc-windows-gnu
```

## Build the Android client

```sh
cargo run android
```

The Cargo runner hands `cargo run android` to `scripts/cargo-android`. It builds `target/android/Patchwork.apk` for the device or emulator `adb` sees, then installs and
opens it. If none is connected, it starts an emulator first: `$PATCHWORK_AVD`, or the first AVD
`emulator -list-avds` prints. `cargo run --release android` builds the release profile, `--abi x86_64` or
`--abi arm64-v8a` picks the ABI, and `--no-run` only builds the APK, for both ABIs unless `--abi`
is given. It needs a JDK, the Android SDK at `$ANDROID_HOME` (default `~/Android/Sdk`) with an
NDK, build-tools, a platform and the emulator, and these targets in rustup:

```sh
rustup target add x86_64-linux-android aarch64-linux-android
```

Start emulators yourself with `-gpu host`, or set `hw.gpu.mode=host` in the AVD's `config.ini`:
SwiftShader's Vulkan crashes the emulator on wgpu's shaders.

## Git hooks

```sh
git config core.hooksPath scripts/hooks
```

The `pre-push` hook runs `cargo fmt --all -- --check`, clippy with `-D warnings` and
`node --check` on the website's script before each push, on the files in the working tree. It
takes a couple of seconds when nothing has changed since the last build and a few seconds after an
incremental edit to `crates/patchwork_backend` or a desktop crate. `git push --no-verify` skips it;
CI runs the same checks either way ([CI and deploys](ci.md)).

## Checks

Run all workspace checks with:

```sh
scripts/check.sh
```

| Check | Command |
| --- | --- |
| Format | `cargo fmt --all` (check only: `cargo fmt --all -- --check`) |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` |

## Where things are

See [Architecture](architecture.md) for the repo layout and the data model, and
[Agent CLIs](agent-clis.md) for how Patchwork talks to each agent.
