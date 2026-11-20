//! Work a view hands off and takes back: to a background thread, a timer, or a file picker.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::time::Duration;

use gpui::{AppContext as _, Context, PathPromptOptions};

/// Runs `work` off the UI thread, then hands its result to `done` on the view. Nothing is
/// handed back when the view is gone by then.
pub fn off_thread<V: 'static, T: Send + 'static>(
    cx: &mut Context<V>,
    work: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(&mut V, T, &mut Context<V>) + 'static,
) {
    let task = cx.background_spawn(async move { work() });
    cx.spawn(async move |this, cx| {
        let out = task.await;
        let _ = this.update(cx, |view, cx| done(view, out, cx));
    })
    .detach();
}

/// Runs `then` on the view once `delay` has passed, unless the view is gone by then.
pub fn after<V: 'static>(
    cx: &mut Context<V>,
    delay: Duration,
    then: impl FnOnce(&mut V, &mut Context<V>) + 'static,
) {
    cx.spawn(async move |this, cx| {
        cx.background_executor().timer(delay).await;
        let _ = this.update(cx, then);
    })
    .detach();
}

/// Asks for files or folders as `options` says, then hands `then` what was picked. Nothing is
/// handed back when the person cancels, the picker fails, or the view is gone by then.
pub fn pick_paths<V: 'static>(
    cx: &mut Context<V>,
    options: PathPromptOptions,
    then: impl FnOnce(&mut V, Vec<PathBuf>, &mut Context<V>) + 'static,
) {
    let picked = cx.prompt_for_paths(options);
    cx.spawn(async move |this, cx| {
        if let Ok(Ok(Some(paths))) = picked.await {
            let _ = this.update(cx, |view, cx| then(view, paths, cx));
        }
    })
    .detach();
}
