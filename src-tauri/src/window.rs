//! Fenstermodus (Wunsch 1, M1 t_90cb34ee): ein ZWEITES, normales
//! WebviewWindow neben dem Overlay-Spotlight (Plan §2 Option A).
//!
//! Verträge (aus docs/FENSTERMODUS-PLAN.md §1/§2/§5/§9.1 — bindend):
//!
//! - Label `app-window`, dasselbe dist (`index.html`); der Frontend-Zweig
//!   erkennt den Fenstermodus über das VORHANDENE Label-Muster
//!   (`getCurrentWindow().label` → `data-window`, overlay.rs-Frontend-Vertrag
//!   Sonde §1.1) plus Hash-Route `#/window` in der URL, falls die
//!   Build-Pipeline sie erhält. Der Datensatz im WebviewUrl-Pfad wurde
//!   bewusst NICHT auf `index.html#/window` erweitert: Tauri encodiert `#`
//!   beim App-URL-Join als `%23` (kein Fragment) — Label-Erkennung ist
//!   robust und ist der von der Sonde empfohlene Andockpunkt.
//! - `decorations(false)` — eigenes Titelband im Overlay-Stil (Variante B,
//!   Sebo-Beschluss §9.1.1) — aber `transparent(false)` (NORMALES Fenster),
//!   `resizable(true)`, `minimizable(true)`, `skip_taskbar(false)`, NICHT
//!   `always_on_top` (das Overlay bleibt über dem Spiel, das Fenster lebt
//!   unter normalen Apps — Risiko R3).
//! - Sichtbar nur nach Tray/Toggle: bei App-Start NICHT automatisch zeigen.
//!   Das Overlay bleibt unverändert (overlay.rs-Kernverträge unberührt,
//!   OVERLAY-UNVERÄNDERT-REGEL — dieses Modul fügt nur hinzu).
//! - CloseRequested → `prevent_close()` + `hide()`: Schließen = nur ins Tray
//!   (§5). Beenden ausschließlich via Fenster-Menü (`window_quit` →
//!   `app.exit(0)`, sauberer Exit wie Tray-Quit) oder Tray.
//! - Minimieren nativ, wo die Desktopumgebung es zulässt (Wayland-Caveat
//!   Sonde §5: CSD/SSD-Verhalten ist nicht garantiert — dokumentierte
//!   Limitation, kein Sondercode).
//! - Geometrie-Persistenz (§9.1.2+3): Default einmalig (1200×800 zentriert
//!   auf dem Primärmonitor), danach IMMER der letzte Stand. Gespeichert wird
//!   debounced bei Moved/Resized; beim Öffnen wird die Monitor-Gültigkeit
//!   geprüft (Monitor weg → Primärmonitor zentriert).
//!
//! Hotkey-Koexistenz (Sonde §4-Matrix, Scope 6): Overlay-Toggle/Klick-Modus
//! sind fensterunabhängig (OS-Ebene: GlobalShortcuts + Windows-LL-Hook +
//! Hyprland-Compositor-Binds) und wirken unverändert auch bei offenem und
//! fokussiertem `app-window`. Dieser Modulcode greift in KEINEN dieser Pfade
//! ein — bewusst nichts zu tun; das Verhalten ist ein Architekturmerkmal
//! (Beleg: `overlay.rs` register_global_shortcuts, `hotkey_llhook.rs`
//! WH_KEYBOARD_LL-Dispatch, `hyprland_sync.rs` sync_combo — alles
//! fensteragnostisch). F1–F12-Modi sind fokusgebundene Frontend-Keydowns und
//! funktionieren im Fenster wie im Overlay, solange es fokussiert ist.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::webview::WebviewWindowBuilder;
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindow};

/// Label des Fenstermodus-Webviews (Capabilities + Frontend-Zweig).
pub const APP_WINDOW_LABEL: &str = "app-window";

/// Default-Geometrie (Plan §9 Frage 2, Sebo-Beschluss §9.1.2):
/// 1200×800, zentriert auf dem Primärmonitor — einmalig; danach gilt immer
/// die zuletzt persistierte Geometrie.
const DEFAULT_W: u32 = 1200;
const DEFAULT_H: u32 = 800;

/// Debounce-Fenster für die Geometrie-Persistenz: Move/Resize feuern
/// kontinuierlich; gespeichert wird erst nach dieser Stillstandszeit.
const GEOMETRY_SAVE_DEBOUNCE_MS: u64 = 400;

/// Tray-Checkbox-Handle (vom tray::init gesetzt), damit show/hide den
/// Checkbox-Zustand des `window_toggle`-Menüpunkts live nachziehen.
pub struct WindowTrayItem<R: Runtime>(pub Mutex<Option<tauri::menu::CheckMenuItem<R>>>);

/// In-memory Zwischenstand der Geometrie (vom Fenster-Event-Thread gefüllt,
/// vom Debounce-Thread gelesen und persistiert).
#[derive(Default)]
struct PendingGeometry {
    pending: Mutex<Option<settings::WindowGeometry>>,
    save_scheduled: AtomicBool,
}

/// Monitor-Box für die pure Monitor-Validität (unit-getestet ohne App).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorRect {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// Pure Restore-Logik (M1 Scope 4): persistierte Geometrie → Fenster-Box.
///
/// - Geometrie vorhanden UND Monitor existiert → exakt x/y/w/h übernehmen.
/// - Monitor weg (Name nicht mehr in `monitors`) oder keine Geometrie →
///   Default 1200×800 zentriert auf `monitors[primary]`, Größen clampen auf
///   den Monitor (kleinere Monitore bekommen kein überstehendes Fenster).
/// - Gar keine Monitore (sollte nicht passieren) → Default bei (0,0).
pub fn resolve_geometry(
    stored: Option<&settings::WindowGeometry>,
    monitors: &[MonitorRect],
    primary: usize,
) -> (i32, i32, u32, u32) {
    if let Some(g) = stored
        && let Some(m) = monitors.iter().find(|m| m.name == g.monitor)
    {
        let w = g.w.min(m.w);
        let h = g.h.min(m.h);
        return (g.x, g.y, w, h);
    }
    let Some(m) = monitors.get(primary) else {
        return (0, 0, DEFAULT_W, DEFAULT_H);
    };
    let w = DEFAULT_W.min(m.w);
    let h = DEFAULT_H.min(m.h);
    let x = m.x + ((m.w as i32 - w as i32) / 2).max(0);
    let y = m.y + ((m.h as i32 - h as i32) / 2).max(0);
    (x, y, w, h)
}

/// Monitor-Boxen der App-Session lesen (Primärmonitor zuerst gemeldet von
/// `primary_monitor()`; die Liste selbst ist Enumerations-Reihenfolge).
fn monitor_rects<R: Runtime>(app: &AppHandle<R>) -> (Vec<MonitorRect>, usize) {
    let primary = app
        .primary_monitor()
        .ok()
        .flatten()
        .and_then(|m| m.name().map(|n| n.to_string()));
    let mut list: Vec<MonitorRect> = app
        .available_monitors()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|m| {
            let pos = m.position();
            let size = m.size();
            m.name().map(|n| MonitorRect {
                name: n.to_string(),
                x: pos.x,
                y: pos.y,
                w: size.width,
                h: size.height,
            })
        })
        .collect();
    let primary_idx = primary
        .as_ref()
        .and_then(|p| list.iter().position(|m| &m.name == p))
        .unwrap_or(0);
    // Primärmonitor an den Listenanfang = `[primary]`-Index 0 stabil halten.
    if primary_idx > 0 {
        list.swap(0, primary_idx);
    }
    (list, 0)
}

/// Erzeugt das Fenstermodus-Fenster (hidden) + Lifecycle-Handler.
/// Sichtbar erst via Tray/`window_show` — der App-Start bleibt Overlay-only.
pub fn init<R: Runtime>(app: &tauri::App<R>) -> tauri::Result<()> {
    app.manage(PendingGeometry::default());
    app.manage(WindowTrayItem::<R>(Mutex::new(None)));

    let (monitors, primary) = monitor_rects(app.handle());
    let stored = {
        let st = app.state::<crate::app_settings::SettingsState>();
        let s = st.inner.lock().expect("settings lock");
        s.window_geometry().cloned()
    };
    let (x, y, w, h) = resolve_geometry(stored.as_ref(), &monitors, primary);

    let win = WebviewWindowBuilder::new(
        app,
        APP_WINDOW_LABEL,
        // Dasselbe dist wie Overlay/Fenster — der Frontend-Zweig erkennt den
        // Fenstermodus über das Label (Modul-Doc oben: warum kein `#` im Pfad).
        WebviewUrl::App("index.html".into()),
    )
    .title("SC Architects Overlay")
    .decorations(false) // eigenes Titelband (Variante B) — Frontend-Chrome
    .transparent(false) // NORMALES Fenster (Kontrastvertrag zum Spotlight)
    .resizable(true)
    .minimizable(true)
    .maximizable(true)
    .closable(true)
    .skip_taskbar(false)
    .always_on_top(false) // Fenster lebt unter normalen Apps (Risiko R3)
    .inner_size(w as f64, h as f64)
    .position(x as f64, y as f64)
    .visible(false) // nur nach Tray/Toggle — kein Auto-Show beim Start
    .build()?;

    // Lifecycle-Handler (M1 Scope 3+4): CloseRequested → nur-ins-Tray-hide;
    // Moved/Resized → debounced Geometrie-Persistenz. Nach dem Build
    // registriert (WebviewWindowBuilder hat keinen on_window_event-Builder-
    // Haken in dieser Tauri-Version).
    let handler = win.clone();
    win.on_window_event(move |event| match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            // M1 Scope 3: Schließen = nur ins Tray (§5). Das Beenden leistet
            // ausschließlich window_quit (Menü) / Tray-Quit.
            api.prevent_close();
            let _ = handler.hide();
            sync_tray_check(handler.app_handle(), false);
            emit_visibility(handler.app_handle(), false);
        }
        // M1 Scope 4: Geometrie debounced persistieren (Moved feuert pro
        // Frame beim Ziehen — hier nur Zwischespeicher, Persist macht der
        // Debounce-Thread).
        tauri::WindowEvent::Moved(pos) => {
            record_geometry(&handler, pos.x, pos.y);
        }
        tauri::WindowEvent::Resized(_) => {
            let pos = handler.outer_position().unwrap_or_default();
            record_geometry(&handler, pos.x, pos.y);
        }
        _ => {}
    });

    log::info!(
        "app-window created (hidden) at {x},{y} {w}x{h} — restored={:?}",
        stored.is_some()
    );
    let _ = win;
    Ok(())
}

/// Geometrie des Events in den Pending-Speicher legen und den Debounce-
/// Persist-Thread (einmalig) anstoßen.
fn record_geometry<R: Runtime>(window: &WebviewWindow<R>, x: i32, y: i32) {
    let app = window.app_handle();
    let size = window.outer_size().unwrap_or_default();
    let Some(pending) = app.try_state::<PendingGeometry>() else {
        return;
    };
    *pending.pending.lock().expect("pending lock") = Some(settings::WindowGeometry {
        x,
        y,
        w: size.width,
        h: size.height,
        monitor: window
            .current_monitor()
            .ok()
            .flatten()
            .and_then(|m| m.name().map(|n| n.to_string()))
            .unwrap_or_default(),
    });
    if pending
        .save_scheduled
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return; // ein Debounce-Thread läuft bereits — er nimmt den jüngsten Stand
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(GEOMETRY_SAVE_DEBOUNCE_MS));
        let Some(pending) = app.try_state::<PendingGeometry>() else {
            return;
        };
        let geo = pending.pending.lock().expect("pending lock").take();
        pending.save_scheduled.store(false, Ordering::Release);
        let Some(geo) = geo else { return };
        let Some(state) = app.try_state::<crate::app_settings::SettingsState>() else {
            return;
        };
        let mut s = state.inner.lock().expect("settings lock");
        s.set_window_geometry(Some(geo));
        crate::app_settings::persist_settings(&s);
    });
}

/// Tray-Checkbox nachziehen (Best-Effort — Tray darf fehlen, fail-soft).
fn sync_tray_check<R: Runtime>(app: &AppHandle<R>, checked: bool) {
    let Some(item) = app.try_state::<WindowTrayItem<R>>() else {
        return;
    };
    if let Some(check) = item.0.lock().expect("tray item lock").as_ref() {
        let _ = check.set_checked(checked);
    }
}

/// Fenstermodus-Sichtbarkeits-DTO (Checkbox-Zustand des Tray-Menüpunkts).
#[derive(Debug, Serialize)]
pub struct WindowState {
    pub visible: bool,
}

#[tauri::command]
pub fn window_state<R: Runtime>(app: AppHandle<R>) -> WindowState {
    WindowState {
        visible: app
            .get_webview_window(APP_WINDOW_LABEL)
            .map(|w| w.is_visible().unwrap_or(false))
            .unwrap_or(false),
    }
}

/// M3 (t_d9c09acf Scope 2): Sichtbarkeits-Event ans app-window-Frontend.
/// Der Titelband-Mode-Umschalter muss nach Tray-Re-Show den echten Zustand
/// zeigen (Reviewer M1-R1 Befund a): ohne Event bleibt 'overlay' stehen,
/// obwohl das Fenster sichtbar ist. Best-Effort — Fenster kann fehlen.
fn emit_visibility<R: Runtime>(app: &AppHandle<R>, visible: bool) {
    use tauri::Emitter;
    let _ = app.emit_to(APP_WINDOW_LABEL, "ao:window-visibility", visible);
}

/// Fenster zeigen (Tray/Toggle-Pfad) + Fokus. Trägt die Checkbox an.
pub fn show<R: Runtime>(app: &AppHandle<R>) {
    if let Some(win) = app.get_webview_window(APP_WINDOW_LABEL) {
        let _ = win.show();
        let _ = win.set_focus();
        sync_tray_check(app, true);
        emit_visibility(app, true);
    }
}

/// Fenster verstecken (Tray-Pfad / Mode-Umschalter „Overlay“).
pub fn hide<R: Runtime>(app: &AppHandle<R>) {
    if let Some(win) = app.get_webview_window(APP_WINDOW_LABEL) {
        let _ = win.hide();
        sync_tray_check(app, false);
        emit_visibility(app, false);
    }
}

/// „Fenstermodus zeigen/verstecken“ — der Tray-Menüpunkt (M1 Scope 5).
pub fn toggle_visible<R: Runtime>(app: &AppHandle<R>) {
    let visible = app
        .get_webview_window(APP_WINDOW_LABEL)
        .map(|w| w.is_visible().unwrap_or(false))
        .unwrap_or(false);
    if visible {
        hide(app);
    } else {
        show(app);
    }
}

#[tauri::command]
pub fn window_show<R: Runtime>(app: AppHandle<R>) {
    show(&app);
}

#[tauri::command]
pub fn window_hide<R: Runtime>(app: AppHandle<R>) {
    hide(&app);
}

#[tauri::command]
pub fn window_toggle<R: Runtime>(app: AppHandle<R>) {
    toggle_visible(&app);
}

/// Titelband-Drag (M1 Scope 2): das ganze Band ist der Griff — das Frontend
/// ruft dieses Kommando auf mousedown auf der Bandfläche (ohne Buttons).
#[tauri::command]
pub fn window_start_drag<R: Runtime>(app: AppHandle<R>) {
    if let Some(win) = app.get_webview_window(APP_WINDOW_LABEL) {
        let _ = win.start_dragging();
    }
}

/// Minimieren nativ — wo die Desktopumgebung es zulässt (Wayland-Caveat,
/// Sonde §5: dokumentierte Limitation, kein Sondercode).
#[tauri::command]
pub fn window_minimize<R: Runtime>(app: AppHandle<R>) {
    if let Some(win) = app.get_webview_window(APP_WINDOW_LABEL) {
        let _ = win.minimize();
    }
}

/// Beenden aus dem Fenster-Menü (roter Punkt, mockup-fenstermodus-b.html):
/// derselbe saubere Exit wie der Tray-Quit — `app.exit(0)`.
#[tauri::command]
pub fn window_quit<R: Runtime>(app: AppHandle<R>) {
    app.exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(name: &str, x: i32, y: i32, w: u32, h: u32) -> MonitorRect {
        MonitorRect {
            name: name.into(),
            x,
            y,
            w,
            h,
        }
    }

    /// Sebo-Beschluss §9.1.2: Default einmalig — 1200×800 zentriert auf dem
    /// Primärmonitor (Listenindex 0), bei kleinerem Monitor geclampt.
    #[test]
    fn default_geometry_centers_on_primary_monitor() {
        let dp3 = m("DP-3", 0, 0, 3440, 1440);
        let hdmi = m("HDMI-A-2", 3440, -1920, 1080, 1920);
        let (x, y, w, h) = resolve_geometry(None, &[dp3.clone(), hdmi], 0);
        assert_eq!((w, h), (1200, 800));
        assert_eq!(x, (3440 - 1200) / 2); // 1120
        assert_eq!(y, (1440 - 800) / 2); // 320

        // Kleinerer Primärmonitor: Fenster wird geclampt statt überstehend.
        let small = m("eDP-1", 0, 0, 1000, 700);
        let (x, y, w, h) = resolve_geometry(None, &[small], 0);
        assert_eq!((x, y, w, h), (0, 0, 1000, 700));
    }

    /// Sebo-Beschluss §9.1.3: letzter Stand gewinnt — exakte x/y/w/h inkl.
    /// negativer Offsets (Monitor über/links), wenn der Monitor noch existiert.
    #[test]
    fn stored_geometry_wins_while_monitor_exists() {
        let dp3 = m("DP-3", 0, 0, 3440, 1440);
        let hdmi = m("HDMI-A-2", 3440, -1920, 1080, 1920);
        let stored = settings::WindowGeometry {
            x: 3600,
            y: -1800,
            w: 900,
            h: 700,
            monitor: "HDMI-A-2".into(),
        };
        let (x, y, w, h) = resolve_geometry(Some(&stored), &[dp3, hdmi], 0);
        assert_eq!((x, y, w, h), (3600, -1800, 900, 700));
    }

    /// §9.1.3: Monitor weg → Primärmonitor zentriert (kein unsichtbares
    /// Fenster auf toter Geometrie).
    #[test]
    fn vanished_monitor_falls_back_to_primary_centered() {
        let dp3 = m("DP-3", 0, 0, 3440, 1440);
        let stored = settings::WindowGeometry {
            x: 3600,
            y: -1800,
            w: 900,
            h: 700,
            monitor: "HDMI-A-2".into(), // existiert nicht mehr
        };
        let (x, y, w, h) = resolve_geometry(Some(&stored), &[dp3], 0);
        assert_eq!((x, y), (1120, 320));
        assert_eq!((w, h), (1200, 800));
    }

    /// Persistierte Übergröße wird auf den (existierenden) Monitor geclampt —
    /// die Position bleibt, nur w/h schrumpfen auf die Monitor-Box.
    #[test]
    fn oversized_geometry_clamps_to_existing_monitor() {
        let dp3 = m("DP-3", 0, 0, 3440, 1440);
        let stored = settings::WindowGeometry {
            x: 10,
            y: 20,
            w: 6000,
            h: 4000,
            monitor: "DP-3".into(),
        };
        let (x, y, w, h) = resolve_geometry(Some(&stored), &[dp3], 0);
        assert_eq!((x, y, w, h), (10, 20, 3440, 1440));
    }
}
