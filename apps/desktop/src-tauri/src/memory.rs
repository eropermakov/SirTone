//! Memory in the tray (T5.10, FR-SHELL-009, D-52, `07` §2; Microsoft Learn — WebView2
//! `TrySuspend`, `01c` §12): the moment the window is hidden its WebView is made invisible and
//! suspended (the renderer sleeps — timers and animations stop); after 60 s in the tray the main
//! window with its WebView is destroyed — the core, the tray, hotkeys, SMTC and the taskbar keep
//! working. «Открыть» builds it again from the config and shows it at once (the dark background
//! is the skeleton while the page loads). «Держать окно в памяти» (`window.keep_in_memory`)
//! keeps it suspended instead.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Manager, WebviewWindowBuilder};

use crate::settings_store::SettingsStore;

/// Bumped on every hide/show: a timer of an older hide does nothing.
static GEN: AtomicU64 = AtomicU64::new(0);
const DESTROY_AFTER: Duration = Duration::from_secs(60);

fn keep(app: &AppHandle) -> bool {
    app.try_state::<Arc<SettingsStore>>()
        .is_some_and(|s| s.get()["window"]["keep_in_memory"].as_bool() == Some(true))
}

/// Test seam: a shorter delay, only for an isolated instance.
fn delay() -> Duration {
    if std::env::var_os("SIRTONE_DATA_DIR").is_some()
        && let Some(ms) = std::env::var("SIRTONE_SMOKE_TRAY_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
    {
        return Duration::from_millis(ms);
    }
    DESTROY_AFTER
}

fn suspend(app: &AppHandle) {
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    let _ = w.with_webview(|wv| {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_3;
        use webview2_com::TrySuspendCompletedHandler;
        use windows::core::Interface;
        let c = wv.controller();
        // SAFETY: WebView2 calls on the UI thread (`with_webview` runs there).
        unsafe {
            let _ = c.SetIsVisible(false);
            if let Ok(core) = c.CoreWebView2()
                && let Ok(core3) = core.cast::<ICoreWebView2_3>()
            {
                let h = TrySuspendCompletedHandler::create(Box::new(|_, _| Ok(())));
                let _ = core3.TrySuspend(&h);
            }
        }
    });
}

/// The main window was hidden to the tray.
pub fn hidden(app: &AppHandle) {
    suspend(app);
    let g = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    if keep(app) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(delay());
        if GEN.load(Ordering::SeqCst) != g {
            return;
        }
        let a = app.clone();
        let _ = app.run_on_main_thread(move || {
            if GEN.load(Ordering::SeqCst) != g {
                return;
            }
            if let Some(w) = a.get_webview_window("main")
                && !w.is_visible().unwrap_or(true)
            {
                tracing::info!(target: "sirtone::memory", "main window destroyed in the tray");
                let _ = w.destroy();
            }
        });
    });
}

/// The main window is coming back: an old timer is void; built again if it was destroyed.
/// `true` — it exists now.
pub fn ensure_main(app: &AppHandle) -> bool {
    GEN.fetch_add(1, Ordering::SeqCst);
    if app.get_webview_window("main").is_some() {
        return true;
    }
    let Some(cfg) = app.config().app.windows.iter().find(|w| w.label == "main") else {
        return false;
    };
    match WebviewWindowBuilder::from_config(app, cfg).and_then(|b| b.build()) {
        Ok(w) => {
            crate::window_chrome::apply_os_theme(&w);
            crate::taskbar::start(app);
            tracing::info!(target: "sirtone::memory", "main window built again");
            true
        }
        Err(e) => {
            tracing::error!(target: "sirtone::memory", error = %e, "main window rebuild failed");
            false
        }
    }
}
