//! M4 (t_7bf507e6) + M5 (t_d10a0d84): Dauerhafte Shellboxen „Pins“.
//!
//! Verträge (docs/FENSTERMODUS-PLAN.md §6-Revision M5 + §9.1.4–6 — bindend):
//!
//! - **Träger (M5 — EIN-FENSTER-ARCHITEKTUR):** Pins sind DOM-Boxen IM
//!   SPOTLIGHT-Webview (`PinBoxes.svelte`), NICHT eigene WebviewWindows.
//!   Die früheren `pins-<idx>`-Fenster (M4 Option A) sind entfernt —
//!   Lokaltest-Beweis (Sebo, 2026-11-10, Wayland/GTK3): input regions
//!   unzuverlässig, always-on-top-Stacking-Fight mit dem Spotlight.
//! - **Spotlight-Lebenslauf:** NEUER Zustand „pins-only“ (overlay.rs
//!   `apply_visibility`): gemappt BLEIBT, solange `pins.enabled &&
//!   gepinnte>0 && game_running` — auch wenn die Palette verborgen ist.
//!   Klick-Modus = der bewährte complete-through (`set_ignore_cursor_events(true)`).
//! - **Sichtbarkeit: strikt an `game_running` gekoppelt** (§9.1.5) — Pins
//!   haben KEINEN Sichtbarkeitskanal ohne laufendes SC. Der Tray-Schalter
//!   (`pins_toggle`, Checkbox „Pin-Boxen anzeigen“) ist eine NECESSARY-
//!   Bedingung, nie hinreichend: effektiv sichtbar = enabled && pinned &&
//!   game_running.
//! - **Interaktion:** die Boxen sind interaktiv, SOBALD das Spotlight
//!   interaktiv ist (Palette offen): Drag → `pin_move`, Klick → Controller-
//!   Popover (Deckkraft, Lösen, Schließen). Palette zu → reine Anzeige (das
//!   Fenster ist komplett durchlässig — DOM-Interaktion ist ohnehin weg).
//! - **Persistenz:** Pin-Eintrag `{view_id, screen, box_index, opacity,
//!   pinned}` (+ Box-Position x/y + Anzeige-Snapshot) in app_settings
//!   (`[pins]`-Sektion), Restore beim Start — effektiv sichtbar erst bei
//!   game_running. Deckkraft pro Box, CSS opacity auf dem INHALT (Range
//!   20–100 %, Presets 20/55/100 — clamp in `clamp_opacity`).
//! - **Placement:** Box-Standardbreite 356px, Edge-Snapping L/R/T/B in
//!   `snap_to_edge` (Drag läuft im Frontend, das Backend snappt + persistiert).
//! - **Monitor-Identität (t_56262ccf, bleibt):** die unified Enumeration
//!   (`monitor_list`/`hyprland_monitor_list`/`repair_unresolvable_screens`)
//!   dient jetzt der Box-Placement-Validierung (Capture-Pfad `pin_add`,
//!   Spotlight-Screen-Zuordnung) und der M1-Fenster-Geometrie.
//! - **Universalität (Schema-U):** die Pin-Verwaltung ist rein mechanisch
//!   ({view_id, screen, box_index}); Plugin-Semantik kommt NUR aus dem
//!   Anzeige-Snapshot, den die View-Komponente des Overlay-Frontends beim
//!   Anheften erzeugt (generische Display-Zeilen). KEIN SCLM-Vokabular.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::window::MonitorRect;

/// Box-Standardbreite (Pins-Vertrag).
pub const PIN_BOX_W: u32 = 356;

/// Deckkraft-Range (Pins-Vertrag): 20–100 %.
pub const OPACITY_MIN: u32 = 20;
pub const OPACITY_MAX: u32 = 100;

/// Edge-Snapping-Mindestabstand zum Screen-Rand (px).
pub const EDGE_MARGIN: i32 = 8;

/// Tray-Checkbox-Handle (vom tray::init gesetzt) — Zustand zieht
/// `pins_set_enabled`/init live nach (Muster wie WindowTrayItem).
pub struct PinsTrayItem<R: tauri::Runtime>(pub Mutex<Option<tauri::menu::CheckMenuItem<R>>>);

/// In-memory Zustandsflag (vom Backend-Logikpfad gesetzt), damit die
/// Tray-Checkbox und `pins_state` denselben Stand sehen.
#[derive(Default)]
pub struct PinsEnabled(AtomicBool);

/// Ereignis ans Spotlight-Frontend: Pin-Set hat sich geändert.
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

/// Spotlight-Modus (M5 §6-Revision — pure Berechnung, unit-getestet).
///
/// - `Palette`: die Palette ist effektiv sichtbar (`config_open ||
///   user_visible && game_running`) — Spotlight gemappt + interaktiv.
/// - `PinsOnly`: Palette verborgen, ABER Pins aktiv (enabled ∧ gepinnte>0 ∧
///   game_running) — Spotlight bleibt gemappt, complete-through, nur die
///   Pin-Boxen als DOM sichtbar.
/// - `Hidden`: weder Palette noch Pins — Spotlight hide (alter Vertrag).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpotlightMode {
    Palette,
    PinsOnly,
    Hidden,
}

/// Berechnet den Spotlight-Modus aus den vier Eingängen (pure, §6-Tabelle).
pub fn spotlight_mode(
    palette_visible: bool,
    pins_enabled: bool,
    pinned_count: usize,
    game_running: bool,
) -> SpotlightMode {
    if palette_visible {
        SpotlightMode::Palette
    } else if pins_effective_visible(pins_enabled, pinned_count, game_running) {
        SpotlightMode::PinsOnly
    } else {
        SpotlightMode::Hidden
    }
}

/// Unified Monitor-Enumeration (t_56262ccf — Monitor-Identitäts-Mismatch):
/// DIE EINE Identitäts-Quelle für Capture (pin_add) und Spotlight-Screen-
/// Zuordnung (spotlight_screen_of).
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
pub fn monitor_list<R: tauri::Runtime>(app: &AppHandle<R>) -> Vec<MonitorRect> {
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
fn primary_first_in_place<R: tauri::Runtime>(app: &AppHandle<R>, list: &mut [MonitorRect]) {
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

fn primary_first<R: tauri::Runtime>(
    app: &AppHandle<R>,
    mut list: Vec<MonitorRect>,
) -> Vec<MonitorRect> {
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
fn current_monitor_rect<R: tauri::Runtime>(
    win: &tauri::WebviewWindow<R>,
) -> Option<(i32, i32, u32, u32)> {
    let m = win.current_monitor().ok().flatten()?;
    let pos = m.position();
    let size = m.size();
    Some((pos.x, pos.y, size.width, size.height))
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

/// Initialisiert die Pins-Zustände (KEINE Fenster mehr — M5: die Boxen
/// leben im Spotlight-Webview).
pub fn init<R: tauri::Runtime>(app: &tauri::App<R>) -> tauri::Result<()> {
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
    Ok(())
}

/// Pin-Aktivität für die Spotlight-Modus-Berechnung (M5): (enabled,
/// pinned_count) — game_running kommt aus dem Overlay-State des Callers.
pub fn pins_activity<R: tauri::Runtime>(app: &AppHandle<R>) -> (bool, usize) {
    let enabled = app
        .try_state::<PinsEnabled>()
        .map(|e| e.0.load(Ordering::SeqCst))
        .unwrap_or(false);
    let pinned = {
        let st = app.state::<crate::app_settings::SettingsState>();
        let s = st.inner.lock().expect("settings lock");
        s.pins()
            .map(|p| p.list.iter().filter(|e| e.pinned).count())
            .unwrap_or(0)
    };
    (enabled, pinned)
}

/// Tray-Checkbox nachziehen (fail-soft — Tray darf fehlen).
fn sync_tray_check<R: tauri::Runtime>(app: &AppHandle<R>, checked: bool) {
    if let Some(item) = app.try_state::<PinsTrayItem<R>>()
        && let Some(check) = item.0.lock().expect("tray item lock").as_ref()
    {
        let _ = check.set_checked(checked);
    }
}

/// Event ans Spotlight-Frontend: Pin-Set geändert (die PinBoxes-Komponente
/// holt selbst nach via pins_state).
fn notify_changed<R: tauri::Runtime>(app: &AppHandle<R>) {
    use tauri::Emitter;
    let _ = app.emit(PINS_CHANGED_EVENT, true);
}

// ---------------------------------------------------------------------------
// Tauri-Commands (Spotlight-Frontend)
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
    /// Die Pin-Boxen im Spotlight rendern nur Pins DIESER Screens —
    /// die Spot-Viewport-Koordinaten decken exakt diesen Monitor ab.
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

/// Monitor des Spotlight-Fensters (derselbe Aufnahme-Pfad wie `pin_add`):
/// Rect aus `current_monitor_rect` → Auflösung über die unified Enumeration
/// (t_56262ccf). Kein Spotlight/kein Monitor → leerer String (Call-Filter
/// im Frontend: `pin.screen === spotlight_screen`).
pub fn spotlight_screen_of<R: tauri::Runtime>(app: &AppHandle<R>) -> String {
    let monitors = monitor_list(app);
    let spotlight = app.get_webview_window(crate::overlay::SPOTLIGHT_LABEL);
    let current = spotlight.as_ref().and_then(current_monitor_rect);
    current
        .and_then(|r| monitor_name_by_rect(&monitors, r))
        .or_else(|| monitors.first().map(|m| m.name.clone()))
        .unwrap_or_default()
}

#[tauri::command(rename_all = "snake_case")]
pub fn pins_state<R: tauri::Runtime>(app: AppHandle<R>) -> PinsState {
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

/// Anheften aus dem Overlay-Frontend: mechanical Keys + Snapshot.
/// Der Screen ist der Monitor des Overlay-Fensters (das Overlay liegt
/// immer da, wo der Nutzer pinnt). Duplikate (view_id/screen/box_index,
/// aktiv) werden verhindert.
#[tauri::command(rename_all = "snake_case")]
pub fn pin_add<R: tauri::Runtime>(
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
    notify_changed(&app);
    crate::overlay::refresh_visibility(&app);
    true
}

/// „Lösen“: pinned=false (Eintrag bleibt, Restore überspringt ihn).
#[tauri::command(rename_all = "snake_case")]
pub fn pin_unpin<R: tauri::Runtime>(
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
        crate::overlay::refresh_visibility(&app);
    }
    changed
}

/// „Schließen“: Pin-Eintrag entfernt (Plugin/View läuft weiter).
#[tauri::command(rename_all = "snake_case")]
pub fn pin_remove<R: tauri::Runtime>(
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
        crate::overlay::refresh_visibility(&app);
    }
    removed
}

/// Deckkraft setzen (20–100, clamp).
#[tauri::command(rename_all = "snake_case")]
pub fn pin_set_opacity<R: tauri::Runtime>(
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
/// (x/y screen-relativ). Drag ist nur im Palette-offen-Zustand möglich —
/// das Frontend ruft das nur dann auf.
#[tauri::command(rename_all = "snake_case")]
pub fn pin_move<R: tauri::Runtime>(
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
pub fn pin_update<R: tauri::Runtime>(
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
pub fn pins_set_enabled<R: tauri::Runtime>(app: AppHandle<R>, on: bool) {
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
    sync_tray_check(&app, on);
    notify_changed(&app);
    crate::overlay::refresh_visibility(&app);
}

/// Tray-Pfad (on_menu_event): Checkbox-Toggle.
pub fn tray_toggle<R: tauri::Runtime>(app: &AppHandle<R>) {
    let on = !app
        .try_state::<PinsEnabled>()
        .map(|e| e.0.load(Ordering::SeqCst))
        .unwrap_or(false);
    pins_set_enabled(app.clone(), on);
}

// ---------------------------------------------------------------------------
// Tests (reine Logik — game_running-Sichtbarkeitsregel, Clamp, Snapping,
// Placement, Spotlight-Modus)
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

    /// M5 (§6-Revision): Spotlight-Modus-Berechnung — die drei Zustände
    /// Palette/PinsOnly/Hidden mit allen Grenzfällen.
    #[test]
    fn spotlight_mode_follows_palette_then_pins() {
        // Palette sichtbar gewinnt IMMER (auch mit Pins).
        assert_eq!(spotlight_mode(true, true, 3, true), SpotlightMode::Palette);
        assert_eq!(
            spotlight_mode(true, false, 0, false),
            SpotlightMode::Palette
        );
        // Pins-only: Palette zu, aber Pins aktiv + Spiel läuft.
        assert_eq!(
            spotlight_mode(false, true, 3, true),
            SpotlightMode::PinsOnly
        );
        // Pins-only braucht game_running (§9.1.5 — kein eigener Kanal).
        assert_eq!(spotlight_mode(false, true, 3, false), SpotlightMode::Hidden);
        // Tray aus → nie pins-only.
        assert_eq!(spotlight_mode(false, false, 3, true), SpotlightMode::Hidden);
        // Keine aktiven Pins → nie pins-only.
        assert_eq!(spotlight_mode(false, true, 0, true), SpotlightMode::Hidden);
        // Beide aus → Hidden (alter Hide-Vertrag).
        assert_eq!(
            spotlight_mode(false, false, 0, false),
            SpotlightMode::Hidden
        );
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
}
