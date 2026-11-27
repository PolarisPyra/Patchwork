// Patchwork: hot reload status. Not in Zed's GPUI; see vendor/README.md.

use crate::Global;

/// What hot reload last heard from the dx development server.
///
/// Hot reload sets this global once it connects to dx and again on every change, so an app can
/// show it with `cx.try_global::<HotReloadStatus>()` and follow it with `cx.observe_global`.
/// Without the `hot-reload` feature, or when dx isn't running, it is never set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotReloadStatus {
    /// Connected, with nothing to build.
    Ready,
    /// dx is compiling a patch for a change.
    Patching,
    /// The last patch is in the running app.
    Patched,
    /// dx is rebuilding the whole app, and will restart it.
    Rebuilding,
    /// The last patch or rebuild failed to compile.
    Failed,
    /// The connection to dx closed.
    Disconnected,
}

impl Global for HotReloadStatus {}
