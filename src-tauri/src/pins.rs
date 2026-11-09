//! M4 (t_7bf507e6): Dauerhafte Shellboxen „Pins“.
//!
//! Verträge (docs/FENSTERMODUS-PLAN.md §6 + §9.1.4–6 — bindend):
//!
//! - **Träger:** EIN dediziertes WebviewWindow pro Screen mit gepinnten
//!   Boxen (Sonde §6 Option A): Label `pins-<idx>`, decorations=false,
//!   transparent, always_on_top, skip_taskbar — NICHT pro Pin ein Fenster.
//! - **Sichtbarkeit: strikt an `game_running` gekoppelt** (§9.1.5) — Pins
//!   haben KEINEN Sichtbarkeitskanal ohne laufendes SC. Der Tray-Schalter
//!   (`pins_toggle`, Checkbox „Pin-Boxen anzeigen“) ist eine NECESSARY-
//!   Bedingung, nie hinreichend: effektiv sichtbar = enabled && pinned &&
//!   game_running.
//! - **Interaktion:** anzeige-only, solange das Overlay NICHT sichtbar ist
//!   (Pin-Fenster dann `set_ignore_cursor_events(true)`). Ist das Overlay
//!   sichtbar, dürfen Pins interagierbar sein (§9.1.6) — der Input-Modus
//!   des Pin-Fensters wechselt mit dem Overlay-Zustand, ganzer Fenster-
//!   Modus (kein per-Region — tao/Wayland kann keine Input-Shapes).
//! - **Persistenz:** Pin-Eintrag `{view_id, screen, box_index, opacity,
//!   pinned}` (+ Box-Position x/y + Anzeige-Snapshot) in app_settings
//!   (`[pins]`-Sektion), Restore beim Start — effektiv sichtbar erst bei
//!   game_running. Deckkraft pro Box, CSS opacity auf dem INHALT (Range
//!   20–100 %, Presets 20/55/100 — clamp in `clamp_opacity`).
//! - **Placement:** Box-Standardbreite 356px, Edge-Snapping L/R/T/B in
//!   `snap_to_edge` (Drag nur im Overlay-sichtbaren Zustand — der Input-
//!   Modus ist im unsichtbaren Overlay-Zustand gesperrt, daraus folgt die
//!   Drag-Beschränkung automatisch).
//! - **Universalität (Schema-U):** die Pin-Verwaltung ist rein mechanisch
//!   ({view_id, screen, box_index}); Plugin-Semantik kommt NUR aus dem
//!   Anzeige-Snapshot, den die View-Komponente des Overlay-Frontends beim
//!   Anheften erzeugt (generische Display-Zeilen). KEIN SCLM-Vokabular.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::webview::WebviewWindowBuilder;
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindow};

/// Box-Standardbreite (Pins-Vertrag).
pub const PIN_BOX_W: u32 = 356;

/// Deckkraft-Range (Pins-Vertrag): 20–100 %.
pub const OPACITY_MIN: u32 = 20;
pub const OPACITY_MAX: u32 = 100;

/// Edge-Snapping-Mindestabstand zum Screen-Rand (px).
pub const EDGE_MARGIN: i32 = 8;

/// Label-Präfix der Pin-Fenster (`pins-0`, `pins-1`, … — Index in die
/// Monitor-Liste). Frontend-Zweig erkennt am Präfix (`data-window="pins"`).
pub const PINS_LABEL_PREFIX: &str = "pins-";

/// Tray-Checkbox-Handle (vom tray::init gesetzt) — Zustand zieht
/// `pins_set_enabled`/init live nach (Muster wie WindowTrayItem).
pub struct PinsTrayItem<R: Runtime>(pub Mutex<Option<tauri::menu::CheckMenuItem<R>>>);

/// In-memory Zustandsflag (vom Backend-Logikpfad gesetzt), damit die
/// Tray-Checkbox und `pins_state` denselben Stand sehen.
#[derive(Default)]
pub struct PinsEnabled(AtomicBool);

/// Ereignis ans Pin-Fenster-Frontend: Pin-Set hat sich geändert.
pub const PINS_CHANGED_EVENT: &str = "ao:pins-changed";

// ---------------------------------------------------------------------------
// Pure Logik (unit-getestet ohne App)
// ---------------------------------------------------------------------------

/// Deckkraft clampen auf 20–100 % (Pins-Vertrag; Presets 20/55/100).
pub fn clamp_opacity(pct: u32) -> u32 {
    pct.clamp(OPACITY_MIN, OPACITY_MAX)
}

/// Effective Sichtbarkeit der Pins (reine Logik, game_running-Vertrag
/// §9.1.5): enabled (Tray-Schalter) UND mindestens ein aktiver Pin UND
/// das Spiel läuft. Ohne laufendes SC gibt es KEINEN Kanal.
pub fn pins_effective_visible(enabled: bool, pinned_count: usize, game_running: bool) -> bool {
    enabled && pinned_count > 0 && game_running
}

/// Screen-Bitmap für die Pin-Fenster-Verwaltung: Monitor-Name → Index.
/// Primärmonitor an Index 0 (gleiche Konvention wie window::monitor_rects).
pub fn screen_index_of(monitors: &[MonitorName], name: &str) -> Option<usize> {
    monitors.iter().position(|m| m.0 == name)
}

/// Monitor-Name (Wrapper für Tests/Docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorName(pub String);

/// Label des Pin-Fensters für den n-ten Screen.
pub fn label_for_screen(idx: usize) -> String {
    format!("{PINS_LABEL_PREFIX}{idx}")
}

/// Screen-Index aus einem Pin-Fenster-Label (None bei fremden Labels).
pub fn screen_of_label(label: &str) -> Option<usize> {
    label
        .strip_prefix(PINS_LABEL_PREFIX)
        .and_then(|rest| rest.parse::<usize>().ok())
}

/// Edge-Snapping L/R/T/B (Pins-Vertrag): die Box wird auf den Screen
/// zurückgezogen, wenn sie über einen Rand hinausragt. `x`/`y` sind
/// screen-relative Koordinaten (linke obere Box-Ecke), `w`/`h` die
/// Box-Größe, `screen_w`/`screen_h` die Screen-Auflösung.
///
/// Regel: erst horizontale Ränder (L dann R), dann vertikale (T dann B).
/// Eine Box breiter als der Screen wird auf den Ursprung gelegt.
pub fn snap_to_edge(x: i32, y: i32, w: i32, h: i32, screen_w: i32, screen_h: i32) -> (i32, i32) {
    let w = w.max(0).min(screen_w.max(0));
    let h = h.max(0).min(screen_h.max(0));
    let max_x = screen_w - w - EDGE_MARGIN;
    let max_y = screen_h - h - EDGE_MARGIN;
    let x = if x < EDGE_MARGIN {
        EDGE_MARGIN
    } else if x > max_x.max(EDGE_MARGIN) {
        max_x.max(EDGE_MARGIN)
    } else {
        x
    };
    let y = if y < EDGE_MARGIN {
        EDGE_MARGIN
    } else if y > max_y.max(EDGE_MARGIN) {
        max_y.max(EDGE_MARGIN)
    } else {
        y
    };
    (x, y)
}

/// Default-Placement einer frisch gepinnten Box: oberste freie Position am
/// rechten Screen-Rand — gestapelt über die schon vergebenen y-Positionen
/// der anderen Boxen desselben Screens (`occupied_heights` = Summe der
/// bisherigen Boxhöhen), vertikaler Stapel von oben. Reihenfolge egal,
/// solange kein Loch entsteht (einfacher Stapel).
pub fn default_placement(screen_w: i32, screen_h: i32, occupied_heights: &[i32]) -> (i32, i32) {
    let x = screen_w - PIN_BOX_W as i32 - EDGE_MARGIN;
    let stacked: i32 = occupied_heights.iter().sum();
    let y = EDGE_MARGIN + stacked;
    // Reicht der Stack über den Screen hinaus → wieder oben beginnen
    // (einfacher Wrap, der Stapel bleibt lesbar).
    let y = if y > screen_h - EDGE_MARGIN {
        EDGE_MARGIN
    } else {
        y
    };
    (x.max(EDGE_MARGIN), y)
}

// ---------------------------------------------------------------------------
// App-Laufzeit
// ---------------------------------------------------------------------------

/// Erzeugt die Pin-Fenster (hidden) für alle Screens, auf denen aktive
/// Pins existieren, und verwaltet Sichtbarkeit/Input-Modus. Kein Fenster
/// ohne Nutzungs-Historie (R1: zweite Webviews nur bei Bedarf).
pub fn init<R: Runtime>(app: &tauri::App<R>) -> tauri::Result<()> {
    app.manage(PinsTrayItem::<R>(Mutex::new(None)));
    let enabled = {
        let st = app.state::<crate::app_settings::SettingsState>();
        let s = st.inner.lock().expect("settings lock");
        s.pins().map(|p| p.enabled).unwrap_or(false)
    };
    app.manage(PinsEnabled(AtomicBool::new(enabled)));
    ensure_pin_windows(app.handle())?;
    Ok(())
}

/// Monitor-Liste als Namen (Primärmonitor zuerst — window::monitor_rects
/// hält den Primärmonitor an Index 0; hier eigenständig enumerieren,
/// damit pins.rs ohne Cross-Modul-Abhängigkeit testbar bleibt).
fn monitor_names<R: Runtime>(app: &AppHandle<R>) -> Vec<MonitorName> {
    let primary = app
        .primary_monitor()
        .ok()
        .flatten()
        .and_then(|m| m.name().map(|n| n.to_string()));
    let mut list: Vec<MonitorName> = app
        .available_monitors()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|m| m.name().map(|n| MonitorName(n.to_string())))
        .collect();
    if let Some(p) = primary.as_ref()
        && let Some(pos) = list.iter().position(|m| &m.0 == p)
        && pos > 0
    {
        list.swap(0, pos);
    }
    list
}

/// Stellt sicher, dass für jeden Screen mit aktiven Pins ein Fenster
/// existiert. Idempotent — bestehende Fenster bleiben unangetastet.
fn ensure_pin_windows<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let screens = {
        let st = app.state::<crate::app_settings::SettingsState>();
        let s = st.inner.lock().expect("settings lock");
        let mut set: Vec<String> = s
            .pins()
            .map(|p| {
                p.list
                    .iter()
                    .filter(|e| e.pinned)
                    .map(|e| e.screen.clone())
                    .collect()
            })
            .unwrap_or_default();
        set.sort();
        set.dedup();
        set
    };
    let names = monitor_names(app);
    for screen in screens {
        let Some(idx) = screen_index_of(&names, &screen) else {
            log::info!("pins: screen {screen:?} nicht mehr vorhanden — übersprungen");
            continue;
        };
        let label = label_for_screen(idx);
        if app.get_webview_window(&label).is_some() {
            continue;
        }
        WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
            .title("Pins")
            .decorations(false)
            .transparent(true)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .closable(false)
            .skip_taskbar(true)
            .always_on_top(true)
            .shadow(false)
            .focused(false)
            .visible(false) // Sichtbarkeit NUR über game_running (§9.1.5)
            .build()?;
        log::info!("pins: Fenster {label} erzeugt (hidden) für Screen {screen:?}");
    }
    Ok(())
}

/// Alle Pin-Fenster (Labels nach PINS_LABEL_PREFIX).
fn pin_windows<R: Runtime>(app: &AppHandle<R>) -> Vec<WebviewWindow<R>> {
    let mut out: Vec<WebviewWindow<R>> = Vec::new();
    let windows = app.webview_windows();
    let mut labels: Vec<String> = windows
        .keys()
        .filter(|l| l.starts_with(PINS_LABEL_PREFIX))
        .cloned()
        .collect();
    labels.sort();
    for l in labels {
        if let Some(w) = app.get_webview_window(&l) {
            out.push(w);
        }
    }
    out
}

/// Zentrale Sichtbarkeits-/Input-Synchronisation (vom Overlay-/Game-Pfad
/// gerufen): zeigt Pin-Fenster nur bei effektiv sichtbaren Pins und
/// setzt den Input-Modus (§9.1.6: interagierbar NUR bei sichtbarem
/// Overlay; sonst click-through/ignore).
pub fn sync_all<R: Runtime>(app: &AppHandle<R>) {
    let (enabled, pinned_count, game_running, overlay_visible) = {
        let st = app.state::<crate::app_settings::SettingsState>();
        let s = st.inner.lock().expect("settings lock");
        let pinned = s
            .pins()
            .map(|p| p.list.iter().filter(|e| e.pinned).count())
            .unwrap_or(0);
        let enabled = app
            .try_state::<PinsEnabled>()
            .map(|e| e.0.load(Ordering::SeqCst))
            .unwrap_or(false);
        (
            enabled,
            pinned,
            crate::overlay::snapshot(app)
                .map(|o| o.game_running)
                .unwrap_or(false),
            crate::overlay::snapshot(app)
                .map(|o| o.visible)
                .unwrap_or(false),
        )
    };
    let visible = pins_effective_visible(enabled, pinned_count, game_running);
    for win in pin_windows(app) {
        let _ = win.set_ignore_cursor_events(!overlay_visible || !visible);
        if visible {
            // Position: Fenster deckt den ganzen Screen ab — Placement
            // macht das Frontend in CSS (persistierte x/y der Boxen).
            if let Some(m) = win.current_monitor().ok().flatten() {
                let pos = m.position();
                let size = m.size();
                let _ = win.set_position(tauri::PhysicalPosition::new(pos.x, pos.y));
                let _ = win.set_size(tauri::PhysicalSize::new(size.width, size.height));
            }
            let _ = win.show();
        } else {
            let _ = win.hide();
        }
        // Input-Modus ans Frontend melden (nur Cursor-Kontrast — die echte
        // Gate ist das set_ignore_cursor_events oben).
        let _ =
            tauri::Emitter::emit_to(app, win.label(), "ao:pins-mode", overlay_visible && visible);
    }
    sync_tray_check(app, enabled);
}

/// Tray-Checkbox nachziehen (fail-soft — Tray darf fehlen).
fn sync_tray_check<R: Runtime>(app: &AppHandle<R>, checked: bool) {
    if let Some(item) = app.try_state::<PinsTrayItem<R>>()
        && let Some(check) = item.0.lock().expect("tray item lock").as_ref()
    {
        let _ = check.set_checked(checked);
    }
}

/// game_running-Wechsel (aus overlay::set_game_running): Pins ziehen
/// strikt mit (§9.1.5 — kein eigener Sichtbarkeitskanal).
pub fn sync_game_running<R: Runtime>(app: &AppHandle<R>, _running: bool) {
    if app.try_state::<PinsEnabled>().is_some() {
        sync_all(app);
    }
}

/// Overlay-Sichtbarkeitswechsel (aus overlay::apply_visibility): wechselt
/// den Input-Modus der Pin-Fenster mit (§9.1.6).
pub fn sync_overlay_visibility<R: Runtime>(app: &AppHandle<R>, overlay_visible: bool) {
    if app.try_state::<PinsEnabled>().is_none() {
        return;
    }
    let visible = pins_effective_visible(
        app.try_state::<PinsEnabled>()
            .map(|e| e.0.load(Ordering::SeqCst))
            .unwrap_or(false),
        pinned_count(app),
        crate::overlay::snapshot(app)
            .map(|o| o.game_running)
            .unwrap_or(false),
    );
    if !visible {
        return; // unsichtbare Pins haben keinen Input-Modus
    }
    for win in pin_windows(app) {
        let _ = win.set_ignore_cursor_events(!overlay_visible);
    }
}

fn pinned_count<R: Runtime>(app: &AppHandle<R>) -> usize {
    let st = app.state::<crate::app_settings::SettingsState>();
    let s = st.inner.lock().expect("settings lock");
    s.pins()
        .map(|p| p.list.iter().filter(|e| e.pinned).count())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Tauri-Commands (Overlay-Frontend + Pin-Fenster-Frontend)
// ---------------------------------------------------------------------------

/// DTO für das Frontend (Mirror von settings::PinEntry).
#[derive(Debug, Serialize, Clone, serde::Deserialize)]
pub struct PinDto {
    pub view_id: String,
    pub screen: String,
    pub box_index: u32,
    pub opacity: u32,
    pub pinned: bool,
    pub x: i32,
    pub y: i32,
    pub title: String,
    pub kpi: Option<String>,
    pub rows: Vec<PinRowDto>,
}

#[derive(Debug, Serialize, Clone, serde::Deserialize)]
pub struct PinRowDto {
    pub idx: String,
    pub name: String,
    pub sub: Option<String>,
    pub value: String,
    pub side: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PinsState {
    pub enabled: bool,
    pub game_running: bool,
    pub overlay_visible: bool,
    pub pins: Vec<PinDto>,
}

fn to_dto(e: &settings::PinEntry) -> PinDto {
    PinDto {
        view_id: e.view_id.clone(),
        screen: e.screen.clone(),
        box_index: e.box_index,
        opacity: e.opacity,
        pinned: e.pinned,
        x: e.x,
        y: e.y,
        title: e.title.clone(),
        kpi: e.kpi.clone(),
        rows: e
            .rows
            .iter()
            .map(|r| PinRowDto {
                idx: r.idx.clone(),
                name: r.name.clone(),
                sub: r.sub.clone(),
                value: r.value.clone(),
                side: r.side.clone(),
            })
            .collect(),
    }
}

/// Screen-Name eines (Pin-)Fensters über sein Label bestimmen.
fn screen_name_for<R: Runtime>(app: &AppHandle<R>, label: &str) -> Option<String> {
    let idx = screen_of_label(label)?;
    let names = monitor_names(app);
    names.get(idx).map(|m| m.0.clone())
}

#[tauri::command]
pub fn pins_state<R: Runtime>(app: AppHandle<R>) -> PinsState {
    let st = app.state::<crate::app_settings::SettingsState>();
    let s = st.inner.lock().expect("settings lock");
    let snap = crate::overlay::snapshot(&app);
    PinsState {
        enabled: app
            .try_state::<PinsEnabled>()
            .map(|e| e.0.load(Ordering::SeqCst))
            .unwrap_or(false),
        game_running: snap.as_ref().map(|o| o.game_running).unwrap_or(false),
        overlay_visible: snap.as_ref().map(|o| o.visible).unwrap_or(false),
        pins: s
            .pins()
            .map(|p| p.list.iter().map(to_dto).collect())
            .unwrap_or_default(),
    }
}

/// Pins für ein Pin-Fenster nach Label (nur der eigene Screen).
#[tauri::command]
pub fn pins_for_screen<R: Runtime>(app: AppHandle<R>, label: String) -> Vec<PinDto> {
    let Some(screen) = screen_name_for(&app, &label) else {
        return Vec::new();
    };
    let st = app.state::<crate::app_settings::SettingsState>();
    let s = st.inner.lock().expect("settings lock");
    s.pins()
        .map(|p| {
            p.list
                .iter()
                .filter(|e| e.pinned && e.screen == screen)
                .map(to_dto)
                .collect()
        })
        .unwrap_or_default()
}

/// Anheften aus dem Overlay-Frontend: mechanical Keys + Snapshot.
/// Der Screen ist der Monitor des Overlay-Fensters (das Overlay liegt
/// immer da, wo der Nutzer pinnt). Duplikate (view_id/screen/box_index,
/// aktiv) werden verhindert.
#[tauri::command]
pub fn pin_add<R: Runtime>(
    app: AppHandle<R>,
    view_id: String,
    box_index: u32,
    opacity: u32,
    title: String,
    kpi: Option<String>,
    rows: Vec<PinRowDto>,
) -> bool {
    let screen = app
        .get_webview_window(crate::overlay::SPOTLIGHT_LABEL)
        .and_then(|w| w.current_monitor().ok().flatten())
        .and_then(|m| m.name().map(|n| n.to_string()))
        .unwrap_or_default();
    if screen.is_empty() {
        return false;
    }
    let st = app.state::<crate::app_settings::SettingsState>();
    let mut s = st.inner.lock().expect("settings lock");
    let section = s.pins_mut_entry();
    if section
        .iter()
        .any(|e| e.pinned && e.view_id == view_id && e.screen == screen && e.box_index == box_index)
    {
        return false; // bereits angeheftet
    }
    // Placement: rechte Kante, gestapelt über die Höhen der vorhandenen
    // aktiven Boxen desselben Screens (Höhe ~ Kopf + Zeilen, grob).
    let heights: Vec<i32> = section
        .iter()
        .filter(|e| e.pinned && e.screen == screen)
        .map(|e| 44 + 34 * e.rows.len().max(1) as i32)
        .collect();
    let screen_size = app
        .get_webview_window(crate::overlay::SPOTLIGHT_LABEL)
        .and_then(|w| w.current_monitor().ok().flatten())
        .map(|m| (m.size().width as i32, m.size().height as i32));
    let (sw, sh) = screen_size.unwrap_or((1920, 1080));
    let (x, y) = default_placement(sw, sh, &heights);
    section.push(settings::PinEntry {
        view_id,
        screen,
        box_index,
        opacity: clamp_opacity(opacity),
        pinned: true,
        x,
        y,
        title,
        kpi,
        rows: rows
            .into_iter()
            .map(|r| settings::PinRow {
                idx: r.idx,
                name: r.name,
                sub: r.sub,
                value: r.value,
                side: r.side,
            })
            .collect(),
    });
    crate::app_settings::persist_settings(&s);
    drop(s);
    ensure_pin_windows(&app).ok();
    notify_changed(&app);
    sync_all(&app);
    true
}

/// „Lösen“: pinned=false (Eintrag bleibt, Restore überspringt ihn).
#[tauri::command]
pub fn pin_unpin<R: Runtime>(
    app: AppHandle<R>,
    view_id: String,
    screen: String,
    box_index: u32,
) -> bool {
    let changed = {
        let st = app.state::<crate::app_settings::SettingsState>();
        let mut s = st.inner.lock().expect("settings lock");
        let list = s.pins_mut_entry();
        let mut hit = false;
        for e in list.iter_mut() {
            if e.view_id == view_id && e.screen == screen && e.box_index == box_index && e.pinned {
                e.pinned = false;
                hit = true;
            }
        }
        if hit {
            crate::app_settings::persist_settings(&s);
        }
        hit
    };
    if changed {
        notify_changed(&app);
        sync_all(&app);
    }
    changed
}

/// „Schließen“: Pin-Eintrag entfernt (Plugin/View läuft weiter).
#[tauri::command]
pub fn pin_remove<R: Runtime>(
    app: AppHandle<R>,
    view_id: String,
    screen: String,
    box_index: u32,
) -> bool {
    let removed = {
        let st = app.state::<crate::app_settings::SettingsState>();
        let mut s = st.inner.lock().expect("settings lock");
        let list = s.pins_mut_entry();
        let before = list.len();
        list.retain(|e| !(e.view_id == view_id && e.screen == screen && e.box_index == box_index));
        let removed = list.len() != before;
        if removed {
            crate::app_settings::persist_settings(&s);
        }
        removed
    };
    if removed {
        notify_changed(&app);
        sync_all(&app);
    }
    removed
}

/// Deckkraft setzen (20–100, clamp).
#[tauri::command]
pub fn pin_set_opacity<R: Runtime>(
    app: AppHandle<R>,
    view_id: String,
    screen: String,
    box_index: u32,
    opacity: u32,
) -> bool {
    let st = app.state::<crate::app_settings::SettingsState>();
    let mut s = st.inner.lock().expect("settings lock");
    let clamped = clamp_opacity(opacity);
    let list = s.pins_mut_entry();
    let mut hit = false;
    for e in list.iter_mut() {
        if e.view_id == view_id && e.screen == screen && e.box_index == box_index && e.pinned {
            e.opacity = clamped;
            hit = true;
        }
    }
    if hit {
        crate::app_settings::persist_settings(&s);
    }
    hit
}

/// Drag-Ende: Position zurückmelden → Edge-Snapping + Persistenz
/// (x/y screen-relativ). Drag ist nur im Overlay-sichtbaren Zustand
/// möglich — das Frontend ruft das nur dann auf.
#[tauri::command]
pub fn pin_move<R: Runtime>(
    app: AppHandle<R>,
    view_id: String,
    screen: String,
    box_index: u32,
    drag: PinDrag,
) -> bool {
    let (sx, sy) = snap_to_edge(drag.x, drag.y, drag.w, drag.h, drag.screen_w, drag.screen_h);
    let st = app.state::<crate::app_settings::SettingsState>();
    let mut s = st.inner.lock().expect("settings lock");
    let list = s.pins_mut_entry();
    let mut hit = false;
    for e in list.iter_mut() {
        if e.view_id == view_id && e.screen == screen && e.box_index == box_index && e.pinned {
            e.x = sx;
            e.y = sy;
            hit = true;
        }
    }
    if hit {
        crate::app_settings::persist_settings(&s);
    }
    hit
}

/// Drag-Geometrie (gebündelt, statt 7 Einzelargumenten — clippy
/// too_many_arguments an Tauri-Command-Fn-Signaturen).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PinDrag {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub screen_w: i32,
    pub screen_h: i32,
}

/// Snapshot-Refresh (Overlay-Datenreload): Overlays melden frische
/// Anzeige-Daten für bereits gepinnte Boxen — generisch, view-lokal.
#[tauri::command]
pub fn pin_update<R: Runtime>(
    app: AppHandle<R>,
    view_id: String,
    box_index: u32,
    title: String,
    kpi: Option<String>,
    rows: Vec<PinRowDto>,
) -> bool {
    let st = app.state::<crate::app_settings::SettingsState>();
    let mut s = st.inner.lock().expect("settings lock");
    let list = s.pins_mut_entry();
    let mut hit = false;
    for e in list.iter_mut() {
        if e.pinned && e.view_id == view_id && e.box_index == box_index {
            e.title = title.clone();
            e.kpi = kpi.clone();
            e.rows = rows
                .iter()
                .map(|r| settings::PinRow {
                    idx: r.idx.clone(),
                    name: r.name.clone(),
                    sub: r.sub.clone(),
                    value: r.value.clone(),
                    side: r.side.clone(),
                })
                .collect();
            hit = true;
        }
    }
    if hit {
        crate::app_settings::persist_settings(&s);
        drop(s);
        notify_changed(&app);
    }
    hit
}

/// Tray-Schalter „Pin-Boxen anzeigen“: enabled setzen + Sync.
#[tauri::command]
pub fn pins_set_enabled<R: Runtime>(app: AppHandle<R>, on: bool) {
    if let Some(st) = app.try_state::<PinsEnabled>() {
        st.0.store(on, Ordering::SeqCst);
    }
    {
        let st = app.state::<crate::app_settings::SettingsState>();
        let mut s = st.inner.lock().expect("settings lock");
        let entry = s.pins_mut_or_default();
        entry.enabled = on;
        crate::app_settings::persist_settings(&s);
    }
    ensure_pin_windows(&app).ok();
    notify_changed(&app);
    sync_all(&app);
}

/// Tray-Pfad (on_menu_event): Checkbox-Toggle.
pub fn tray_toggle<R: Runtime>(app: &AppHandle<R>) {
    let on = !app
        .try_state::<PinsEnabled>()
        .map(|e| e.0.load(Ordering::SeqCst))
        .unwrap_or(false);
    pins_set_enabled(app.clone(), on);
}

/// Event an alle Pin-Fenster: Pin-Set geändert (Pin-Fenster-Frontend
/// holt selbst nach via pins_for_screen).
fn notify_changed<R: Runtime>(app: &AppHandle<R>) {
    use tauri::Emitter;
    let windows = app.webview_windows();
    for (label, _) in windows
        .iter()
        .filter(|(l, _)| l.starts_with(PINS_LABEL_PREFIX))
    {
        let _ = app.emit_to(label.as_str(), PINS_CHANGED_EVENT, true);
    }
}

// ---------------------------------------------------------------------------
// Tests (reine Logik — game_running-Sichtbarkeitsregel, Clamp, Snapping,
// Placement, Label-Mapping)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// §9.1.5: Sichtbarkeit strikt an game_running gekoppelt.
    #[test]
    fn visibility_requires_game_running() {
        assert!(!pins_effective_visible(true, 3, false));
        assert!(pins_effective_visible(true, 3, true));
        // Tray aus → nie sichtbar, auch nicht mit Spiel.
        assert!(!pins_effective_visible(false, 3, true));
        // Keine aktiven Pins → nie sichtbar.
        assert!(!pins_effective_visible(true, 0, true));
    }

    /// Deckkraft-Range 20–100 % (Pins-Vertrag).
    #[test]
    fn opacity_clamps_to_contract_range() {
        assert_eq!(clamp_opacity(0), OPACITY_MIN);
        assert_eq!(clamp_opacity(19), OPACITY_MIN);
        assert_eq!(clamp_opacity(20), 20);
        assert_eq!(clamp_opacity(55), 55);
        assert_eq!(clamp_opacity(100), 100);
        assert_eq!(clamp_opacity(101), OPACITY_MAX);
        assert_eq!(clamp_opacity(5000), OPACITY_MAX);
    }

    /// Edge-Snapping L/R/T/B: Box bleibt vollständig auf dem Screen.
    #[test]
    fn snapping_pulls_box_back_inside() {
        // links raus → linker Rand
        assert_eq!(
            snap_to_edge(-50, 100, 356, 120, 1920, 1080),
            (EDGE_MARGIN, 100)
        );
        // rechts raus → rechter Rand (1920-356-8)
        assert_eq!(
            snap_to_edge(1900, 100, 356, 120, 1920, 1080),
            (1920 - 356 - EDGE_MARGIN, 100)
        );
        // oben raus
        assert_eq!(
            snap_to_edge(100, -5, 356, 120, 1920, 1080),
            (100, EDGE_MARGIN)
        );
        // unten raus
        assert_eq!(
            snap_to_edge(100, 1100, 356, 120, 1920, 1080),
            (100, 1080 - 120 - EDGE_MARGIN)
        );
        // innerhalb bleibt innerhalb
        assert_eq!(snap_to_edge(500, 500, 356, 120, 1920, 1080), (500, 500));
        // Box breiter als Screen → Ursprung (nach Margin)
        assert_eq!(
            snap_to_edge(50, 50, 2000, 120, 1920, 1080),
            (EDGE_MARGIN, 50)
        );
    }

    /// Default-Placement: rechte Kante, gestapelt; Wrap oben bei Overflow.
    #[test]
    fn default_placement_stacks_right_edge() {
        let (x, y) = default_placement(1920, 1080, &[]);
        assert_eq!(x, 1920 - 356 - EDGE_MARGIN);
        assert_eq!(y, EDGE_MARGIN);
        let (_, y2) = default_placement(1920, 1080, &[200]);
        assert_eq!(y2, EDGE_MARGIN + 200);
        // Overflow → wieder oben.
        let (_, y3) = default_placement(1080, 600, &[600]);
        assert_eq!(y3, EDGE_MARGIN);
        // Sehr schmaler Screen: x mindestens am linken Rand.
        let (x4, _) = default_placement(300, 600, &[]);
        assert_eq!(x4, EDGE_MARGIN);
    }

    /// Label-Mapping: pins-<idx> ↔ Screen-Index, fremde Labels → None.
    #[test]
    fn label_roundtrip_and_reject() {
        assert_eq!(label_for_screen(0), "pins-0");
        assert_eq!(label_for_screen(3), "pins-3");
        assert_eq!(screen_of_label("pins-2"), Some(2));
        assert_eq!(screen_of_label("spotlight"), None);
        assert_eq!(screen_of_label("app-window"), None);
        assert_eq!(screen_of_label("pins-x"), None);
        assert_eq!(screen_of_label("pins-"), None);
    }

    /// Screen-Index-Auflösung über Namen (Primärmonitor-Liste).
    #[test]
    fn screen_index_matches_names() {
        let m = [MonitorName("DP-3".into()), MonitorName("HDMI-A-2".into())];
        assert_eq!(screen_index_of(&m, "DP-3"), Some(0));
        assert_eq!(screen_index_of(&m, "HDMI-A-2"), Some(1));
        assert_eq!(screen_index_of(&m, "eDP-1"), None);
    }
}
