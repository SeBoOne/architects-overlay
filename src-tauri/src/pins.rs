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
//! - **Interaktion (§9.1.6, präzisiert M4-E t_34efb781):** das Pin-Fenster
//!   ist REIN ANZEIGE — IMMER `set_ignore_cursor_events(true)` (Konstante,
//!   kein Moduswechsel mit dem Overlay-Zustand). Die Interaktion läuft über
//!   unsichtbare PATCH-Flächen IM SPOTLIGHT-Webview (PinPatches.svelte:
//!   Drag → pin_move, Klick → Controller-Popover), die nur bei sichtbarem
//!   Overlay gerendert werden. Kein per-Region-Input-Shape (tao/Wayland
//!   kann das nicht) — der STICKING-CONFLICT (Spotlight hebt sich beim
//!   show() über gleichrangige always-on-top-Pin-Fenster) wird durch die
//!   Bauform gelöst: das Spotlight ist die EINZIGE interaktive Ebene.
//! - **Persistenz:** Pin-Eintrag `{view_id, screen, box_index, opacity,
//!   pinned}` (+ Box-Position x/y + Anzeige-Snapshot) in app_settings
//!   (`[pins]`-Sektion), Restore beim Start — effektiv sichtbar erst bei
//!   game_running. Deckkraft pro Box, CSS opacity auf dem INHALT (Range
//!   20–100 %, Presets 20/55/100 — clamp in `clamp_opacity`).
//! - **Placement:** Box-Standardbreite 356px, Edge-Snapping L/R/T/B in
//!   `snap_to_edge` (Drag nur im Overlay-sichtbaren Zustand — die Patch-
//!   Flächen im Spotlight existieren nur dann, daraus folgt die Drag-
//!   Beschränkung automatisch).
//! - **Universalität (Schema-U):** die Pin-Verwaltung ist rein mechanisch
//!   ({view_id, screen, box_index}); Plugin-Semantik kommt NUR aus dem
//!   Anzeige-Snapshot, den die View-Komponente des Overlay-Frontends beim
//!   Anheften erzeugt (generische Display-Zeilen). KEIN SCLM-Vokabular.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::webview::WebviewWindowBuilder;
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindow};

use crate::window::MonitorRect;

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

/// Input-Modus-Entscheidung (pure Logik, t_ef8222da / t_34efb781): für ein
/// sichtbares/gemapptes Pin-Fenster wird der neue Ignore-Modus
/// zurückgegeben; für ein hidden/nicht gemapptes Fenster `None` —
/// dann wird KEINE Window-Mutation gestellt. Grund: tao paniziert in
/// `WindowRequest::CursorIgnoreEvents` (`window.window().unwrap()`),
/// wenn das GTK-Window nicht existiert/nicht gemappt ist — der Request
/// darf auf versteckten Pin-Fenstern gar nicht erst in die Queue.
/// Muster §9.1.6: Input-Modus nur im Moment des show() setzen.
///
/// M4-E (t_34efb781): das Pin-Fenster ist REIN ANZEIGE (Konstante) —
/// immer `ignore=true` (input-transparent). Die Interaktion läuft über
/// unsichtbare Patch-Flächen im Spotlight-Webview; der Overlay-Zustand
/// spielt für den Input-Modus KEINE Rolle mehr (§9.1.6, präzisiert).
pub fn pin_input_mode(pin_visible: bool) -> Option<bool> {
    if !pin_visible {
        return None;
    }
    Some(true)
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

/// Unified Monitor-Enumeration (t_56262ccf — Monitor-Identitäts-Mismatch):
/// DIE EINE Identitäts-Quelle für Capture (pin_add), Lookup
/// (ensure_pin_windows/screen_name_for) und Geometrie (window::record_geometry).
///
/// Name-Quelle festgenagelt: Auf Hyprland liefert `hyprctl -j monitors` das
/// `name`-Feld (wl_output-Name, z. B. `DP-3`) als Source of Truth. Beleg
/// (Omar-Diagnose am laufenden System, 2026-11-10): hyprctl meldet
/// `name='DP-3', model='CU34G2XP'` — tauri `current_monitor().name()` lieferte
/// dort den MODEL-String (`CU34G2XP`), `available_monitors().name()` den
/// wl_output-Namen (`DP-3`) → 5 persistierte Pins mit screen='CU34G2XP'
/// lösten in screen_index_of nie auf. Die Rects beider Quellen liegen im
/// selben Hyprland-Layout-Raum (verifiziert t_317b9d15).
/// Ohne Hyprland (und auf Windows/macOS): tauri available_monitors() —
/// Primärmonitor an Index 0 (gleiche Konvention wie window::monitor_rects).
pub fn monitor_list<R: Runtime>(app: &AppHandle<R>) -> Vec<MonitorRect> {
    #[cfg(target_os = "linux")]
    if let Some(list) = hyprland_monitor_list() {
        return primary_first(app, list);
    }
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
    primary_first_in_place(app, &mut list);
    list
}

/// Stellt die Primärmonitor-zuerst-Konvention über die Namen her (in-place).
fn primary_first_in_place<R: Runtime>(app: &AppHandle<R>, list: &mut [MonitorRect]) {
    let primary = app
        .primary_monitor()
        .ok()
        .flatten()
        .and_then(|m| m.name().map(|n| n.to_string()));
    if let Some(p) = primary
        && let Some(pos) = list.iter().position(|m| m.name == p)
        && pos > 0
    {
        list.swap(0, pos);
    }
}

fn primary_first<R: Runtime>(app: &AppHandle<R>, mut list: Vec<MonitorRect>) -> Vec<MonitorRect> {
    primary_first_in_place(app, &mut list);
    list
}

/// Hyprland-Monitorliste via `hyprctl -j monitors` (nur Linux/Hyprland):
/// `name`-Feld als Identität (wl_output-Name — NICHT `model`/`description`,
/// genau das war der Mismatch). Rects sind Layout-Px wie available_monitors.
#[cfg(target_os = "linux")]
fn hyprland_monitor_list() -> Option<Vec<MonitorRect>> {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    let out = crate::overlay::hyprctl_limited(&["-j", "monitors"])?;
    if !out.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let arr = v.as_array()?;
    let mut list = Vec::new();
    for m in arr {
        let name = m.get("name")?.as_str()?.to_string();
        let x = m.get("x")?.as_i64()? as i32;
        let y = m.get("y")?.as_i64()? as i32;
        let w = m.get("width")?.as_i64()? as u32;
        let h = m.get("height")?.as_i64()? as u32;
        list.push(MonitorRect { name, x, y, w, h });
    }
    (!list.is_empty()).then_some(list)
}

/// Monitor-Identität über Geometrie auflösen (Capture-Pfad): exakte
/// Position+Größe zuerst, sonst der Monitor, der das Zentrum des Rects
/// enthält (Layout-Raum-Konsistenz t_317b9d15 — die Rects von
/// current_monitor und monitor_list stammen aus demselben Koordinatenraum,
/// nur der NAME divergiert auf Hyprland). Liefert den EINHEITLICHEN Namen.
pub fn monitor_name_by_rect(
    monitors: &[MonitorRect],
    rect: (i32, i32, u32, u32),
) -> Option<String> {
    let (rx, ry, rw, rh) = rect;
    if let Some(m) = monitors
        .iter()
        .find(|m| m.x == rx && m.y == ry && m.w == rw && m.h == rh)
    {
        return Some(m.name.clone());
    }
    let cx = rx + rw as i32 / 2;
    let cy = ry + rh as i32 / 2;
    monitors
        .iter()
        .find(|m| cx >= m.x && cx < m.x + m.w as i32 && cy >= m.y && cy < m.y + m.h as i32)
        .map(|m| m.name.clone())
}

/// Boot-Repair (pure, t_56262ccf Punkt 3): gepinnte Einträge mit nicht
/// auflösbarem Screen auf den Primärmonitor (Index 0) ummappen — einmalig,
/// der Aufrufer persistiert. Liefert die Anzahl der Repairs; leere
/// Monitorliste (sollte nicht passieren) repariert nichts.
pub fn repair_unresolvable_screens(
    entries: &mut [settings::PinEntry],
    monitors: &[MonitorRect],
) -> usize {
    let Some(primary) = monitors.first().map(|m| m.name.clone()) else {
        return 0;
    };
    let mut repaired = 0;
    for e in entries.iter_mut().filter(|e| e.pinned) {
        if !monitors.iter().any(|m| m.name == e.screen) {
            repaired += 1;
            e.screen = primary.clone();
        }
    }
    repaired
}

/// Rect des Monitors, auf dem ein Fenster aktuell lebt (physical). Nur die
/// Geometrie interessiert — der NAME von current_monitor() ist auf Hyprland
/// unzuverlässig (Model-String) und wird bewusst NICHT verwendet.
///
/// ACHTUNG (t_01f1a9ed): `current_monitor()` liefert None für ein NIE
/// gemapptes Fenster — der Pin-Pfad darf damit KEINE Geometrie mehr
/// auflösen (siehe `rect_for_screen`); der Helfer bleibt nur für den
/// Capture-Pfad in `pin_add` (Spotlight-Fenster ist dort bereits gemappt).
fn current_monitor_rect<R: Runtime>(win: &WebviewWindow<R>) -> Option<(i32, i32, u32, u32)> {
    let m = win.current_monitor().ok().flatten()?;
    let pos = m.position();
    let size = m.size();
    Some((pos.x, pos.y, size.width, size.height))
}

/// Geometrie-Quelle für Pin-Fenster (t_01f1a9ed): Rect aus der unified
/// Monitor-Enumeration über den Screen-Index IM LABEL — NICHT über
/// `win.current_monitor()` (liefert None vor dem ersten Map → Geometrie
/// wurde komplett übersprungen → show() mit Tauri-Default 800×600).
pub fn rect_for_screen(monitors: &[MonitorRect], idx: usize) -> Option<(i32, i32, u32, u32)> {
    monitors.get(idx).map(|m| (m.x, m.y, m.w, m.h))
}

/// Rect eines Pin-Fensters aus dem Label auflösen; nicht auflösbar →
/// Fallback Primärmonitor (Index 0) mit SICHTBAREM log::warn (gleiche
/// Fail-soft-Disziplin wie `screen_index_resolved_or_primary`).
fn pin_window_rect<R: Runtime>(
    monitors: &[MonitorRect],
    win: &WebviewWindow<R>,
) -> Option<(i32, i32, u32, u32)> {
    let idx = screen_of_label(win.label());
    let rect = idx.and_then(|i| rect_for_screen(monitors, i));
    if rect.is_some() {
        return rect;
    }
    log::warn!(
        "pins: Rect für Label {:?} nicht auflösbar — Fallback auf Primärmonitor",
        win.label()
    );
    rect_for_screen(monitors, 0)
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
    // Boot-Repair (t_56262ccf Punkt 3): Einträge mit nicht auflösbarem
    // Screen (z. B. persistierte Hyprland-Model-Strings 'CU34G2XP') einmalig
    // auf den Primärmonitor ummappen UND persistieren.
    let monitors = monitor_list(app.handle());
    let repaired = {
        let st = app.state::<crate::app_settings::SettingsState>();
        let mut s = st.inner.lock().expect("settings lock");
        let n = repair_unresolvable_screens(s.pins_mut_entry(), &monitors);
        if n > 0 {
            crate::app_settings::persist_settings(&s);
            log::warn!("pins: Boot-Repair: {n} Pin-Einträge auf Primärmonitor umgemappt");
        }
        n
    };
    if repaired == 0 && monitors.is_empty() {
        log::warn!("pins: keine Monitore enumerierbar — Pins bleiben unaufgelöst");
    }
    ensure_pin_windows(app.handle())?;
    // Boot-Sync (t_01f1a9ed-Folge, R1-befund Omar): Der Game-Poller-First-Tick
    // kann VOR diesem init laufen (overlay::init startet den Poller vor
    // pins::init in lib.rs-Setup-Reihenfolge) — die Transition
    // false→true feuert dann sync_game_running, während PinsEnabled noch
    // nicht existiert → Early-Return → sync_all NIE wieder (keine weitere
    // Transition, solange das Spiel durchläuft). Einmaliger Boot-Sync am
    // Ende des init deckt genau diesen Fall deterministisch ab.
    sync_all(app.handle());
    Ok(())
}

/// Lookup-Pfade als Namen (abgeleitet aus monitor_list — unified
/// Enumeration, t_56262ccf; die alte available_monitors-only-Version
/// divergierte auf Hyprland vom Capture-Pfad).
fn monitor_names<R: Runtime>(app: &AppHandle<R>) -> Vec<MonitorName> {
    monitor_list(app)
        .into_iter()
        .map(|m| MonitorName(m.name))
        .collect()
}

/// Lookup-Pfad (t_56262ccf): Screen-Index über die unified Enumeration —
/// nicht auflösbar → FAIL-SOFT-FALLBACK auf den Primärmonitor (Index 0)
/// mit SICHTBAREM log::warn (Journal-Filter ≥WARN), statt still zu
/// überspringen und die Pins für immer unsichtbar zu lassen.
fn screen_index_resolved_or_primary(names: &[MonitorName], screen: &str) -> Option<usize> {
    match screen_index_of(names, screen) {
        Some(idx) => Some(idx),
        None if !names.is_empty() => {
            log::warn!(
                "pins: Screen {screen:?} nicht auflösbar (erwartet: Monitor-Namen) — Fallback auf Primärmonitor"
            );
            Some(0)
        }
        None => None,
    }
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
        let Some(idx) = screen_index_resolved_or_primary(&names, &screen) else {
            log::warn!("pins: keine Monitore enumerierbar — Screen {screen:?} übersprungen");
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
/// setzt den Input-Modus (§9.1.6, präzisiert M4-E t_34efb781): das
/// Pin-Fenster ist REIN ANZEIGE (Konstante `ignore=true`), die
/// Interaktion läuft über unsichtbare Patch-Flächen im Spotlight-Webview.
pub fn sync_all<R: Runtime>(app: &AppHandle<R>) {
    let (enabled, pinned_count, game_running) = {
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
        )
    };
    let visible = pins_effective_visible(enabled, pinned_count, game_running);
    log::info!(
        "pins: sync_all visible={visible} (enabled={enabled} pinned={pinned_count} game_running={game_running}) windows={}",
        pin_windows(app).len()
    );
    // Geometrie-Quelle (t_01f1a9ed): unified Monitor-Enumeration, aufgelöst
    // über den Screen-Index im Label — NICHT win.current_monitor() (None vor
    // dem ersten Map → Geometrie wurde komplett übersprungen → show() mit
    // Tauri-Default 800×600, Boxen außerhalb des Viewports geclippt).
    let monitors = monitor_list(app);
    for win in pin_windows(app) {
        if visible {
            let rect = pin_window_rect(&monitors, &win);
            if let Some((x, y, w, h)) = rect {
                // VOR dem Map: Fenster deckt den ganzen Screen ab —
                // Placement macht das Frontend in CSS (persistierte x/y).
                let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
                let _ = win.set_size(tauri::PhysicalSize::new(w, h));
            }
            // Input-Modus NUR im Moment des show() — erst mappen, dann
            // Shape stellen (tao paniziert auf nicht gemappten Fenstern,
            // t_ef8222da). pin_input_mode liefert hier immer Some.
            let _ = win.show();
            // NACH dem Map nochmal (idempotent, billiger Satz): fängt
            // Wayland-Async-Mapping ab, das die Pre-Show-Geometrie
            // verwerfen kann (t_01f1a9ed Punkt 2).
            if let Some((x, y, w, h)) = rect {
                let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
                let _ = win.set_size(tauri::PhysicalSize::new(w, h));
            }
            if let Some(ignore) = pin_input_mode(true) {
                let _ = win.set_ignore_cursor_events(ignore);
            }
        } else {
            // Hide OHNE Follow-up-Window-Mutation: kein set_ignore auf
            // versteckten Fenstern (tao-Panik-Pfad).
            let _ = win.hide();
        }
    }
    sync_tray_check(app, enabled);
}

// NOTE t_34efb781: das frühere „Mapped-Tracking“ von Pin-Fenstern ist ent-
// fallen — sein einziger Verbraucher war der Input-Modus-Flip in
// `sync_overlay_visibility`, und PINS SIND JETZT REIN ANZEIGE (Konstante).
// Window-Mutation passieren weiterhin NUR im Moment des show() (Panic-Gate
// bleibt via Bauform erfüllt), es gibt aber keinen Zustand mehr zu spiegeln.

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

/// Overlay-Sichtbarkeitswechsel (aus overlay::apply_visibility): KEINE
/// Window-Mutation mehr (§9.1.6, präzisiert M4-E t_34efb781) — das
/// Pin-Fenster ist REIN ANZEIGE (Konstante `ignore=true`), der Input-Modus
/// wechselt nicht mit dem Overlay-Zustand. Die Interaktion läuft über
/// unsichtbare PATCH-Flächen im Spotlight-Webview; der Overlay-Zustand
/// wird vom Backend-Konstantenpfad hier nicht mehr gespiegelt. Die
/// Naht bleibt (overlay.rs ruft sie weiter) — Fail-soft, ohne State.
pub fn sync_overlay_visibility<R: Runtime>(app: &AppHandle<R>, _overlay_visible: bool) {
    // Konstante: nichts zu tun. Der Input-Modus des Pin-Fensters ist
    // ausschließlich im Moment des show() gesetzt (sync_all, §9.1.6) —
    // Overlay-Visibility toggelt KEINE Window-Mutation mehr (Fail-soft:
    // keine State-Anfrage, kein Window-Touch).
    let _ = app;
}

// ---------------------------------------------------------------------------
// Tauri-Commands (Overlay-Frontend + Pin-Fenster-Frontend)
// ---------------------------------------------------------------------------

/// DTO für das Frontend (Mirror von settings::PinEntry).
#[derive(Debug, Serialize, Clone, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
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
#[serde(rename_all = "snake_case")]
pub struct PinRowDto {
    pub idx: String,
    pub name: String,
    pub sub: Option<String>,
    pub value: String,
    pub side: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PinsState {
    pub enabled: bool,
    pub game_running: bool,
    pub overlay_visible: bool,
    pub pins: Vec<PinDto>,
    /// M4-E (t_34efb781): Monitor des SPOTLIGHT-Fensters (derselbe
    /// Aufnahme-Pfad wie `pin_add` — unified Enumeration, t_56262ccf).
    /// Die Patch-Flächen im Spotlight rendern nur Pins DIESER Screens
    /// — die Spot-Viewport-Koordinaten decken exakt diesen Monitor ab.
    pub spotlight_screen: String,
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

/// Monitor des Spotlight-Fensters (derselbe Aufnahme-Pfad wie `pin_add`):
/// Rect aus `current_monitor_rect` → Auflösung über die unified Enumeration
/// (t_56262ccf). Kein Spotlight/kein Monitor → leerer String (Call-Filter
/// im Frontend: `pin.screen === spotlight_screen`).
pub fn spotlight_screen_of<R: Runtime>(app: &AppHandle<R>) -> String {
    let monitors = monitor_list(app);
    let spotlight = app.get_webview_window(crate::overlay::SPOTLIGHT_LABEL);
    let current = spotlight.as_ref().and_then(current_monitor_rect);
    current
        .and_then(|r| monitor_name_by_rect(&monitors, r))
        .or_else(|| monitors.first().map(|m| m.name.clone()))
        .unwrap_or_default()
}

#[tauri::command(rename_all = "snake_case")]
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
        spotlight_screen: spotlight_screen_of(&app),
    }
}

/// Pins für ein Pin-Fenster nach Label (nur der eigene Screen).
#[tauri::command(rename_all = "snake_case")]
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
#[tauri::command(rename_all = "snake_case")]
pub fn pin_add<R: Runtime>(
    app: AppHandle<R>,
    view_id: String,
    box_index: u32,
    opacity: u32,
    title: String,
    kpi: Option<String>,
    rows: Vec<PinRowDto>,
) -> bool {
    // Capture (t_56262ccf): Identität über die unified Enumeration — der
    // NAME von current_monitor() ist auf Hyprland der Model-String
    // ('CU34G2XP' statt 'DP-3') und war die Wurzel des Mismatch. Die
    // Geometrie des current_monitor wird gegen monitor_list aufgelöst
    // (selber Layout-Raum, t_317b9d15). Fail-soft: nicht auflösbar →
    // Primärmonitor-Name persistieren (statt leer abzubrechen).
    let monitors = monitor_list(&app);
    let spotlight = app.get_webview_window(crate::overlay::SPOTLIGHT_LABEL);
    let current = spotlight.as_ref().and_then(|w| current_monitor_rect(w));
    let screen = current
        .as_ref()
        .and_then(|r| monitor_name_by_rect(&monitors, *r))
        .or_else(|| monitors.first().map(|m| m.name.clone()))
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
    let screen_size = current.map(|(_, _, w, h)| (w as i32, h as i32));
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
#[tauri::command(rename_all = "snake_case")]
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
#[tauri::command(rename_all = "snake_case")]
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
#[tauri::command(rename_all = "snake_case")]
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
#[tauri::command(rename_all = "snake_case")]
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
#[serde(rename_all = "snake_case")]
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
#[tauri::command(rename_all = "snake_case")]
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
#[tauri::command(rename_all = "snake_case")]
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

    /// M4-Fix (t_df166209, Befund 2): Tauri-Command-Args + Serde-DTOs
    /// WERDEN snake_case — das Frontend ruft alles mit {view_id,
    /// box_index, screen_w …} (pins.ts-Spiegel). Ohne rename_all
    /// mappt Tauri v2 auf camelCase → „missing required key viewId“.
    #[test]
    fn dto_serializes_snake_case() {
        let dto = PinDto {
            view_id: "routes".into(),
            screen: "Monitor 0".into(),
            box_index: 3,
            opacity: 100,
            pinned: true,
            x: 10,
            y: 20,
            title: "t".into(),
            kpi: None,
            rows: Vec::new(),
        };
        let js = serde_json::to_value(&dto).unwrap();
        assert!(js.get("view_id").is_some());
        assert!(js.get("box_index").is_some());
        assert!(js.get("viewId").is_none());

        let drag: PinDrag = serde_json::from_value(serde_json::json!({
            "x": 1, "y": 2, "w": 356, "h": 78,
            "screen_w": 1920, "screen_h": 1080
        }))
        .expect("snake_case-Keys (screen_w/screen_h) müssen deserialisieren");
        assert_eq!(drag.screen_w, 1920);
        assert_eq!(drag.screen_h, 1080);

        let st = PinsState {
            enabled: false,
            game_running: false,
            overlay_visible: false,
            pins: Vec::new(),
            spotlight_screen: "DP-3".into(),
        };
        let js = serde_json::to_value(&st).unwrap();
        assert!(js.get("game_running").is_some());
        assert!(js.get("overlay_visible").is_some());
        assert!(js.get("spotlight_screen").is_some());
    }

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

    /// t_56262ccf (Fail-soft-Fallback): nicht auflösbare Screens werden
    /// NICHT still übersprungen — Fallback auf Primärmonitor (Index 0).
    /// Die alte Logik sprang still ab (Info-Log, im Journal unsichtbar
    /// bei Filter ≥WARN) → Pins nie sichtbar.
    #[test]
    fn unresolvable_screen_falls_back_to_primary() {
        let m = [MonitorName("DP-3".into()), MonitorName("HDMI-A-2".into())];
        // Persistierter Model-String aus dem Lokaltest-Befund:
        assert_eq!(screen_index_resolved_or_primary(&m, "CU34G2XP"), Some(0));
        // Auflösbarer Name bleibt exakt:
        assert_eq!(screen_index_resolved_or_primary(&m, "HDMI-A-2"), Some(1));
        // Leere Monitorliste (keine Enumeration): None, kein Fake-Index.
        assert_eq!(screen_index_resolved_or_primary(&[], "DP-3"), None);
    }

    /// t_56262ccf (Identitäts-Helfer): Capture über Geometrie — ein
    /// Monitor, dessen rect exakt passt, liefert den EINHEITLICHEN Namen
    /// (wl_output-Name 'DP-3'), egal welchen Namens-String die
    /// current_monitor()-API zurückgab (Model-String 'CU34G2XP').
    #[test]
    fn identity_resolves_by_rect_not_by_name_string() {
        let monitors = [
            MonitorRect {
                name: "DP-3".into(),
                x: 0,
                y: 0,
                w: 3440,
                h: 1440,
            },
            MonitorRect {
                name: "HDMI-A-2".into(),
                x: 1825,
                y: -1920,
                w: 1080,
                h: 1920,
            },
        ];
        // Exakter Rect-Match (Sebo-Layout, t_317b9d15):
        assert_eq!(
            monitor_name_by_rect(&monitors, (0, 0, 3440, 1440)),
            Some("DP-3".into())
        );
        assert_eq!(
            monitor_name_by_rect(&monitors, (1825, -1920, 1080, 1920)),
            Some("HDMI-A-2".into())
        );
        // Kein exakter Match → Zentrum-Kontainment (Skalierungs-Toleranz):
        assert_eq!(
            monitor_name_by_rect(&monitors, (2, 2, 3436, 1436)),
            Some("DP-3".into())
        );
        // Ganz daneben → None (Caller fällt auf Primärmonitor zurück).
        assert_eq!(monitor_name_by_rect(&monitors, (9999, 9999, 10, 10)), None);
    }

    /// t_56262ccf (Boot-Repair): persistierte Einträge mit nicht
    /// auflösbarem Screen werden beim Init auf den Primärmonitor
    /// umgemappt (einmalig, persistiert); auflösbare bleiben unangetastet,
    /// nicht-gepinnte werden nicht repariert, leere Monitorliste = no-op.
    #[test]
    fn boot_repair_remaps_unresolvable_screens() {
        let mk = |screen: &str, pinned: bool| settings::PinEntry {
            view_id: "routes".into(),
            screen: screen.into(),
            box_index: 0,
            opacity: 100,
            pinned,
            x: 0,
            y: 0,
            title: "t".into(),
            kpi: None,
            rows: Vec::new(),
        };
        let monitors = [
            MonitorRect {
                name: "DP-3".into(),
                x: 0,
                y: 0,
                w: 3440,
                h: 1440,
            },
            MonitorRect {
                name: "HDMI-A-2".into(),
                x: 1825,
                y: -1920,
                w: 1080,
                h: 1920,
            },
        ];
        let mut entries = vec![
            mk("CU34G2XP", true),  // Model-String → Reparatur auf DP-3
            mk("CU34G2XP", false), // nicht gepinnt → keine Reparatur
            mk("HDMI-A-2", true),  // auflösbar → unverändert
        ];
        let repaired = repair_unresolvable_screens(&mut entries, &monitors);
        assert_eq!(repaired, 1);
        assert_eq!(entries[0].screen, "DP-3");
        assert!(entries[0].pinned);
        assert_eq!(entries[1].screen, "CU34G2XP"); // ungepinnt bleibt
        assert_eq!(entries[2].screen, "HDMI-A-2");
        // Keine Monitore: no-op (nichts wird blind auf "" gemappt).
        let mut entries2 = vec![mk("CU34G2XP", true)];
        assert_eq!(repair_unresolvable_screens(&mut entries2, &[]), 0);
        assert_eq!(entries2[0].screen, "CU34G2XP");
        // Zweiter Lauf nach Reparatur: idempotent (0 Repairs).
        assert_eq!(repair_unresolvable_screens(&mut entries, &monitors), 0);
    }

    /// t_01f1a9ed (Punkt 1): Geometrie aus dem resolved Rect — der
    /// Screen-Index im Label wählt den Monitor aus der unified Enumeration;
    /// Index außerhalb der Liste → None (Caller: Fallback + warn).
    #[test]
    fn rect_for_screen_resolves_by_label_index() {
        let monitors = [
            MonitorRect {
                name: "DP-3".into(),
                x: 0,
                y: 0,
                w: 3440,
                h: 1440,
            },
            MonitorRect {
                name: "HDMI-A-2".into(),
                x: 1825,
                y: -1920,
                w: 1080,
                h: 1920,
            },
        ];
        assert_eq!(rect_for_screen(&monitors, 0), Some((0, 0, 3440, 1440)));
        assert_eq!(
            rect_for_screen(&monitors, 1),
            Some((1825, -1920, 1080, 1920))
        );
        // Kein dritter Monitor → None (kein Fake-Rect).
        assert_eq!(rect_for_screen(&monitors, 2), None);
        assert_eq!(rect_for_screen(&[], 0), None);
    }

    /// t_01f1a9ed (Punkt 1, Fallback): Label-Index nicht auflösbar →
    /// Primärmonitor-Rect (Index 0); leere Monitorliste → None.
    #[test]
    fn pin_window_rect_falls_back_to_primary() {
        // Simulierte Auflösungslogik wie pin_window_rect (pure Teil):
        let monitors = [MonitorRect {
            name: "DP-3".into(),
            x: 0,
            y: 0,
            w: 3440,
            h: 1440,
        }];
        let resolve = |idx: Option<usize>| -> Option<(i32, i32, u32, u32)> {
            idx.and_then(|i| rect_for_screen(&monitors, i))
                .or_else(|| rect_for_screen(&monitors, 0))
        };
        // 'pins-5' auf Ein-Monitor-Setup → Index 5 existiert nicht → Primary.
        assert_eq!(resolve(screen_of_label("pins-5")), Some((0, 0, 3440, 1440)));
        // Gültiger Index bleibt exakt:
        assert_eq!(resolve(screen_of_label("pins-0")), Some((0, 0, 3440, 1440)));
        // Fremdes Label (None) → trotzdem Primary-Fallback.
        assert_eq!(
            resolve(screen_of_label("app-window")),
            Some((0, 0, 3440, 1440))
        );
        // Leere Monitorliste: auch der Fallback liefert None.
        let empty: [MonitorRect; 0] = [];
        assert_eq!(
            screen_of_label("pins-0")
                .and_then(|i| rect_for_screen(&empty, i))
                .or_else(|| rect_for_screen(&empty, 0)),
            None
        );
    }

    /// Regression t_ef8222da (M4-Crash-Loop, tao-Panik in
    /// CursorIgnoreEvents auf nicht gemapptem Pin-Fenster): die pure
    /// Logik darf für versteckte Pin-Fenster NIEMALS einen Input-Shape-
    /// Request ergeben — Szenarien aus der FIX-KARTE.
    /// M4-E (t_34efb781): das Pin-Fenster ist REIN ANZEIGE — sichtbare
    /// Fenster bekommen IMMER `ignore=true`, der Overlay-Zustand spielt
    /// KEINE Rolle mehr (§9.1.6 präzisiert; Interaktion über Spotlight-
    /// Patches).
    #[test]
    fn input_mode_never_touches_hidden_pin_windows() {
        // Boot ohne SC: Pins restauriert, game_running=false → Pin-Fenster
        // hidden → KEIN Call (vorher: unconditional set_ignore → Panik).
        assert_eq!(pin_input_mode(false), None);
        // Overlay-Toggle bei hiddenem Pin-Fenster → kein Call.
        assert_eq!(pin_input_mode(false), None);
        // Sichtbares Pin-Fenster → IMMER input-transparent (Konstante,
        // unabhängig vom Overlay-Zustand — der Parameter ist entfallen).
        assert_eq!(pin_input_mode(true), Some(true));
    }
}
