//! Keeps the WebView2 controller's visibility in step with its window.
//!
//! Tauri's `WebviewWindow::hide()` reaches only the window layer: it ends at
//! `tao` clearing `WS_VISIBLE`, i.e. `ShowWindow(SW_HIDE)` on the parent HWND.
//! It never touches `CoreWebView2Controller.IsVisible`, so WebView2 keeps
//! treating a "hidden" window as visible. Measured on this app: three windows
//! that were all hidden still held three renderer processes, ~297 MB.
//!
//! Microsoft documents the consequence and the fix:
//! "WebView2 as a child window does not get window messages when the top window
//! is minimized or restored. For performance reasons, developers should set the
//! IsVisible property of the WebView to FALSE when the app window is minimized."
//! A hidden webview also "will purge some caches to reduce memory usage", and
//! `TrySuspend` rejects any webview whose `IsVisible` is still true.
//!
//! `Webview::hide()` does reach `SetIsVisible(false)`, but its `SW_SHOW` half is
//! not paired with `SetIsVisible(true)`. Showing through the Win32 no-activate
//! paths the capsule relies on would therefore leave the webview itself
//! invisible, so visibility is driven explicitly here instead.

use tauri::WebviewWindow;

/// Brings every window that is not on screen into the hidden state.
///
/// Windows declared `visible: false` are built with the WebView2 controller
/// still reporting itself visible — Tauri forwards `visible` to the window layer
/// only — so without this pass those windows would keep a renderer resident from
/// process start, before any hide path runs.
pub(crate) fn sync_hidden_windows<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    for (label, window) in app.webview_windows() {
        if window.is_visible().unwrap_or(false) {
            continue;
        }
        log::info!("[webview] {label} starts hidden; syncing WebView2 IsVisible=false");
        sync_webview_visibility(&window, false);
    }
}

/// Propagates `visible` to the window's WebView2 controller.
///
/// Call with `true` on every show path and `false` on every hide path. The two
/// directions are deliberately not symmetric in consequence: a missed `true`
/// leaves a blank window, while a missed `false` only forgoes the memory saving,
/// so show paths apply `true` unconditionally rather than tracking state.
#[cfg(target_os = "windows")]
pub(crate) fn sync_webview_visibility<R: tauri::Runtime>(
    window: &WebviewWindow<R>,
    visible: bool,
) {
    // `with_webview` posts to the event loop, so the closure body runs on the
    // thread that owns the controller — the only thread COM may be used from.
    let dispatched = window.with_webview(move |platform| {
        let controller = platform.controller();
        // SAFETY: called on the UI thread that owns this controller; the call
        // only sets a visibility flag and its HRESULT is intentionally ignored
        // because a failed sync must never break the window transition.
        let outcome = unsafe { controller.SetIsVisible(visible) };
        if let Err(error) = outcome {
            log::warn!("[webview] SetIsVisible({visible}) failed: {error}");
        }
    });
    if let Err(error) = dispatched {
        log::warn!("[webview] visibility sync dispatch (visible={visible}) failed: {error}");
    }
}

/// Non-Windows keeps the pre-existing behaviour: window visibility already
/// drives WebKit's own occlusion handling.
#[cfg(not(target_os = "windows"))]
pub(crate) fn sync_webview_visibility<R: tauri::Runtime>(
    _window: &WebviewWindow<R>,
    _visible: bool,
) {
}

/// Hides a window and tells its webview it is hidden.
pub(crate) fn hide_window_with_webview<R: tauri::Runtime>(window: &WebviewWindow<R>) {
    if let Err(error) = window.hide() {
        log::warn!("[webview] window hide failed: {error}");
    }
    sync_webview_visibility(window, false);
}

/// Shows a window and re-enables its webview.
///
/// The webview flag is set even when `show()` reports an error: a window that is
/// already visible is exactly the case where the flag must be repaired.
pub(crate) fn show_window_with_webview<R: tauri::Runtime>(window: &WebviewWindow<R>) {
    if let Err(error) = window.show() {
        log::warn!("[webview] window show failed: {error}");
    }
    sync_webview_visibility(window, true);
}
