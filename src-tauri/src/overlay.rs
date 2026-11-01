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

use crate::i18n_err::{err_key, err_keyp};
use serde_json::json;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::webview::WebviewWindow;
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

use tauri_plugin_global_shortcut::{GlobalShortcut, Shortcut, ShortcutEvent, ShortcutState};

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

/// Which global shortcut slot a combo drives (C2a, t_6565fe8b).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutKind {
    /// Overlay-Toggle (default `Ctrl+Shift+U`).
    Toggle,
    /// Click-through-Umschalter (default `Ctrl+Shift+O`).
    ClickThrough,
}

/// Live mapping pressed hotkey → action. Single source of truth for the
/// plugin handler AND for runtime re-registration: the plugin's global
/// handler dispatches against THIS state, so a rebind only swaps the
/// bindings + the OS registration, never the handler closure itself.
pub struct ShortcutBindings {
    toggle: RwLock<Option<Shortcut>>,
    click: RwLock<Option<Shortcut>>,
}

impl ShortcutBindings {
    pub fn new() -> Self {
        Self {
            toggle: RwLock::new(None),
            click: RwLock::new(None),
        }
    }

    fn slot(&self, kind: ShortcutKind) -> &RwLock<Option<Shortcut>> {
        match kind {
            ShortcutKind::Toggle => &self.toggle,
            ShortcutKind::ClickThrough => &self.click,
        }
    }

    pub fn current(&self, kind: ShortcutKind) -> Option<Shortcut> {
        *self.slot(kind).read().expect("shortcut bindings lock")
    }

    pub fn set(&self, kind: ShortcutKind, shortcut: Option<Shortcut>) {
        *self.slot(kind).write().expect("shortcut bindings lock") = shortcut;
    }

    /// Which action a pressed hotkey maps to (`Shortcut` is Copy+PartialEq).
    pub fn match_kind(&self, pressed: &Shortcut) -> Option<ShortcutKind> {
        if self.toggle.read().expect("lock").as_ref() == Some(pressed) {
            return Some(ShortcutKind::Toggle);
        }
        if self.click.read().expect("lock").as_ref() == Some(pressed) {
            return Some(ShortcutKind::ClickThrough);
        }
        None
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
    app.manage(ShortcutBindings::new());

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

    // C2a (t_6565fe8b): the persisted settings are the canonical shortcut
    // source (settings state is managed BEFORE overlay::init in lib.rs —
    // try_state keeps this robust if the order ever changes). Migration to
    // the historical U/O defaults happens inside the settings crate, so the
    // values here are always canonical, validated strings.
    let (toggle, click) = app
        .try_state::<crate::app_settings::SettingsState>()
        .map(|st| st.shortcut_combos())
        .unwrap_or_else(|| {
            (
                settings::DEFAULT_TOGGLE_SHORTCUT.to_owned(),
                settings::DEFAULT_CLICK_SHORTCUT.to_owned(),
            )
        });
    register_global_shortcuts(app, &toggle, &click);

    let handle = app.handle();
    spawn_game_poller(handle);
    // Hyprland dialog events (dialog-suspend of the focus bridge) — no-op off
    // Hyprland. NO workspace events: the overlay never reacts to (and never
    // influences) workspace navigation (v2, t_6d26985e).
    spawn_hyprland_events(handle);

    Ok(())
}

/// Create the borderless, always-on-top, taskbar-skipped spotlight window.
/// Hidden until the game is detected. Initial size = full primary monitor —
/// while the window is unmapped, `set_size` maps to GTK's default-size hint,
/// so this is the size the FIRST map starts with. A compositor `size` rule
/// can still override it at map time; the per-show geometry is owned by
/// `place_on_pointer_monitor` + `reassert_place_after_map` (t_5133f3e9).
/// The CSS floats the compact palette inside this window.
fn create_spotlight_window<R: Runtime>(app: &tauri::App<R>) -> tauri::Result<WebviewWindow<R>> {
    // Best-effort monitor size for the initial map; the real geometry is
    // decided on every show (cursor monitor), never here. On Windows the
    // initial size is the work_area too (t_bd8cbdfd) — never the taskbar.
    let (w, h) = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| {
            #[cfg(target_os = "windows")]
            let sz = m.work_area().size;
            #[cfg(not(target_os = "windows"))]
            let sz = m.size();
            (sz.width as f64, sz.height as f64)
        })
        .unwrap_or((PANEL_W, PANEL_H));

    let builder = WebviewWindowBuilder::new(
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
    .visible(false); // shown when Star Citizen is detected

    // Windows overlay shape (t_bd8cbdfd): ein echtes Overlay ist historisch —
    // randlos OHNE jede Chrome (auch kein Fensterschatten), NICHT
    // verschieb-/größenänderbar (die vier Drag-Ränder + Aero-Snap von
    // v0.5.12 entfallen damit; ohne Dekorationen gibt es auch keine
    // Titel-Leiste mehr, die das Fenster bewegen könnte). skip_taskbar und
    // always_on_top oben gelten schon für beide Plattformen. Linux bleibt
    // unangetastet: dort MUSS resizable(true) bleiben (Kommentar oben —
    // min=max-Geometrie würde den Wayland-Resize-Weg blockieren).
    #[cfg(target_os = "windows")]
    let builder = builder.shadow(false).resizable(false);

    let win = builder.build()?;

    // Every *show* re-places the window on the monitor under the pointer —
    // the multi-monitor behaviour — see `place_on_pointer_monitor`.
    win.center()?;

    Ok(win)
}

/// Register the global shortcuts from the persisted canonical combos
/// (C2a, t_6565fe8b). The `global-shortcut` backend is X11-based
/// (global_hotkey has no Wayland implementation), so on a pure Wayland session
/// the plugin fails to initialise; we swallow that — the overlay still works,
/// only the global hotkey is unavailable there. The Wayland fallback is the
/// Unix-socket CLI (`arch-overlay toggle`, see `ipc_socket` + docs/WAYLAND.md),
/// which compositor keybinds exec directly; C2b generates those binds FROM
/// the same canonical settings values.
///
/// The plugin is built with a single global handler that dispatches against
/// the managed `ShortcutBindings` — registered at plugin setup. Every actual
/// combo goes through `global_shortcut().register()` afterwards, which is the
/// exact API a runtime rebind uses (see `rebind_shortcut`).
fn register_global_shortcuts<R: Runtime>(app: &tauri::App<R>, toggle: &str, click: &str) {
    let mut builder = tauri_plugin_global_shortcut::Builder::new();
    builder = builder.with_handler(
        move |open_app: &AppHandle<R>, shortcut: &Shortcut, event: ShortcutEvent| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            let Some(bindings) = open_app.try_state::<ShortcutBindings>() else {
                return;
            };
            match bindings.match_kind(shortcut) {
                Some(ShortcutKind::Toggle) => toggle_visible(open_app),
                Some(ShortcutKind::ClickThrough) => {
                    let _ = toggle_click_through(open_app);
                }
                None => {}
            }
        },
    );

    // Failure (e.g. no X11 on Wayland) must not abort startup — but it must
    // be visible: a silently missing hotkey looks like a dead overlay. The
    // socket CLI (`ipc_socket`) is the Wayland fallback.
    if let Err(err) = app.handle().plugin(builder.build()) {
        log::warn!(
            "global shortcuts unavailable ({err}); under Wayland use the socket CLI: \
             arch-overlay toggle (see docs/WAYLAND.md) — persisted shortcuts become \
             compositor binds (C2b)"
        );
        return;
    }

    for (kind, combo) in [
        (ShortcutKind::Toggle, toggle),
        (ShortcutKind::ClickThrough, click),
    ] {
        if let Err(err) = register_combo(app.handle(), kind, combo) {
            // Startup collision (e.g. another app holds the combo): visible in
            // the log; the Settings view surfaces it on the next (re)bind.
            log::warn!("global shortcut {combo:?} ({kind:?}) not registered: {err}");
        }
    }
}

/// Register one canonical combo and publish it in the live bindings.
fn register_combo<R: Runtime>(
    app: &AppHandle<R>,
    kind: ShortcutKind,
    canonical: &str,
) -> Result<(), String> {
    let shortcut: Shortcut = match canonical.parse() {
        Ok(s) => s,
        Err(err) => {
            return Err(err_keyp(
                "shortcuts.err_combo_invalid",
                json!({ "combo": canonical, "err": err.to_string() }),
            ));
        }
    };
    let global_shortcut = app.state::<GlobalShortcut<R>>();
    global_shortcut.register(shortcut).map_err(|err| {
        err_keyp(
            "overlay.err_register",
            json!({ "combo": canonical, "err": err.to_string() }),
        )
    })?;
    if let Some(bindings) = app.try_state::<ShortcutBindings>() {
        bindings.set(kind, Some(shortcut));
    }
    Ok(())
}

/// Runtime re-registration (C2a): deregister the slot's old combo, register
/// the new one, swap the bindings. On failure (combo occupied by another
/// app) the OLD combo is re-registered and the error is returned — the
/// settings value stays untouched, so old value + old registration survive
/// intact (Windows path: same API, muda registers via RegisterHotKey).
///
/// If the plugin never initialised (Wayland), only the bindings are swapped:
/// the combo is still persisted as the canonical source and C2b will render
/// compositor binds from it — persisting must not fail there.
pub fn rebind_shortcut<R: Runtime>(
    app: &AppHandle<R>,
    kind: ShortcutKind,
    canonical: &str,
) -> Result<(), String> {
    let new_shortcut: Shortcut = canonical.parse::<Shortcut>().map_err(|err| {
        err_keyp(
            "overlay.err_invalid_combo",
            json!({ "combo": canonical, "err": err.to_string() }),
        )
    })?;
    let Some(bindings) = app.try_state::<ShortcutBindings>() else {
        return Err(err_key("overlay.err_state_unavailable"));
    };
    let old = bindings.current(kind);

    if let Some(global_shortcut) = app.try_state::<GlobalShortcut<R>>() {
        if let Some(old) = old {
            let _ = global_shortcut.unregister(old);
        }
        if let Err(err) = global_shortcut.register(new_shortcut) {
            // Restore the previous live registration before reporting.
            if let Some(old) = old {
                let _ = global_shortcut.register(old);
            }
            return Err(err_keyp(
                "overlay.err_register_occupied",
                json!({ "combo": canonical, "err": err.to_string() }),
            ));
        }
    } else {
        log::warn!(
            "global shortcuts inaktiv (Wayland?) — Kürzel {canonical:?} nur persistiert; \
             Aktivierung erfolgt über Compositor-Bindung (arch-overlay toggle, C2b)"
        );
    }

    bindings.set(kind, Some(new_shortcut));
    Ok(())
}

/// Whether the OS-level global shortcut plugin is alive (Soll-Ist display in
/// the Settings view: persisted combo vs. actually registered combo).
pub fn global_shortcuts_active<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.try_state::<GlobalShortcut<R>>().is_some()
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
    if let Some(st) = app.try_state::<OverlayState>()
        && st.config_open.swap(false, Ordering::AcqRel)
    {
        apply_visibility(app, effective_visible(st.inner()));
        emit_state(app);
    }
}

/// Effective window visibility: the usual user intent × game condition, OR
/// the tray-requested config view (independent of the game). Workspace
/// position plays NO role (v2, t_6d26985e): the overlay is a plain floating
/// window on whatever workspace it was shown on and moves with that
/// workspace — the app never reacts to workspace changes (no socket2
/// workspace listener, no polling) and never dispatches workspace/focus
/// navigation of its own.
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
            // The show request was computed by the caller; re-check the state
            // here so a racing change (e.g. game quit) wins over the stale
            // caller flag.
            let visible = visible
                && worker
                    .try_state::<OverlayState>()
                    .map(|st| effective_visible(st.inner()))
                    .unwrap_or(visible);
            if visible {
                // P3 (t_d340f851): opening ALWAYS behaves as focus mode — the
                // compositor focus dispatch below takes keyboard focus regard-
                // less of the persisted click-through flag, so the state and
                // the status button must say FOKUS too. Persisted click-through
                // intent is NOT restored on re-show; the user re-toggles it.
                // v2 (t_6d26985e): NO workspace is captured or dispatched —
                // the window simply maps on the user's CURRENT workspace and
                // stays there (passive workspace coherence).
                if let Some(st) = worker.try_state::<OverlayState>() {
                    st.click_through.store(false, Ordering::Release);
                }
                // Pre-map geometry hint: while the window is unmapped,
                // `set_size`/`set_position` land as GTK default-size/position
                // for the upcoming map, and the Hyprland geometry dispatch is
                // queued for whatever window exists. A compositor `size` rule
                // still wins AT map time (static effect) — which is why the
                // post-map `reassert_place_after_map` is the actual lever on
                // Hyprland (t_5133f3e9: rule-pinned 3440×1440 corrected to
                // 1080×1920 on the transform-3 monitor only via compositor
                // resize/move dispatch after the map).
                place_on_pointer_monitor(&win);
                // t_bd8cbdfd (Punkt 4): Windows has no Hyprland topmost handling —
                // re-assert always-on-top on every show (VM-Livetest showed the
                // overlay losing topmost status after game focus switches).
                #[cfg(target_os = "windows")]
                let _ = win.set_always_on_top(true);
                let _ = win.show();
                // NOTE: no panel-size re-assert here. The single source of
                // window geometry is `place_on_pointer_monitor` (full
                // monitor, Arkanis-style) — the CSS keeps the visible panel
                // compact inside the transparent full-screen window.
                // The GTK window is only realized after the first map; shape
                // the (hidden-at-boot) window only now, when click-through is
                // safe to apply. On Wayland the compositor may ignore the
                // position hint — documented limitation.
                // P3: the state above was just forced to focus mode — shape
                // the window accordingly (never ride a stale flag into the
                // input shape).
                let _ = win.set_ignore_cursor_events(false);
                // Multi-monitor: span the full monitor under the pointer.
                place_on_pointer_monitor(&win);
                // ...and re-assert it AFTER the (async) map, when Hyprland's
                // static size/move rules have already fired once.
                reassert_place_after_map(&win);
                // Take keyboard focus so the search is immediately typable —
                // compositors do not move keyboard focus to a newly shown
                // window, and the game would keep swallowing keystrokes.
                // Retry in a thread: the Wayland map is ASYNC — right after
                // show() the compositor does not know the window yet
                // ("hl.focus: window not found"), so the immediate attempt
                // always missed (verified via compositor_focus logging).
                let label = SPOTLIGHT_WM_CLASS.to_string();
                let win_probe = win.clone();
                std::thread::spawn(move || {
                    for wait in [300u64, 600, 1200] {
                        std::thread::sleep(Duration::from_millis(wait));
                        // If the overlay was hidden again meanwhile (game
                        // quit), stop touching the compositor — a pending
                        // focus grab on a hidden window would yank the user
                        // out of whatever they switched to.
                        if !win_probe.is_visible().unwrap_or(false) {
                            return;
                        }
                        compositor_focus(&label);
                    }
                });
                // v2 (t_6d26985e): arm the stay_focused bridge AFTER the map,
                // from the app itself — the SC focus-steal workaround keeps
                // working even on setups without the static rule. NOTE: NO
                // `pin` is ever dispatched: the overlay lives on the
                // workspace it was shown on and hides with it (passive
                // workspace coherence). The dialog suspend/restore cycle
                // (event listener) clears it again whenever one of our own
                // dialogs needs the input.
                // Windows-Build: die Hyprland-Fokusbrücke existiert nur auf
                // Linux (arm_focus_ownership ist linux-cfg'd) — der Aufruf
                // muss es ebenso sein, sonst bricht der MSVC-Build (E0425).
                #[cfg(target_os = "linux")]
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(500));
                    if win.is_visible().unwrap_or(false) {
                        arm_focus_ownership();
                    }
                });
                // State changed (click_through forced off): publish once more,
                // so the status button never lags behind the actual behaviour.
                emit_state(&worker);
            } else {
                let _ = win.hide();
                // Hand keyboard focus back to the game.
                compositor_focus("starcitizen.exe");
            }
        }
    });
}

/// hyprctl mit hartem Zeitbudget (t_1adbc017): waehrend eines laufenden
/// `hyprctl reload` antwortet des Kompositors Socket-Interface erst nach
/// Abschluss — unlimitierte `.output()`-Aufrufe blockieren dann den
/// aufrufenden Thread (im Main-Thread-Kontext bis zum xdg-Ping-Timeout:
/// 'Application Not Responding', Input-Sperre fuer das Overlay-Fenster).
/// `None` = Spawn-Fehler oder Timeout (Kind wird gekillt); die Anrufer
/// fallen auf ihre ohnehin vorhandenen Best-Effort-Pfade.
#[cfg(target_os = "linux")]
fn hyprctl_limited(args: &[&str]) -> Option<std::process::Output> {
    const BUDGET: std::time::Duration = std::time::Duration::from_millis(800);
    let start = std::time::Instant::now();
    let mut child = std::process::Command::new("hyprctl")
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) => {
                if start.elapsed() > BUDGET {
                    let _ = child.kill();
                    let _ = child.wait();
                    log::warn!("hyprctl {:?}: Timeout nach {BUDGET:?}", args);
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(40));
            }
            Err(_) => {
                let _ = child.kill();
                return None;
            }
        }
    }
}

/// Hyprland's own global cursor position via `hyprctl -j cursorpos` — in the
/// compositor's logical layout pixels. This is exactly the coordinate space in
/// which `available_monitors()` reports its rectangles on Wayland (verified
/// live against `hyprctl monitors` output, t_317b9d15). Returns `None` when
/// hyprctl is unavailable (not Hyprland, Lua-bridge rejects the keyword, …).
#[cfg(target_os = "linux")]
fn hyprland_cursor_position() -> Option<(i32, i32)> {
    let out = hyprctl_limited(&["-j", "cursorpos"])?;
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
        if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
            && let Some((x, y)) = hyprland_cursor_position()
        {
            return Some((x, y, "hyprland cursorpos"));
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

    let (mpos, msize) = {
        // Linux (unchanged, Arkanis-style): the overlay uses the WHOLE monitor —
        // the transparent window spans the screen, the palette UI floats inside
        // it (CSS keeps the visible shell compact). This also makes the click/
        // keyboard surface predictable across monitor layouts.
        // Windows (t_bd8cbdfd, Punkte 3+6): size = work_area (Monitor minus
        // Taskleiste), not the full monitor — the overlay must never cover the
        // taskbar and must not block it.
        #[cfg(target_os = "windows")]
        let target = {
            let wa = mon.work_area();
            (wa.position, wa.size)
        };
        #[cfg(not(target_os = "windows"))]
        let target = (mon.position(), mon.size());
        target
    };
    // tao lever: on X11/XWayland this resizes outright; on Wayland it only
    // acts pre-map (GTK default-size hint for the upcoming map) and is
    // silently dropped after map (wire proof t_5133f3e9, WAYLAND_DEBUG:
    // xdg_toplevel simply has no client->compositor resize request).
    let _ = win.set_size(tauri::PhysicalSize::new(msize.width, msize.height));
    let _ = win.set_position(tauri::PhysicalPosition::new(mpos.x, mpos.y));
    // Hyprland lever: ask the compositor to do it for us.
    compositor_geometry((mpos.x, mpos.y), (msize.width, msize.height));
}

/// Build the two Hyprland Lua dispatcher expressions for a geometry set.
/// Pure + exact-string tested: negative monitor coordinates (rotated/above
/// layouts, e.g. HDMI-A-2 at 1825,-1920) must survive formatting verbatim.
fn geometry_dispatch_lua(pid: u32, pos: (i32, i32), size: (u32, u32)) -> (String, String) {
    (
        format!(
            "hl.dsp.window.resize({{ x = {}, y = {}, window = \"pid:{pid}\" }})",
            size.0, size.1
        ),
        format!(
            "hl.dsp.window.move({{ x = {}, y = {}, window = \"pid:{pid}\" }})",
            pos.0, pos.1
        ),
    )
}

/// The real geometry lever on Hyprland: compositor-side resize + move of OUR
/// OWN window via the control socket (same hyprctl pattern as `compositor_focus`).
///
/// Why: Hyprland `size`/`move` window rules are static (they fire on every map,
/// and hide/show is a re-map) and pin the window to whatever the config says —
/// a fixed `size {3440,1440}` leaves the overlay wider than a 1080×1920
/// transform-3 monitor. The client cannot fight that over Wayland protocol
/// (no client resize after map; `set_maximized`/`set_fullscreen` DO reach the
/// compositor but are swallowed for a pinned floating window — live wire
/// evidence t_5133f3e9). Hyprland's own dispatchers do work, targeted by
/// `pid:` (a selector matching no mapped window is a verified no-op):
/// `hl.dsp.window.resize` + `hl.dsp.window.move`, coordinates in Hyprland
/// layout px — the same space `available_monitors()` reports (t_317b9d15),
/// with `move` absolute so the monitor position IS the window target.
/// Order: resize first, then move — Hyprland's resize re-anchors the window
/// (live-observed +1180,-240 shift), the move corrects it.
#[cfg(target_os = "linux")]
fn compositor_geometry(pos: (i32, i32), size: (u32, u32)) {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
        return; // not a Hyprland session: nothing to dispatch to
    }
    let (resize_lua, move_lua) = geometry_dispatch_lua(std::process::id(), pos, size);
    for lua in [resize_lua.as_str(), move_lua.as_str()] {
        match hyprctl_limited(&["dispatch", lua]) {
            Some(out) => log::info!(
                "geometry dispatch: out={:?}",
                String::from_utf8_lossy(&out.stdout).trim()
            ),
            None => log::warn!("geometry dispatch: hyprctl spawn failed or timed out"),
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn compositor_geometry(_pos: (i32, i32), _size: (u32, u32)) {}

/// Re-assert the pointer-monitor geometry AFTER the async Wayland map.
///
/// Why this is needed (live proof, card t_5133f3e9): Hyprland window rules
/// like `size`/`move` are *static* effects — evaluated once per window open
/// (wiki: "evaluated once when the window is opened and never again"), and
/// hide/show is unmap/re-map, i.e. a new open: the rule fires on EVERY show
/// (measured: right after re-show the window is back at the pinned 3440×1440).
/// Client-side geometry requests before or during that map are overwritten or
/// protocol-impossible. `place_on_pointer_monitor`'s Hyprland lever (compositor
/// `window.resize`/`window.move` dispatch) only works once the window is
/// actually mapped — and the Wayland map is async (same race the
/// `compositor_focus` retry loop documents), so "after the map" means: delayed.
fn reassert_place_after_map<R: Runtime>(win: &WebviewWindow<R>) {
    // Same cadence class as the compositor_focus retry: comfortably past any
    // map a compositor on this machine has ever needed, short enough that the
    // resize is invisible between show() and first paint attention.
    const DELAYS_MS: [u64; 2] = [400, 900];
    let win = win.clone();
    std::thread::spawn(move || {
        for delay in DELAYS_MS {
            std::thread::sleep(Duration::from_millis(delay));
            let w = win.clone();
            // If the overlay was hidden again meanwhile, the pending geometry
            // becomes the default-size hint for the next map — harmless, but
            // pointless: stop.
            if !win.is_visible().unwrap_or(false) {
                log::info!("reassert: window hidden again -> stop re-place");
                return;
            }
            log::info!("reassert: re-placing after map (+{delay} ms)");
            let _ = win.run_on_main_thread(move || place_on_pointer_monitor(&w));
        }
    });
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
        let ok = match hyprctl_limited(&["dispatch", &lua]) {
            Some(o) => {
                log::info!(
                    "compositor_focus({class}): status={:?} out={:?}",
                    o.status.code(),
                    String::from_utf8_lossy(&o.stdout).trim()
                );
                o.status.success()
            }
            None => {
                log::info!("compositor_focus({class}): spawn failed or timed out");
                false
            }
        };
        if !ok {
            let _ = hyprctl_limited(&["dispatch", "focuswindow", &format!("class:{class}")]);
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = class;
}

/// Window title of the spotlight window as mapped by the compositor. Used to
/// distinguish the MAIN overlay window from other same-class toplevels of our
/// own process (the GTK file chooser of `<input type="file">` shares WM class
/// AND pid with us — only the title differs).
pub const SPOTLIGHT_TITLE: &str = "SC Architects Spotlight";

// ---------------------------------------------------------------------------
// Hyprland live behaviour (v2, t_6d26985e): passive workspace coherence +
// stay_focused focus bridge + dialog suspend.
//
// v0.5.x (PA, t_d340f851) actively managed the workspace: every show bound
// the overlay to the SC workspace, a socket2 `workspace>>` listener hid it
// whenever the user left — and every hide dispatched focus back to the game,
// yanking the user out of Discord/Browser/2nd monitor at every switch
// attempt. Reverted in v0.4.6; NEVER return to active workspace/focus
// dispatches.
//
// v2 is strictly passive:
//
//  * NO pin anywhere — static rule (hyprland.lua) and runtime dispatches
//    removed. The overlay is a plain floating window: it maps on the
//    workspace the user is currently on and disappears with it on workspace
//    switches (stock Hyprland behaviour). Reaching the overlay is the
//    user's business; the app never switches workspaces for them.
//  * NO socket2 workspace events, NO polling that "corrects" the workspace,
//    NO focus dispatch on workspace changes.
//  * The stay_focused bridge stays (it is the OPPOSITE of the problem):
//    while the overlay is visible it keeps keyboard focus against the
//    game's focus-steal; `openwindow>>`/`closewindow>>` still detect our
//    own in-process dialogs (file chooser) and suspend/restore that prop so
//    the dialog is usable.
//
// All compositor calls are best-effort via the same hyprctl_limited budget
// as the geometry/focus dispatchers; without Hyprland everything is a no-op.
// ---------------------------------------------------------------------------

/// Lua snippet arming/disarming `stay_focused` on a window selector. Pure so
/// the exact string is unit-tested (Hyprland 0.56 Lua API: dispatchers must
/// be wrapped in `hl.dispatch(...)`, set_prop takes STRING values, and the
/// prop is `stay_focused` — `stayfocused` is rejected with "Invalid prop
/// name", both probed live).
fn stay_focused_lua(selector: &str, on: bool) -> String {
    format!(
        "hl.dispatch(hl.dsp.window.set_prop({{ window = \"{selector}\", \
         prop = \"stay_focused\", value = \"{}\" }}))",
        if on { 1 } else { 0 }
    )
}

#[cfg(target_os = "linux")]
fn hypr_eval(lua: &str) {
    match hyprctl_limited(&["eval", lua]) {
        Some(out) => log::info!(
            "hypr eval: out={:?}",
            String::from_utf8_lossy(&out.stdout).trim()
        ),
        None => log::warn!("hypr eval: spawn failed or timed out"),
    }
}

/// Address (`0x…`) of the MAIN spotlight window — same pid + class as any
/// in-process GTK dialog, so the window TITLE is the discriminator.
#[cfg(target_os = "linux")]
fn hyprland_main_window_address() -> Option<String> {
    let out = hyprctl_limited(&["-j", "clients"])?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    for c in v.as_array()? {
        if c.get("pid")?.as_i64()? == std::process::id() as i64
            && c.get("class")?.as_str()? == SPOTLIGHT_WM_CLASS
            && c.get("title")?.as_str()? == SPOTLIGHT_TITLE
        {
            return c.get("address")?.as_str().map(str::to_owned);
        }
    }
    None
}

/// Arm `stay_focused` on the main overlay window (post-map) — the SC
/// focus-steal workaround, independent of whether the user's hyprland.lua
/// still carries the static rule. NEVER pins (v2, t_6d26985e): pinning would
/// make the overlay global across workspaces, which is exactly what must not
/// happen again.
#[cfg(target_os = "linux")]
fn arm_focus_ownership() {
    let Some(addr) = hyprland_main_window_address() else {
        return;
    };
    let sel = format!("address:{addr}");
    hypr_eval(&stay_focused_lua(&sel, true));
}

/// Suspend the focus bridge while one of our own dialogs (file chooser) needs
/// focus/stacking. Best-effort: without a main window address nothing happens.
#[cfg(target_os = "linux")]
fn suspend_focus_ownership() {
    let Some(addr) = hyprland_main_window_address() else {
        return;
    };
    let sel = format!("address:{addr}");
    hypr_eval(&stay_focused_lua(&sel, false));
}

/// Restore after the dialog closed. Skips the focus grab when the overlay hid
/// itself meanwhile (game quit) — refocusing a hidden window would be a
/// no-op at best.
#[cfg(target_os = "linux")]
fn restore_focus_ownership<R: Runtime>(app: &AppHandle<R>) {
    let Some(addr) = hyprland_main_window_address() else {
        return;
    };
    let sel = format!("address:{addr}");
    hypr_eval(&stay_focused_lua(&sel, true));
    let still_visible = app
        .get_webview_window(SPOTLIGHT_LABEL)
        .map(|w| w.is_visible().unwrap_or(false))
        .unwrap_or(false);
    if still_visible {
        compositor_focus(SPOTLIGHT_WM_CLASS);
    }
}

/// socket2 event path for this Hyprland instance (`$XDG_RUNTIME_DIR` first,
/// the historical `/tmp/hypr` fallback second).
#[cfg(target_os = "linux")]
fn hyprland_event_socket(name: &str) -> Option<std::path::PathBuf> {
    let sig = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        let p = std::path::PathBuf::from(runtime)
            .join("hypr")
            .join(&sig)
            .join(name);
        if p.exists() {
            return Some(p);
        }
    }
    let p = std::path::PathBuf::from("/tmp/hypr").join(&sig).join(name);
    p.exists().then_some(p)
}

/// Parse an `openwindow>>` event line: `openwindow>>ADDR,WORKSPACE,CLASS,TITLE…`
/// (the title may contain commas — splitn keeps them).
fn parse_openwindow(line: &str) -> Option<(String, String, String)> {
    let payload = line.strip_prefix("openwindow>>")?;
    let mut it = payload.splitn(4, ',');
    let addr = it.next()?.trim().to_owned();
    let _ws = it.next()?;
    let class = it.next()?.trim().to_owned();
    let title = it.next()?.trim().to_owned();
    Some((addr, class, title))
}

/// Whether an opened window is one of our own in-process dialogs: it shares
/// pid+class with the overlay, but is not the main window (title differs).
/// The GTK file chooser spawned by `<input type="file">` matches exactly this.
fn is_in_process_dialog(class: &str, title: &str) -> bool {
    class == SPOTLIGHT_WM_CLASS && title != SPOTLIGHT_TITLE
}

/// Background listener on Hyprland's socket2 event stream. Reconnects with a
/// backoff; every parse failure is logged and skipped (the event format is
/// compositor-stable, but a malformed line must never kill the loop).
#[cfg(target_os = "linux")]
fn spawn_hyprland_events<R: Runtime>(handle: &AppHandle<R>) {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
        return;
    }
    let app = handle.clone();
    std::thread::spawn(move || {
        use std::io::BufRead;
        loop {
            let Some(path) = hyprland_event_socket(".socket2.sock") else {
                std::thread::sleep(Duration::from_secs(2));
                continue;
            };
            match std::os::unix::net::UnixStream::connect(&path) {
                Err(err) => {
                    log::info!("socket2 connect failed ({err}); retrying");
                    std::thread::sleep(Duration::from_secs(2));
                }
                Ok(stream) => {
                    let mut dialog_addr: Option<String> = None;
                    // v2 (t_6d26985e): workspace events (`workspace>>`,
                    // `focusedmon…`) are deliberately IGNORED — the overlay
                    // must never react to (let alone correct) the user's
                    // workspace navigation. Only own-dialog events are
                    // consumed, for the stay_focused suspend/restore cycle.
                    let reader = std::io::BufReader::new(stream);
                    for line in reader.lines() {
                        let Ok(line) = line else { break };
                        // Synthetic events (hl.dsp.event, probes) arrive with a
                        // `custom>>` prefix — strip it so both shapes match.
                        let line = line.strip_prefix("custom>>").unwrap_or(&line);
                        if let Some((_addr, class, title)) = parse_openwindow(line) {
                            if is_in_process_dialog(&class, &title) {
                                log::info!(
                                    "own dialog opened ({title:?}) -> suspend focus ownership"
                                );
                                dialog_addr = Some(_addr);
                                suspend_focus_ownership();
                            }
                        } else if let Some(addr) = line.strip_prefix("closewindow>>")
                            && dialog_addr.as_deref() == Some(addr.trim())
                        {
                            log::info!("own dialog closed -> restore focus ownership");
                            dialog_addr = None;
                            restore_focus_ownership(&app);
                        }
                    }
                    // Stream dropped (compositor restart): loop reconnects.
                    log::info!("socket2 stream ended; reconnecting");
                }
            }
        }
    });
}

#[cfg(not(target_os = "linux"))]
fn spawn_hyprland_events<R: Runtime>(_handle: &AppHandle<R>) {}

/// Background poller: every `POLL_INTERVAL_MS` re-checks whether Star Citizen
/// runs and updates the overlay accordingly. Detached — dies with the process,
/// so it needs no explicit shutdown signalling. Reading /proc on each tick is
/// cheap relative to a 5 s cadence, and only state *changes* touch the window.
/// (5 s statt 2 s: schließt auch das Windows-typische tasklist-Flackern
/// wahrnehmbar — Detection-Latenz beim SC-Start bleibt unkritisch.)
const POLL_INTERVAL_MS: u64 = 5000;

fn spawn_game_poller<R: Runtime>(handle: &AppHandle<R>) {
    let app = handle.clone();
    std::thread::spawn(move || {
        let mut last: Option<bool> = None;
        loop {
            let running = game_detection::is_game_running();
            // Log only transitions — enough to diagnose a missed detection
            // (e.g. a fake StarCitizen.exe test copy) without log spam per poll.
            if last != Some(running) {
                log::info!(
                    "game-detection: running={} (poll every {}ms)",
                    running,
                    POLL_INTERVAL_MS
                );
                last = Some(running);
            }
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

    /// The Hyprland geometry dispatch strings must survive the real Sebo
    /// layout — negative monitor coords, transform-swapped size — verbatim,
    /// because they are shipped to the Lua bridge as-is (t_5133f3e9).
    #[test]
    fn geometry_dispatch_lua_formats_measured_layout() {
        // HDMI-A-2: 1080x1920 at 1825,-1920 (transform 3).
        let (resize, mov) = geometry_dispatch_lua(4242, (1825, -1920), (1080, 1920));
        assert_eq!(
            resize,
            "hl.dsp.window.resize({ x = 1080, y = 1920, window = \"pid:4242\" })"
        );
        assert_eq!(
            mov,
            "hl.dsp.window.move({ x = 1825, y = -1920, window = \"pid:4242\" })"
        );
        // DP-3: 3440x1440 at 0,0 — the identity case (dispatch is a no-op change).
        let (resize, mov) = geometry_dispatch_lua(4242, (0, 0), (3440, 1440));
        assert_eq!(
            resize,
            "hl.dsp.window.resize({ x = 3440, y = 1440, window = \"pid:4242\" })"
        );
        assert_eq!(
            mov,
            "hl.dsp.window.move({ x = 0, y = 0, window = \"pid:4242\" })"
        );
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

    /// C2a (t_6565fe8b): the canonical strings persisted by the settings
    /// crate MUST parse through global_hotkey's `Shortcut::from_str` into
    /// exactly the hotkey the plugin registers (Windows/X11 path shares this
    /// exact parser). Defaults included.
    #[test]
    fn canonical_combos_parse_into_expected_hotkeys() {
        use tauri_plugin_global_shortcut::{Code, Modifiers};
        let toggle: Shortcut = "Ctrl+Shift+U"
            .parse()
            .expect("canonical default toggle parses");
        assert_eq!(toggle.key, Code::KeyU);
        assert!(toggle.mods.contains(Modifiers::CONTROL));
        assert!(toggle.mods.contains(Modifiers::SHIFT));

        let click: Shortcut = "Ctrl+Shift+O"
            .parse()
            .expect("canonical default click parses");
        assert_eq!(click.key, Code::KeyO);

        let fancy: Shortcut = "Ctrl+Alt+Shift+Super+F9"
            .parse()
            .expect("full canonical order parses");
        assert_eq!(fancy.key, Code::F9);
        assert!(fancy.mods.contains(Modifiers::SUPER));

        // settings::validate_shortcut_combo output feeds this parse directly
        for canonical in [
            settings::DEFAULT_TOGGLE_SHORTCUT,
            settings::DEFAULT_CLICK_SHORTCUT,
            "Alt+Space",
            "Ctrl+PageUp",
        ] {
            assert!(
                canonical.parse::<Shortcut>().is_ok(),
                "{canonical} must parse"
            );
        }
    }

    /// C2a: dispatch is binding-driven, not closure-driven — after a rebind
    /// (bindings swap WITHOUT re-registering the plugin handler) the NEW combo
    /// maps to its action and the OLD combo maps to nothing.
    #[test]
    fn bindings_dispatch_follows_rebind() {
        let bindings = ShortcutBindings::new();
        bindings.set(ShortcutKind::Toggle, Some("Ctrl+Shift+U".parse().unwrap()));
        bindings.set(
            ShortcutKind::ClickThrough,
            Some("Ctrl+Shift+O".parse().unwrap()),
        );

        let u: Shortcut = "Ctrl+Shift+U".parse().unwrap();
        let o: Shortcut = "Ctrl+Shift+O".parse().unwrap();
        assert_eq!(bindings.match_kind(&u), Some(ShortcutKind::Toggle));
        assert_eq!(bindings.match_kind(&o), Some(ShortcutKind::ClickThrough));

        // swap the toggle slot to Ctrl+Alt+K (what rebind_shortcut publishes)
        let k: Shortcut = "Ctrl+Alt+K".parse().unwrap();
        bindings.set(ShortcutKind::Toggle, Some(k));
        assert_eq!(bindings.match_kind(&k), Some(ShortcutKind::Toggle));
        assert_eq!(bindings.match_kind(&u), None, "old combo must stop firing");
        assert_eq!(bindings.current(ShortcutKind::ClickThrough), Some(o));
    }

    /// `rebind_shortcut` without a live plugin (the Wayland case: bindings
    /// managed, no GlobalShortcut state) must NOT error — the combo persists
    /// as canonical source for C2b and only the bindings move. The
    /// occupied-by-other-app failure path (plugin state present) needs a live
    /// hotkey manager and is verified on the Windows/X11 build, not in unit
    /// scope; the bindings-only fallback is exactly what C2a promises for
    /// Wayland.
    #[test]
    fn rebind_without_plugin_updates_bindings_only() {
        let app = mock_app_with_bindings();
        let h = app.handle();
        let bindings = h.state::<ShortcutBindings>();
        bindings.set(ShortcutKind::Toggle, Some("Ctrl+Shift+U".parse().unwrap()));
        assert!(bindings.current(ShortcutKind::Toggle).is_some());

        let result = rebind_shortcut(h, ShortcutKind::Toggle, "Ctrl+Alt+K");
        assert!(
            result.is_ok(),
            "Wayland rebind must persist, not fail: {result:?}"
        );
        assert_eq!(
            bindings.current(ShortcutKind::Toggle),
            Some("Ctrl+Alt+K".parse::<Shortcut>().unwrap())
        );
        assert!(!global_shortcuts_active(h), "mock app has no plugin state");
    }

    fn mock_app_with_bindings() -> tauri::App<tauri::test::MockRuntime> {
        let app = mock_app();
        app.manage(OverlayState::new());
        app.manage(ShortcutBindings::new());
        app
    }

    // ---- t_6d26985e (v2): passive workspace coherence, dialog detection ----

    /// v2: workspace position plays NO role in visibility — the overlay shows
    /// purely on `config_open || (user_visible && game_running)`. There is no
    /// bound/active workspace state left to consult; this pins that
    /// regression surface (the PA active-workspace gating is gone).
    #[test]
    fn effective_visible_ignores_workspaces() {
        let st = OverlayState::new();
        st.user_visible.store(true, Ordering::Release);
        st.game_running.store(true, Ordering::Release);
        assert!(effective_visible(&st));
        // Config view alone shows the overlay (independent of the game).
        let st2 = OverlayState::new();
        st2.config_open.store(true, Ordering::Release);
        assert!(effective_visible(&st2));
    }

    /// `openwindow>>` parsing: address/workspace/class/title with commas in
    /// the title intact.
    #[test]
    fn parse_openwindow_splits_with_commas_in_title() {
        let (addr, class, title) =
            parse_openwindow("openwindow>>0x649,4,architects-overlay,Open File, awesome")
                .expect("parse");
        assert_eq!(addr, "0x649");
        assert_eq!(class, "architects-overlay");
        assert_eq!(title, "Open File, awesome");
        assert!(parse_openwindow("workspace>>3").is_none());
    }

    /// Dialog discriminator: same class, different title = our in-process
    /// dialog; the main window title is never a dialog; foreign classes never.
    #[test]
    fn in_process_dialog_matches_only_same_class_other_title() {
        assert!(is_in_process_dialog(SPOTLIGHT_WM_CLASS, "Datei auswählen"));
        assert!(!is_in_process_dialog(SPOTLIGHT_WM_CLASS, SPOTLIGHT_TITLE));
        assert!(!is_in_process_dialog("firefox", "Whatever"));
    }

    /// Exact dispatcher strings (live-probed on Hyprland 0.56.2): the Lua
    /// bridge needs hl.dispatch-wrapping, string values, prop `stay_focused`.
    #[test]
    fn lua_snippets_are_bridge_exact() {
        assert_eq!(
            stay_focused_lua("address:0xabc", true),
            "hl.dispatch(hl.dsp.window.set_prop({ window = \"address:0xabc\", \
             prop = \"stay_focused\", value = \"1\" }))"
        );
        assert_eq!(
            stay_focused_lua("address:0xabc", false),
            "hl.dispatch(hl.dsp.window.set_prop({ window = \"address:0xabc\", \
             prop = \"stay_focused\", value = \"0\" }))"
        );
    }

    /// v2 regression pin (t_6d26985e): NO pin dispatch may ever exist again.
    /// Grepping the source for the pin dispatcher is the cheapest guarantee —
    /// if someone reintroduces the pin dispatcher (or a pin windowrule) this
    /// test fails and the review sees it.
    #[test]
    fn no_pin_dispatches_exist() {
        // Needles are split across string pieces so the test never matches
        // its own source file.
        let pin_dsp = "hl.dsp.window.".to_owned() + "pin";
        let mvt = "movetowork".to_owned() + "space";
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        for path in [dir.join("src/overlay.rs"), dir.join("src/lib.rs")] {
            let src = std::fs::read_to_string(&path).unwrap_or_default();
            assert!(
                !src.contains(&pin_dsp) && !src.contains(&mvt),
                "{} must not contain active workspace/pin dispatches (v2 ban)",
                path.display()
            );
        }
    }
}
