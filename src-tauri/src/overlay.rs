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
///
/// IMPORTANT: the label ("spotlight") is Tauri-internal. The WM class the
/// compositor sees derives from the Tauri identifier
/// (`com.sebo.architects-overlay` → window class `architects-overlay`) —
/// compositor-side focus dispatches MUST use WINDOW_CLASS, not this label,
/// or the dispatch fails with "window not found" (classic bug: focus stuck
/// in the game while the overlay was open).
pub const SPOTLIGHT_LABEL: &str = "spotlight";

/// WM window class Hyprland et al. see for the spotlight window. NOT derived
/// from `tauri.conf.json` → `identifier` (a common misconception): tao creates
/// the window via GTK, and GDK takes the WM_CLASS class from the program
/// name, capitalizing its first letter (`architects-overlay` →
/// `Architects-overlay`) unless prgname/program-class are set explicitly.
/// `lib.rs::pin_wm_class` (called from `setup`, before the first map) pins
/// it to exactly this constant, so Hyprland rules
/// (`class:^(architects-overlay)$`) and `focuswindow class:…` match — both
/// are case-sensitive (root cause: card t_68c09e76).
pub const SPOTLIGHT_WM_CLASS: &str = "architects-overlay";

/// Fallback panel size in logical px, used only as the initial `inner_size`
/// when the primary monitor can't be queried. NOT a post-map size assertion —
/// since the layout fix the single source of window geometry is
/// `place_on_pointer_monitor` (full monitor); a post-map 660×600 re-assert
/// used to shrink the compositor-sized overlay back down (review B2 lesson).
const PANEL_W: f64 = 660.;
const PANEL_H: f64 = 600.;

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
    /// Whether the config view was opened from the tray ("Config öffnen").
    /// Keeps the window visible even without a running game — the config must
    /// be reachable at any time, independent of `game_running`. Cleared when
    /// the frontend reports the config view closed (`overlay_config_closed`).
    pub config_open: AtomicBool,
    /// Whether the window ignores cursor events (click-through). `true` =
    /// the game keeps the mouse; `false` = focus mode (the overlay is clickable).
    pub click_through: AtomicBool,
}

impl OverlayState {
    pub fn new() -> Self {
        Self {
            user_visible: AtomicBool::new(true),
            game_running: AtomicBool::new(false),
            config_open: AtomicBool::new(false),
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

/// Create the borderless, always-on-top, taskbar-skipped spotlight window.
/// Hidden until the game is detected. Initial size = full primary monitor:
/// on Wayland the client's own geometry (set at map time) is what actually
/// sticks — compositor windowrule size effects were silently overridden by
/// the client's initial 660×600 geometry, and post-map resizes don't work on
/// Wayland at all. The CSS floats the compact palette inside this window.
fn create_spotlight_window<R: Runtime>(app: &tauri::App<R>) -> tauri::Result<WebviewWindow<R>> {
    // Best-effort monitor size for the initial map. `resizable(false)` keeps
    // users from accidentally shrinking the overlay; the window is never
    // meant to be resized by hand.
    let (w, h) = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| (m.size().width as f64, m.size().height as f64))
        .unwrap_or((PANEL_W, PANEL_H));

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
    // MUST stay resizable: a non-resizable xdg_toplevel advertises fixed
    // min=max geometry, and neither compositor windowrules nor client resizes
    // can then grow the window to full-monitor overlay size. Nobody resizes
    // it by hand anyway (borderless, no decorations).
    .resizable(true)
    .inner_size(w, h)
    .visible(false) // shown when Star Citizen is detected
    .build()?;

    // Every *show* re-places the window on the monitor under the pointer —
    // the multi-monitor behaviour — see `place_on_pointer_monitor`.
    win.center()?;

    Ok(win)
}

/// Register the global shortcuts. The `global-shortcut` backend is X11-based
/// (global_hotkey has no Wayland implementation), so on a pure Wayland session
/// the plugin fails to initialise; we swallow that — the overlay still works,
/// only the global hotkey is unavailable there. The Wayland fallback is the
/// Unix-socket CLI (`arch-overlay toggle`, see `ipc_socket` + docs/WAYLAND.md),
/// which compositor keybinds exec directly.
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
                // Switching INTO focus mode must also move keyboard focus to
                // the overlay — under Hyprland/Wayland the game would other-
                // wise keep swallowing every keystroke (practical report).
                if !ignore {
                    compositor_focus(SPOTLIGHT_WM_CLASS);
                } else {
                    compositor_focus("starcitizen.exe");
                }
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

/// Tauri event the frontend listens to when the config view was requested
/// from the tray ("Config öffnen") — the JS side opens the pane-config view
/// in response, exactly like the in-overlay gear button.
pub const TRAY_OPEN_CONFIG_EVENT: &str = "tray-open-config";

/// Tray menu action "Config öffnen": show the overlay window (also without a
/// running game — the config is reachable at any time) and ask the frontend
/// to open the config view.
pub fn open_config_from_tray<R: Runtime>(app: &AppHandle<R>) {
    if let Some(st) = app.try_state::<OverlayState>() {
        st.config_open.store(true, Ordering::Release);
        apply_visibility(app, effective_visible(st.inner()));
        emit_state(app);
        let _ = app.emit(TRAY_OPEN_CONFIG_EVENT, ());
    }
}

/// IPC command called by the frontend when the config view closes (Esc,
/// selection, close button). Only re-applies visibility when the window was
/// shown *solely* for the config (no game running) — otherwise the normal
/// `user_visible && game_running` semantics already apply and a hide here
/// would fight the poller/hotkey state.
pub fn close_config<R: Runtime>(app: &AppHandle<R>) {
    if let Some(st) = app.try_state::<OverlayState>() {
        if st.config_open.swap(false, Ordering::AcqRel) {
            apply_visibility(app, effective_visible(st.inner()));
            emit_state(app);
        }
    }
}

/// Effective window visibility: the usual user intent × game condition, OR
/// the tray-requested config view (independent of the game).
fn effective_visible(st: &OverlayState) -> bool {
    st.config_open.load(Ordering::SeqCst)
        || (st.user_visible.load(Ordering::SeqCst) && st.game_running.load(Ordering::SeqCst))
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
            apply_visibility(app, effective_visible(st.inner()));
            emit_state(app);
        }
    }
}

/// Read a snapshot for the IPC state command.
pub fn snapshot<R: Runtime>(app: &AppHandle<R>) -> Option<OverlaySnapshot> {
    app.try_state::<OverlayState>().map(|st| OverlaySnapshot {
        visible: effective_visible(st.inner()),
        game_running: st.game_running.load(Ordering::SeqCst),
        click_through: st.click_through.load(Ordering::SeqCst),
    })
}

/// IPC command: the frontend closed the config view (see `close_config`).
#[cfg(desktop)]
#[tauri::command]
pub fn overlay_config_closed<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    close_config(&app);
}

/// Apply the effective visibility to the actual window, on the main thread.
fn apply_visibility<R: Runtime>(app: &AppHandle<R>, visible: bool) {
    let receiver = app.clone();
    let worker = app.clone();
    let _ = receiver.run_on_main_thread(move || {
        if let Some(win) = worker.get_webview_window(SPOTLIGHT_LABEL) {
            if visible {
                let _ = win.show();
                // NOTE: no panel-size re-assert here. Under Hyprland the
                // client's post-map resize request WINS over the compositor's
                // `maximize` windowrule, which silently shrank the full-screen
                // overlay back to the 660×600 panel (practical report 1/3).
                // The single source of window geometry is
                // `place_on_pointer_monitor` below (full monitor, Arkanis-
                // style) — the CSS keeps the visible panel compact inside the
                // transparent full-screen window.
                // The GTK window is only realized after the first map; shape
                // the (hidden-at-boot) window only now, when click-through is
                // safe to apply. On Wayland the compositor may ignore the
                // position hint — documented limitation.
                if let Some(st) = worker.try_state::<OverlayState>() {
                    let ignore = st.click_through.load(Ordering::SeqCst);
                    let _ = win.set_ignore_cursor_events(ignore);
                }
                // Multi-monitor: span the full monitor under the pointer.
                place_on_pointer_monitor(&win);
                // Take keyboard focus so the search is immediately typable —
                // compositors do not move keyboard focus to a newly shown
                // window, and the game would keep swallowing keystrokes.
                // Retry in a thread: the Wayland map is ASYNC — right after
                // show() the compositor does not know the window yet
                // ("hl.focus: window not found"), so the immediate attempt
                // always missed (verified via compositor_focus logging).
                let label = SPOTLIGHT_WM_CLASS.to_string();
                std::thread::spawn(move || {
                    for wait in [300u64, 600, 1200] {
                        std::thread::sleep(Duration::from_millis(wait));
                        compositor_focus(&label);
                    }
                });
            } else {
                let _ = win.hide();
                // Hand keyboard focus back to the game.
                compositor_focus("starcitizen.exe");
            }
        }
    });
}

/// Hyprland's own global cursor position via `hyprctl -j cursorpos` — in the
/// compositor's logical layout pixels. This is exactly the coordinate space in
/// which `available_monitors()` reports its rectangles on Wayland (verified
/// live against `hyprctl monitors` output, t_317b9d15). Returns `None` when
/// hyprctl is unavailable (not Hyprland, Lua-bridge rejects the keyword, …).
#[cfg(target_os = "linux")]
fn hyprland_cursor_position() -> Option<(i32, i32)> {
    let out = std::process::Command::new("hyprctl")
        .args(["-j", "cursorpos"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let x = v.get("x")?.as_f64()?;
    let y = v.get("y")?.as_f64()?;
    Some((x.round() as i32, y.round() as i32))
}

/// Current global pointer position + source label, in the coordinate space of
/// the monitor rectangles `available_monitors()` will report on this session
/// (see the per-branch notes). `None` means "unknown" — e.g. a pure Wayland
/// session without XWayland or Hyprland, or macOS, where the caller falls back
/// to centering on the current display.
fn cursor_position() -> Option<(i32, i32, &'static str)> {
    #[cfg(target_os = "linux")]
    {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::ConnectionExt;
        // Hyprland: ask the compositor itself. The X11 answer is NOT usable
        // here: XWayland's canvas is its own left-to-right arrangement by
        // output creation order that ignores the Hyprland layout entirely
        // (measured t_317b9d15: HDMI-A-2 lives at layout [1825,-1920] with
        // transform 3, but on the X11 canvas it is `1080x1920+3440+0`; a
        // cursor at layout (1720,720) on DP-3 read as X11 (3440,1919)).
        // Worse, the X11 pointer only updates when the cursor crosses X
        // client surfaces — Wayland-side motion leaves it stale. Containment
        // of X11-space coords against layout-space rects therefore misses
        // (or randomly hits) monitors whenever monitors are offset or
        // rotated. The cursorpos answer is exact and needs no transform.
        if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
            if let Some((x, y)) = hyprland_cursor_position() {
                return Some((x, y, "hyprland cursorpos"));
            }
        }
        // Non-Hyprland: X11/XWayland query. On a real X11 session the tauri
        // monitor rects come from the same X screen, so the spaces agree.
        // Pure Wayland without Hyprland (no DISPLAY): no global pointer query
        // without a compositor protocol extension (wlr_foreign_toplevel).
        std::env::var_os("DISPLAY")?;
        let (conn, screen_num) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots[screen_num].root;
        let reply = conn.query_pointer(root).ok()?.reply().ok()?;
        Some((
            i32::from(reply.root_x),
            i32::from(reply.root_y),
            "x11 query_pointer",
        ))
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut pt = POINT { x: 0, y: 0 };
        // SAFETY: `pt` is a valid, writable POINT on our stack and GetCursorPos
        // writes only into it. Windows-only FFI call, feature-gated.
        if unsafe { GetCursorPos(&mut pt) } != 0 {
            return Some((pt.x, pt.y, "win32 GetCursorPos"));
        }
        return None;
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = (); // macOS, other Unix: deliberately no pointer query.
        return None;
    }
}

/// Half-open rect containment `x in [x0, x0+w)`, `y in [y0, y0+h)` — pure so
/// it can be unit-tested against the measured Sebo layout (DP-3 `[0,0]
/// 3440x1440`, HDMI-A-2 `[1825,-1920] 1080x1920` transform 3). Both operands
/// are already in the same space by construction: on Hyprland the cursor comes
/// from `hyprctl cursorpos` and the rects from wl_output (identical layout
/// space); on X11 sessions both come from the X screen (t_317b9d15).
fn contains_point(rect: (i32, i32, u32, u32), cursor: (i32, i32)) -> bool {
    let (x0, y0, w, h) = rect;
    cursor.0 >= x0 && cursor.0 < x0 + w as i32 && cursor.1 >= y0 && cursor.1 < y0 + h as i32
}

/// Center the window on the monitor that currently contains the pointer.
///
/// Falls back to the primary monitor, then to plain `center()`, whenever the
/// pointer position is unknown or no monitor contains it. Physical-coordinate
/// math throughout (monitor position/size are physical; so is the pointer).
fn place_on_pointer_monitor<R: Runtime>(win: &WebviewWindow<R>) {
    #[cfg(debug_assertions)]
    log::info!("place: cursor_position()={:?}", cursor_position());
    // Pointer query is best-effort (X11 needs DISPLAY+XAUTHORITY; a bare
    // Wayland session has no global pointer query). Without it we still span
    // the FULL monitor the window currently lives on — falling back to plain
    // centering (the old 660×600 panel behaviour) must never happen again.
    let target = match cursor_position() {
        Some((cx, cy, src)) => {
            let mut chosen = None;
            if let Ok(monitors) = win.available_monitors() {
                chosen = monitors.into_iter().find(|mon| {
                    let pos = mon.position();
                    let size = mon.size();
                    contains_point((pos.x, pos.y, size.width, size.height), (cx, cy))
                });
            }
            if chosen.is_none() {
                log::info!(
                    "place: cursor=({cx},{cy}) src={src} contained by no monitor -> primary fallback"
                );
                chosen = win.primary_monitor().ok().flatten();
            }
            if let Some(mon) = &chosen {
                let pos = mon.position();
                let size = mon.size();
                log::info!(
                    "place: cursor=({cx},{cy}) src={src} -> monitor {:?} {}x{}@{},{}",
                    mon.name(),
                    size.width,
                    size.height,
                    pos.x,
                    pos.y
                );
            }
            chosen
        }
        None => {
            log::info!("place: cursor unknown -> current monitor");
            win.current_monitor().ok().flatten()
        }
    };

    let Some(mon) = target else {
        // Truly nothing known (mock runtime in tests): the compositor's
        // windowrules (Wayland) or the initial inner_size decide.
        return;
    };

    let mpos = mon.position();
    let msize = mon.size();
    // Arkanis-style: the overlay uses the WHOLE monitor — the transparent
    // window spans the screen, the palette UI floats inside it (CSS keeps the
    // visible shell compact). This also makes the click/keyboard surface
    // predictable across monitor layouts.
    let _ = win.set_size(tauri::PhysicalSize::new(msize.width, msize.height));
    let _ = win.set_position(tauri::PhysicalPosition::new(mpos.x, mpos.y));
}

/// Best-effort keyboard-focus management for the Wayland/Hyprland case:
/// compositors do not transfer keyboard focus to a newly shown overlay window
/// on their own, and the game keeps grabbing keystrokes. We shell out to
/// `hyprctl` (present on the target desktop; a no-op everywhere else):
/// show → focus the spotlight so search typing works, hide → hand focus back
/// to the game.
fn compositor_focus(class: &str) {
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
            return;
        }
        // Omarchy's Hyprland wraps hyprctl in a Lua bridge: the raw two-arg
        // form (`dispatch focuswindow class:X`) fails to parse there and
        // silently does nothing. The bridge-native form
        // `hl.dsp.focus({ window = "class:X" })` is the reliable one
        // (verified live); the plain form stays as fallback for non-Omarchy
        // Hyprland installs.
        let lua = format!("hl.dsp.focus({{ window = \"class:{class}\" }})");
        let out = std::process::Command::new("hyprctl")
            .args(["dispatch", &lua])
            .output();
        let ok = match &out {
            Ok(o) => {
                log::info!(
                    "compositor_focus({class}): status={:?} out={:?}",
                    o.status.code(),
                    String::from_utf8_lossy(&o.stdout).trim()
                );
                o.status.success()
            }
            Err(e) => {
                log::info!("compositor_focus({class}): spawn failed: {e}");
                false
            }
        };
        if !ok {
            let _ = std::process::Command::new("hyprctl")
                .args(["dispatch", "focuswindow", &format!("class:{class}")])
                .status();
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = class;
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

    /// Regression fixtures for the monitor placement bug (t_317b9d15): the
    /// rects are the REAL wl_output boxes measured on Sebo's Hyprland —
    /// DP-3 `[0,0] 3440x1440` and the rotated HDMI-A-2 `[1825,-1920]
    /// 1080x1920` (transform 3, hence the swapped logical size and the
    /// negative y). The cursor values are the matching `hyprctl cursorpos`
    /// probes from the same live session.
    #[test]
    fn contains_point_matches_measured_hyprland_layout() {
        let dp3 = (0, 0, 3440, 1440);
        let hdmi = (1825, -1920, 1080, 1920);

        // Pointer warps from the live probe matrix: each must hit its monitor.
        assert!(contains_point(dp3, (1720, 720))); // DP-3 center
        assert!(contains_point(dp3, (100, 50))); // DP-3 top-left
        assert!(contains_point(dp3, (3430, 1430))); // DP-3 bottom-right
        assert!(contains_point(hdmi, (2365, -960))); // HDMI-A-2 center
        assert!(contains_point(hdmi, (1900, -1850))); // HDMI-A-2 top-left
        assert!(contains_point(hdmi, (1825, -1920))); // HDMI-A-2 exact corner
        assert!(contains_point(hdmi, (2904, -1))); // HDMI-A-2 bottom-right edge

        // Half-open rects: shared edges belong to exactly one monitor.
        assert!(!contains_point(hdmi, (2905, -1))); // one past right edge
        assert!(!contains_point(dp3, (3440, 0))); // DP-3 width edge exclusive
        assert!(!contains_point(hdmi, (2365, 0))); // HDMI height edge exclusive
        // The stale X11-canvas reading from the bug report is no coordinate a
        // cursor can legitimately have in layout space on this layout.
        assert!(!contains_point(dp3, (3440, 1919)));
        assert!(!contains_point(hdmi, (3440, 1919)));
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
