//! Runs blocking backend calls off the UI thread and hands the result back to a view.
//!
//! Backend commands (DuckDB reads, saves, imports) block, so views call them through
//! these helpers instead of directly in a render or click handler. In headless tests
//! the work runs on GPUI's test executor; `Harness::settle` drives it.

use gpui_kit::*;

/// Runs `work` on the background executor, then `done` on the view (if it still exists).
pub fn spawn_call<V: 'static, T: Send + 'static>(
    cx: &mut Context<V>,
    work: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(&mut V, T, &mut Context<V>) + 'static,
) {
    let job = cx.background_spawn(async move { work() });
    cx.spawn(async move |this, cx| {
        let value = job.await;
        let _ = this.update(cx, |view, cx| done(view, value, cx));
    })
    .detach();
}

/// Like `spawn_call`, with the window available to `done` (to focus or update inputs).
pub fn spawn_call_in<V: 'static, T: Send + 'static>(
    window: &Window,
    cx: &mut Context<V>,
    work: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(&mut V, T, &mut Window, &mut Context<V>) + 'static,
) {
    let job = cx.background_spawn(async move { work() });
    cx.spawn_in(window, async move |this, cx| {
        let value = job.await;
        let _ = this.update_in(cx, |view, window, cx| done(view, value, window, cx));
    })
    .detach();
}

/// Awaits a GPUI task (a dialog, say) and then runs `done` on the view.
pub fn after<V: 'static, T: 'static>(
    cx: &mut Context<V>,
    task: Task<T>,
    done: impl FnOnce(&mut V, T, &mut Context<V>) + 'static,
) {
    cx.spawn(async move |this, cx| {
        let value = task.await;
        let _ = this.update(cx, |view, cx| done(view, value, cx));
    })
    .detach();
}

/// `after` with the window available to `done`.
pub fn after_in<V: 'static, T: 'static>(
    window: &Window,
    cx: &mut Context<V>,
    task: Task<T>,
    done: impl FnOnce(&mut V, T, &mut Window, &mut Context<V>) + 'static,
) {
    cx.spawn_in(window, async move |this, cx| {
        let value = task.await;
        let _ = this.update_in(cx, |view, window, cx| done(view, value, window, cx));
    })
    .detach();
}
