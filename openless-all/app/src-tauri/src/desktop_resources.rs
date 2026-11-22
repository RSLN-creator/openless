//! Desktop window policy is fixed at launch. Preference changes take effect
//! after restart, so an in-flight session never changes renderer ownership.
#[cfg(target_os = "macos")]
#[path = "macos_card_panel.rs"]
mod macos_card_panel;

use std::sync::atomic::{AtomicBool, Ordering};
static LOW_MEMORY_ACTIVE: AtomicBool = AtomicBool::new(false);
pub(crate) fn active() -> bool {
    LOW_MEMORY_ACTIVE.load(Ordering::Relaxed)
}

/// Whether the live-transcript rail is enabled. Fixed at launch, like the rest of
/// the desktop window policy, so an in-flight session never changes topology.
static CAPSULE_TRANSCRIPT_ACTIVE: AtomicBool = AtomicBool::new(true);

#[cfg(not(mobile))]
pub(crate) fn configure(
    context: &mut tauri::Context<tauri::Wry>,
    low_memory: bool,
    capsule_transcript: bool,
) {
    LOW_MEMORY_ACTIVE.store(low_memory, Ordering::Relaxed);
    CAPSULE_TRANSCRIPT_ACTIVE.store(capsule_transcript, Ordering::Relaxed);
    for window in &mut context.config_mut().app.windows {
        if low_memory && matches!(window.label.as_str(), "main" | "capsule" | "capsule-rail") {
            window.create = false;
        }
        // The transcript rail is a separate webview that costs a whole renderer
        // process (~80 MB) whether or not it is ever shown. When the transcript
        // preference is off it was still being created at launch and hidden
        // immediately, because the flag is only consulted later, by the capsule UI
        // at runtime. Skipping creation here is the difference between paying for a
        // feature that is switched off and not paying for it.
        //
        // Windows only: tauri.windows.conf.json is what declares the rail, so other
        // platforms have no such window to skip. `ensure_transcript_rail` creates it
        // on demand if the preference is turned on later.
        if !capsule_transcript && window.label == "capsule-rail" && cfg!(target_os = "windows") {
            window.create = false;
        }
    }
}

/// Whether the transcript rail exists under the current policy.
///
/// Read by the capsule host before touching the rail so it can create it on demand
/// instead of assuming the config declared it.
#[cfg(not(mobile))]
pub(crate) fn capsule_transcript_active() -> bool {
    CAPSULE_TRANSCRIPT_ACTIVE.load(Ordering::Relaxed)
}

/// Creates the transcript rail if the preference asks for it and it is missing.
///
/// Returns the window, or `None` when the feature is off. Kept next to
/// [`ensure_main`] and [`ensure_card_window`], which solve the same problem for the
/// other two windows.
#[cfg(all(not(mobile), target_os = "windows"))]
pub(crate) fn ensure_transcript_rail<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> tauri::Result<Option<tauri::WebviewWindow<R>>> {
    use tauri::Manager;
    if !CAPSULE_TRANSCRIPT_ACTIVE.load(Ordering::Relaxed) {
        return Ok(None);
    }
    if let Some(window) = app.get_webview_window("capsule-rail") {
        return Ok(Some(window));
    }
    let Some(config) = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "capsule-rail")
    else {
        return Ok(None);
    };
    let window = tauri::WebviewWindowBuilder::from_config(app, config)?.build()?;
    // Built visible by default would flash; the caller positions and shows it.
    if let Err(error) = window.hide() {
        log::warn!("[capsule] transcript rail hide after build failed: {error}");
    }
    crate::webview_visibility::sync_webview_visibility(&window, false);
    log::info!("[capsule] transcript rail created on demand");
    Ok(Some(window))
}

#[cfg(not(mobile))]
#[derive(Default)]
pub(crate) struct MainWindowState {
    pub closing: bool,
    pub reopen_pending: bool,
    /// Deferred destruction: the window is hidden at once, but kept alive for
    /// a short grace period so an accidental close/reopen cycle preserves
    /// unsaved form state. `(token, deadline)`; the token guards against a
    /// second close resetting the deadline while the first timer is pending.
    pub hide_grace: Option<(u64, std::time::Instant)>,
    pub hide_grace_seq: u64,
    pub size: Option<tauri::PhysicalSize<u32>>,
    pub position: Option<tauri::PhysicalPosition<i32>>,
    pub maximized: bool,
}

#[cfg(not(mobile))]
pub(crate) type MainWindowLifecycle = parking_lot::Mutex<MainWindowState>;

#[cfg(not(mobile))]
pub(crate) fn initialize_main<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    // macOS deliberately applies no NSVisualEffectView: the React shell paints an opaque
    // surface over the whole content view, so a behind-window blur would be invisible yet
    // keep compositing every frame. Decorations and the Overlay title bar come from
    // tauri.conf.json, which also covers windows recreated by `ensure_main`.
    #[cfg(target_os = "windows")]
    {
        if let Err(error) = window_vibrancy::apply_mica(window, None) {
            log::warn!("[main] mica failed: {error}");
        }
        crate::apply_windows_caption_theme(window, false);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

#[cfg(not(mobile))]
pub(crate) fn ensure_main<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> tauri::Result<Option<tauri::WebviewWindow<R>>> {
    use tauri::Manager;
    if let Some(lifecycle) = app.try_state::<MainWindowLifecycle>() {
        let mut state = lifecycle.lock();
        if state.closing {
            state.reopen_pending = true;
            return Ok(None);
        }
        // Reopening inside the grace window cancels the pending destroy; the
        // hidden WebView is still alive, so drafts and scroll state survive.
        state.hide_grace = None;
    }
    if let Some(window) = app.get_webview_window("main") {
        return Ok(Some(window));
    }
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    let window = tauri::WebviewWindowBuilder::from_config(app, config)?.build()?;
    initialize_main(&window);
    if let Some(lifecycle) = app.try_state::<MainWindowLifecycle>() {
        let state = lifecycle.lock();
        if let Some(size) = state.size {
            let _ = window.set_size(size);
        }
        if let Some(position) = state.position {
            let _ = window.set_position(position);
        }
        if state.maximized {
            let _ = window.maximize();
        }
    }
    Ok(Some(window))
}

#[cfg(not(mobile))]
pub(crate) fn close_main<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if let Some(lifecycle) = app.try_state::<MainWindowLifecycle>() {
        let mut state = lifecycle.lock();
        if state.closing {
            return;
        }
        state.closing = true;
        state.hide_grace = None;
        state.maximized = window.is_maximized().unwrap_or(false);
        // Keep the last normal bounds: reopening a maximized window must not
        // overwrite its restore size with the monitor's dimensions.
        if !state.maximized {
            state.size = window.inner_size().ok();
            state.position = window.outer_position().ok();
        }
    }
    // Let the WebView unload its document before destroying the native window. WebKit can
    // otherwise keep the page's JS/DOM allocator in the shared content process used by the
    // resident capsule, which makes a closed main window look like a memory leak.
    crate::webview_visibility::hide_window_with_webview(&window);
    let _ = window.eval("window.stop(); location.replace('about:blank');");
    let app_for_destroy = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        let app_for_main = app_for_destroy.clone();
        let _ = app_for_destroy.run_on_main_thread(move || {
            let Some(window) = app_for_main.get_webview_window("main") else {
                return;
            };
            if let Err(error) = window.destroy() {
                let reopen = if let Some(lifecycle) = app_for_main.try_state::<MainWindowLifecycle>() {
                    let mut state = lifecycle.lock();
                    let reopen = state.reopen_pending;
                    state.closing = false;
                    state.reopen_pending = false;
                    reopen
                } else {
                    false
                };
                // The unload step intentionally replaced the document before destroy. If the
                // native destroy fails, restore the main document before the next open instead
                // of leaving a hidden, reusable about:blank WebView behind.
                let _ = window.eval("location.replace('index.html')");
                if reopen {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
                log::warn!("[main] destroy after WebView unload failed: {error}");
            }
        });
    });
}

/// How long a hidden WebView remains alive before a real destroy, so accidental
/// close/reopen cycles preserve unsaved state.
#[cfg(not(mobile))]
pub(crate) const MAIN_CLOSE_GRACE: std::time::Duration = std::time::Duration::from_secs(3);

#[cfg(not(mobile))]
pub(crate) fn hide_main_with_grace<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let token = if let Some(lifecycle) = app.try_state::<MainWindowLifecycle>() {
        let mut state = lifecycle.lock();
        if state.closing {
            return;
        }
        state.hide_grace_seq += 1;
        state.hide_grace = Some((
            state.hide_grace_seq,
            std::time::Instant::now() + MAIN_CLOSE_GRACE,
        ));
        state.hide_grace_seq
    } else {
        0
    };
    crate::webview_visibility::hide_window_with_webview(&window);
    let app_for_destroy = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(MAIN_CLOSE_GRACE).await;
        let app_for_main = app_for_destroy.clone();
        let _ = app_for_destroy.run_on_main_thread(move || {
            // Recheck after dispatch: reopening must cancel even an already-fired timer.
            let destroy = app_for_main.try_state::<MainWindowLifecycle>().is_some_and(|lifecycle| {
                matches!(lifecycle.lock().hide_grace, Some((pending, _)) if pending == token)
            });
            if destroy {
                close_main(&app_for_main);
            }
        });
    });
}

#[cfg(not(mobile))]
pub(crate) fn main_destroyed<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    let reopen = app
        .try_state::<MainWindowLifecycle>()
        .is_some_and(|lifecycle| {
            let mut state = lifecycle.lock();
            state.closing = false;
            std::mem::take(&mut state.reopen_pending)
        });
    if reopen {
        crate::show_main_window(app);
    }
}

/// Error/correction cards keep their existing UI, but their WebView is created
/// only while a card is on screen. No recording capsule/rail WebView is needed.
#[cfg(not(mobile))]
pub(crate) fn ensure_card_window<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> tauri::Result<()> {
    use tauri::Manager;
    if app.get_webview_window("capsule").is_some() {
        return Ok(());
    }
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "capsule")
        .ok_or(tauri::Error::WindowNotFound)?;
    #[cfg(target_os = "macos")]
    let initializer = macos_card_panel::CardInitializer::install()?;
    let result =
        tauri::WebviewWindowBuilder::from_config(app, config).and_then(|builder| builder.build());
    #[cfg(target_os = "macos")]
    drop(initializer);
    let window = result?;
    #[cfg(target_os = "macos")]
    if let Err(error) = macos_card_panel::configure(&window) {
        let _ = window.destroy();
        return Err(error);
    }
    window.set_ignore_cursor_events(false)?;
    Ok(())
}

/// Release disposable card content after every dismissal. Shared with the
/// macOS lifecycle fixture so it exercises the production hide/destroy path.
#[cfg(not(mobile))]
pub(crate) fn hide_capsule_window<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> tauri::Result<()> {
    window.hide()?;
    crate::webview_visibility::sync_webview_visibility(window, false);
    if active() {
        window.destroy()?;
    }
    Ok(())
}
