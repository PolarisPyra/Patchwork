# Vendored GPUI Stack

Patchwork keeps a copy of Zed's GPUI crates under `vendor/` instead of pulling them from Zed's
Git repository.

## Why GPUI Lives in `vendor/`

- **Patching.** Patchwork can change GPUI internals (rendering, text, input, platform code) in
  the same pull request as the app code that needs the change. With a Git dependency, every fix
  needs a fork or a `[patch]` entry. Discord's Phoenix keeps its copy of GPUI the same way.
- **One pinned revision.** Every crate comes from the single Zed commit below. Updating GPUI is a
  normal diff that can be reviewed, not a lockfile hash change.
- **Offline builds.** Cargo and CI build GPUI from the repository without cloning Zed.

The cost is upkeep: an upgrade copies the crates again, and any local patches have to be carried
over to the new version by hand.

## Not Workspace Members

`Cargo.toml` sets `exclude = ["vendor"]`, so the vendored crates build as path dependencies of the
app rather than as workspace members. `cargo clippy --workspace --all-targets` then skips GPUI's
upstream tests, benches and examples, whose dev-dependencies (`proptest`, `ctor` and others) are
not vendored. Clippy and rustfmt run on Patchwork's code, not on Zed's.

## Pinned Upstream Revision

- Repository: `https://github.com/zed-industries/zed`
- Commit: `d2a4fb6ded7c470be25d1541281da3882495b65b`

## Vendored Crates

| Crate | Role |
| --- | --- |
| `gpui` | Core retained/immediate GPU UI runtime, element tree, layout, SVG renderer, actions |
| `gpui_platform` | Cross-platform backend selector and app entrypoint |
| `gpui_wgpu` | Shared `wgpu` rendering backend and `cosmic-text` text system |
| `gpui_linux` | Linux Wayland / X11 platform windowing and input |
| `gpui_macos`, `gpui_apple`, `gpui_ios` | Apple Metal / AppKit / CoreText platform backends |
| `gpui_windows` | Windows Win32 / DirectWrite platform backend |
| `gpui_web` | Web/WASM platform backend |
| `gpui_android` | Android platform backend, written for Patchwork (not from Zed) |
| `gpui_macros`, `gpui_shared_string`, `gpui_util` | GPUI derive macros, shared strings, and helpers |
| `collections`, `http_client`, `refineable`, `derive_refineable`, `scheduler`, `sum_tree`, `ztracing`, `ztracing_macro` | Supporting Zed crates required by GPUI |

## Patchwork Patches

`gpui_android` is Patchwork's own crate, not a copy of Zed's. It runs GPUI in an `android-activity`
NativeActivity: the activity's looper drives the main thread, `gpui_wgpu` renders to the activity's
window, touches go to GPUI as `PlatformInput::Touch`, and Roboto is loaded from `/system/fonts`.
`scripts/vendor-gpui.py` does not touch it.

Regenerating replaces the upstream crates, so these edits must be made again afterwards. Each one
is marked with a `Patchwork: Android support` comment:

- `gpui/src/gpui.rs`: builds `queue` and exports `PriorityQueueReceiver` and
  `PriorityQueueSender` on `target_os = "android"`.
- `gpui_wgpu/src/wgpu_context.rs`: uses only the Vulkan backend on Android.
- `gpui_platform/Cargo.toml`: depends on `gpui_android` on Android.
- `gpui_platform/src/gpui_platform.rs`: `current_platform` returns
  `gpui_android::current_platform` on Android.

## How to Regenerate or Upgrade

1. Check out the desired revision of `zed-industries/zed` locally (or use the cached Cargo checkout).
2. Update `ZED_REV` in `scripts/vendor-gpui.py` and in this file.
3. Run from the repository root:

   ```sh
   uv run --no-project python3 scripts/vendor-gpui.py --zed-dir /path/to/zed
   ```

4. Apply the Patchwork patches above again.
5. Refresh `Cargo.lock` and verify the build:

   ```sh
   cargo build
   scripts/check.sh
   cargo run android --no-run
   ```
