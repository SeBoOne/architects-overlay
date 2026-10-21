//! # Overlay window scaffold
//!
//! The **Spotlight** is a separate, borderless, always-on-top, taskbar-skipped
//! window that can be toggled with a global shortcut while Star Citizen is
//! running, and which supports a click-through (input-transparent) mode so the
//! game keeps receiving the mouse. This module owns:
//!
//! - creating the spotlight window at startup (hidden, click-through by default),
//! - the visibility state machine (`user_visible && game_running`),
//! - click-through toggling (`set_ignore_cursor_events`),
//! - multi-monitor placement (centered on the monitor under the pointer),
//! - the process-detection poller that shows/hides the window with the game.
//!
//! Everything that touches the window runs on the Tauri main thread via
//! `app.run_on_main_thread` — the hotkey handler thread, the game poller thread
//! and the IPC commands all funnel through it.
//!
//! No telemetry: nothing is logged or transmitted.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::webview::WebviewWindow;
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, ShortcutEvent, ShortcutState};

/// Window label of the spotlight overlay. Referenced from capabilities and the
/// frontend (`getCurrentWebviewWindow().getLabel()`).
pub const SPOTLIGHT_LABEL: &str = "spotlight";

/// Tauri event the frontend listens to for live overlay state updates
/// (game started/stopped, visibility toggled, click-through toggled).
pub const OVERLAY_STATE_EVENT: &str = "overlay-state-changed";

/// Shared, thread-safe runtime state of the overlay window. Managed as a single
/// Tauri state value so the hotkey handler, the game poller and the IPC
/// commands all observe the same flags.
pub struct OverlayState {
    /// Whether the user wants the overlay shown (flipped by the toggle hotkey).
    /// Starts `true`; the window is only *actually* visible when this AND
    /// `game_running` are both set.
    pub user_visible: AtomicBool,
    /// Whether Star Citizen is currently running (fed by the poller).
    pub game_running: AtomicBool,
    /// Whether the window ignores cursor events (click-through). `true` =
    /// the game keeps the mouse; `false` = focus mode (the overlay is clickable).
    pub click_through: AtomicBool,
}

impl OverlayState {
    pub fn new() -> Self {
        Self {
            user_visible: AtomicBool::new(true),
            game_running: AtomicBool::new(false),
            // Click-through by default: at startup it must not steal input from
            // the game until the user explicitly toggles into focus mode.
            click_through: AtomicBool::new(true),
        }
    }
}

/// Snapshot of the overlay state, serialized to the frontend via IPC.
#[derive(Clone, serde::Serialize)]
pub struct OverlaySnapshot {
    pub visible: bool,
    pub game_running: bool,
    pub click_through: bool,
}

/// Create the spotlight window and start everything (state, hotkey, poller).
/// Must be called from the app setup (main thread).
pub fn init<R: Runtime>(app: &tauri::App<R>) -> tauri::Result<()> {
    app.manage(OverlayState::new());

    create_spotlight_window(app)?;
    // NOTE: click-through is applied on first *show* (see `apply_visibility`):
    // tao panics if set_ignore_cursor_events is dispatched before the GTK
    // window is realized (input-shape on a non-mapped window), so it must
    // never be called while the window is still hidden at startup.

    // Smoke-test / CI hook: `ARCH_OVERLAY_SMOKE=1` (debug builds only) shows
    // the spotlight window at startup without waiting for Star Citizen, so UI
    // states can be screenshotted end-to-end. No effect in normal use.
    #[cfg(debug_assertions)]
    if std::env::var_os("ARCH_OVERLAY_SMOKE").is_some() {
        log::info!("smoke mode: showing spotlight window at startup");
        apply_visibility(app.handle(), true);
    }

    register_global_shortcuts(app);

    let handle = app.handle();
    spawn_game_poller(handle);

    Ok(())
}

/// Create the borderless, always-on-top, taskbar-skipped spotlight window,
/// centered on its (primary) monitor. Hidden until the game is detected.
fn create_spotlight_window<R: Runtime>(app: &tauri::App<R>) -> tauri::Result<WebviewWindow<R>> {
    let win = WebviewWindowBuilder::new(
        app,
        SPOTLIGHT_LABEL,
        // Same SvelteKit frontend as the main window; it renders the compact
        // overlay view when it detects it is running in this window.
        WebviewUrl::App("index.html".into()),
    )
    .title("SC Architects Spotlight")
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .inner_size(660., 600.)
    .visible(false) // shown when Star Citizen is detected
    .build()?;

    // Initial placement: center on the window's display (the primary monitor).
    // Every *show* re-places the window on the monitor under the pointer —
    // the multi-monitor behaviour — see `place_on_pointer_monitor`.
    win.center()?;

    Ok(win)
}

/// Register the global shortcuts. The `global-shortcut` backend is X11-based
/// (global_hotkey has no Wayland implementation), so on a pure Wayland session
/// the plugin fails to initialise; we swallow that — the overlay still works,
/// only the global hotkey is unavailable there.
/// TODO(wayland): wire the toggle through the compositor (e.g. a Hyprland
/// binding) or a tray/CLI entry point, when Sebo's Wayland setup needs it.
fn register_global_shortcuts<R: Runtime>(app: &tauri::App<R>) {
    // Strg+Shift+U toggles visibility, Strg+Shift+O toggles click-through.
    // Hard-coded for Phase 1 — both become configurable via `settings`.
    let shortcut_visible = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyU);
    let shortcut_click = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyO);

    let mut builder = tauri_plugin_global_shortcut::Builder::new();
    builder = builder.with_shortcut(shortcut_visible).unwrap();
    builder = builder.with_shortcut(shortcut_click).unwrap();
    let plugin = builder
        .with_handler(
            move |open_app: &AppHandle<R>, shortcut: &Shortcut, event: ShortcutEvent| {
                if event.state != ShortcutState::Pressed {
                    return;
                }
                if shortcut == &shortcut_visible {
                    toggle_visible(open_app);
                } else if shortcut == &shortcut_click {
                    toggle_click_through(open_app);
                }
            },
        )
        .build();

    // Failure (e.g. no X11 on Wayland) must not abort startup — but it must
    // be visible: a silently missing hotkey looks like a dead overlay. The
    // socket CLI (`ipc_socket`) is the Wayland fallback.
    if let Err(err) = app.handle().plugin(plugin) {
        log::warn!(
            "global shortcuts unavailable ({err}); under Wayland use the socket CLI: \
             arch-overlay toggle (see docs/WAYLAND.md)"
        );
    }
}

/// Toggle spotlight visibility. Only meaningful while the game runs (the toggle
/// is a no-op without a game, so a stray hotkey press can't silently disable
/// the overlay for the next game session). Effective state:
/// `user_visible && game_running`.
pub fn toggle_visible<R: Runtime>(app: &AppHandle<R>) {
    if let Some(st) = app.try_state::<OverlayState>() {
        // The overlay is a game companion: without Star Citizen there is
        // nothing to toggle, so leave the user intent untouched.
        if !st.game_running.load(Ordering::SeqCst) {
            return;
        }
        let next = !st.user_visible.load(Ordering::SeqCst);
        st.user_visible.store(next, Ordering::Release);
        apply_visibility(app, next);
        emit_state(app);
    }
}

/// Toggle between click-through (input passes to the game) and focus mode
/// (overlay is clickable). Returns the new click-through value.
pub fn toggle_click_through<R: Runtime>(app: &AppHandle<R>) -> bool {
    let next = match app.try_state::<OverlayState>() {
        Some(st) => !st.click_through.load(Ordering::SeqCst),
        None => true,
    };
    set_click_through(app, next);
    next
}

/// Set click-through explicitly (also used by the IPC command).
pub fn set_click_through<R: Runtime>(app: &AppHandle<R>, ignore: bool) -> bool {
    if let Some(st) = app.try_state::<OverlayState>() {
        st.click_through.store(ignore, Ordering::Release);
    } else {
        return false;
    }
    emit_state(app);
    let receiver = app.clone();
    let worker = app.clone();
    let _ = receiver.run_on_main_thread(move || {
        if let Some(win) = worker.get_webview_window(SPOTLIGHT_LABEL) {
            // Only shape input while the window is actually mapped: on GTK,
            // `set_ignore_cursor_events` before the first map panics, and a
            // hidden window has no input shape anyway. When hidden, the flag
            // rides in `OverlayState` and is applied by `apply_visibility`
            // on the next show — the hotkey stays active in either state.
            if win.is_visible().unwrap_or(false) {
                let _ = win.set_ignore_cursor_events(ignore);
            }
        }
    });
    true
}

/// Broadcast the current overlay state to the frontend so the spotlight UI
/// stays live (status dot, ready text, click-through button) without polling.
fn emit_state<R: Runtime>(app: &AppHandle<R>) {
    if let Some(snap) = snapshot(app) {
        let _ = app.emit(OVERLAY_STATE_EVENT, snap);
    }
}

/// Feed the current game-running status into the state and show/hide the
/// window only when it actually changes.
pub fn set_game_running<R: Runtime>(app: &AppHandle<R>, running: bool) {
    if let Some(st) = app.try_state::<OverlayState>() {
        let was = st.game_running.swap(running, Ordering::Release);
        if was != running {
            // Smoke mode (ARCH_OVERLAY_SMOKE=1, debug builds only) keeps the
            // overlay visible for screenshotting even when the game flaps in
            // and out of /proc — the test must not fight the poller.
            #[cfg(debug_assertions)]
            if !running && std::env::var_os("ARCH_OVERLAY_SMOKE").is_some() {
                return;
            }
            apply_visibility(app, st.user_visible.load(Ordering::SeqCst) && running);
            emit_state(app);
        }
    }
}

/// Read a snapshot for the IPC state command.
pub fn snapshot<R: Runtime>(app: &AppHandle<R>) -> Option<OverlaySnapshot> {
    app.try_state::<OverlayState>().map(|st| OverlaySnapshot {
        visible: st.user_visible.load(Ordering::SeqCst) && st.game_running.load(Ordering::SeqCst),
        game_running: st.game_running.load(Ordering::SeqCst),
        click_through: st.click_through.load(Ordering::SeqCst),
    })
}

/// Apply the effective visibility to the actual window, on the main thread.
fn apply_visibility<R: Runtime>(app: &AppHandle<R>, visible: bool) {
    let receiver = app.clone();
    let worker = app.clone();
    let _ = receiver.run_on_main_thread(move || {
        if let Some(win) = worker.get_webview_window(SPOTLIGHT_LABEL) {
            if visible {
                let _ = win.show();
                // The GTK window is only realized after the first map; shape
                // the (hidden-at-boot) window only now, when click-through is
                // safe to apply. On Wayland the compositor may ignore the
                // position hint — documented limitation.
                if let Some(st) = worker.try_state::<OverlayState>() {
                    let ignore = st.click_through.load(Ordering::SeqCst);
                    let _ = win.set_ignore_cursor_events(ignore);
                }
                // Multi-monitor: land centered on the monitor under the
                // pointer (falling back to the primary display).
                place_on_pointer_monitor(&win);
            } else {
                let _ = win.hide();
            }
        }
    });
}

/// Current global pointer position in physical pixels, when the platform can
/// answer (X11/Wayland-XWayland and Windows). `None` means "unknown" — e.g. a
/// pure Wayland session without XWayland, or macOS, where the caller falls
/// back to centering on the primary display.
fn cursor_position() -> Option<(i32, i32)> {
    #[cfg(target_os = "linux")]
    {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::ConnectionExt;
        // No XWayland (pure Wayland): there is no global pointer query without
        // a compositor protocol extension (wlr_foreign_toplevel / zwlr_*).
        std::env::var_os("DISPLAY")?;
        let (conn, screen_num) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots[screen_num].root;
        let reply = conn.query_pointer(root).ok()?.reply().ok()?;
        Some((i32::from(reply.root_x), i32::from(reply.root_y)))
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut pt = POINT { x: 0, y: 0 };
        // SAFETY: `pt` is a valid, writable POINT on our stack and GetCursorPos
        // writes only into it. Windows-only FFI call, feature-gated.
        if unsafe { GetCursorPos(&mut pt) } != 0 {
            return Some((pt.x, pt.y));
        }
        return None;
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = (); // macOS, other Unix: deliberately no pointer query.
        return None;
    }
}

/// Center the window on the monitor that currently contains the pointer.
///
/// Falls back to the primary monitor, then to plain `center()`, whenever the
/// pointer position is unknown or no monitor contains it. Physical-coordinate
/// math throughout (monitor position/size are physical; so is the pointer).
fn place_on_pointer_monitor<R: Runtime>(win: &WebviewWindow<R>) {
    let Some((cx, cy)) = cursor_position() else {
        let _ = win.center();
        return;
    };

    let mut target = None;
    if let Ok(monitors) = win.available_monitors() {
        for mon in monitors {
            let pos = mon.position();
            let size = mon.size();
            let contains = cx >= pos.x
                && cx < pos.x + size.width as i32
                && cy >= pos.y
                && cy < pos.y + size.height as i32;
            if contains {
                target = Some(mon);
                break;
            }
        }
    }
    let target = match target {
        Some(mon) => Some(mon),
        // Pointer on no monitor (strange) or not queryable → primary display.
        None => win.primary_monitor().ok().flatten(),
    };
    let Some(mon) = target else {
        let _ = win.center();
        return;
    };

    let Ok(ws) = win.outer_size() else {
        let _ = win.center();
        return;
    };
    let mpos = mon.position();
    let msize = mon.size();
    let x = mpos.x + (msize.width as i32 - ws.width as i32) / 2;
    let y = mpos.y + (msize.height as i32 - ws.height as i32) / 2;
    // Monitors may sit at negative global coordinates (e.g. a display above
    // the primary): these are valid global physical positions — clamping to 0
    // would pull the window onto the wrong monitor.
    let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
}

/// Background poller: every `POLL_INTERVAL_MS` re-checks whether Star Citizen
/// runs and updates the overlay accordingly. Detached — dies with the process,
/// so it needs no explicit shutdown signalling. Reading /proc on each tick is
/// cheap relative to a 2 s cadence, and only state *changes* touch the window.
const POLL_INTERVAL_MS: u64 = 2000;

fn spawn_game_poller<R: Runtime>(handle: &AppHandle<R>) {
    let app = handle.clone();
    std::thread::spawn(move || {
        loop {
            let running = game_detection::is_game_running();
            set_game_running(&app, running);
            std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
        }
    });
}

// ---------------------------------------------------------------------------
// IPC commands the frontend can call (e.g. to build a click-through toggle, or
// to reflect the state in a status line).
// ---------------------------------------------------------------------------

/// Current overlay state, for the frontend status UI.
#[tauri::command]
pub fn overlay_get_state(app: tauri::AppHandle) -> OverlaySnapshot {
    snapshot(&app).unwrap_or(OverlaySnapshot {
        visible: false,
        game_running: false,
        click_through: true,
    })
}

/// Set click-through (focus vs. input-transparent) from the frontend.
#[tauri::command]
pub fn overlay_set_click_through(app: tauri::AppHandle, ignore: bool) -> bool {
    set_click_through(&app, ignore)
}

/// Set overlay visibility from the frontend (Esc closes the palette). The
/// effective visibility stays gated on the game running, exactly like the
/// hotkey toggle.
#[tauri::command]
pub fn overlay_set_visible(app: tauri::AppHandle, visible: bool) -> bool {
    if let Some(st) = app.try_state::<OverlayState>() {
        st.user_visible.store(visible, Ordering::Release);
        let effective = visible && st.game_running.load(Ordering::SeqCst);
        apply_visibility(&app, effective);
        emit_state(&app);
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Manager;
    use tauri::test::mock_app;

    fn managed_app() -> tauri::App<tauri::test::MockRuntime> {
        let app = mock_app();
        app.manage(OverlayState::new());
        app
    }

    /// The overlay shows exactly when the user wants it AND the game runs:
    /// `user_visible && game_running`.
    #[test]
    fn visibility_follows_game_state() {
        let app = managed_app();
        let h = app.handle();
        // No game yet → hidden although the user wants it on.
        assert!(!snapshot(h).unwrap().visible);
        set_game_running(h, true);
        assert!(snapshot(h).unwrap().visible);
        set_game_running(h, false);
        assert!(!snapshot(h).unwrap().visible);
    }

    /// Hotkey toggle only takes effect while Star Citizen runs; a manual "off"
    /// survives game restarts until toggled back on.
    #[test]
    fn toggle_requires_running_game() {
        let app = managed_app();
        let h = app.handle();
        toggle_visible(h); // no game → stays hidden
        assert!(!snapshot(h).unwrap().visible);

        set_game_running(h, true);
        assert!(snapshot(h).unwrap().visible);

        toggle_visible(h); // user turns the overlay off
        assert!(!snapshot(h).unwrap().visible);

        // Game restarts: still off (user intent preserved).
        set_game_running(h, false);
        set_game_running(h, true);
        assert!(!snapshot(h).unwrap().visible);

        toggle_visible(h); // back on
        assert!(snapshot(h).unwrap().visible);
    }

    /// Click-through starts enabled and toggling via either path persists.
    #[test]
    fn click_through_toggles_and_persists() {
        let app = managed_app();
        let h = app.handle();
        assert!(snapshot(h).unwrap().click_through);

        // Toggling off (focus mode) flips the state.
        assert!(!toggle_click_through(h));
        assert!(!snapshot(h).unwrap().click_through);

        // Explicit set on again (click-through) persists.
        assert!(set_click_through(h, true));
        assert!(snapshot(h).unwrap().click_through);
    }

    /// A game-state change that does not alter the result triggers nothing
    /// (idempotent set — the window only gets touched on transitions).
    #[test]
    fn game_state_changes_are_idempotent() {
        let app = managed_app();
        let h = app.handle();
        set_game_running(h, true);
        assert!(snapshot(h).unwrap().visible);
        set_game_running(h, true); // no change → state untouched
        assert!(snapshot(h).unwrap().visible);
    }
}
