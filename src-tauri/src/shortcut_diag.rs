//! Diagnose-Instrumentierung für die globale-Shortcut-Verdrahtung
//! (t_ed01e8f7 — „Shortcuts tot, wenn SC gestartet + fokussiert").
//!
//! Beobachtung: Der globale Hotkey-Handler feuert scheinbar nicht (mehr),
//! sobald Star Citizen den Fokus hat. Diese Sonden trennen die zwei
//! Schichten, die bislang nicht unterscheidbar sind:
//!
//! 1. **OS-Ebene**: `WM_HOTKEY` erreicht den Plugin-Handler des
//!    global-shortcut-Plugins überhaupt nicht (Registrierung tot, Höher-
//!    priorendo, UIPI/Elevation, Fullscreen-Exclusive) — dann taucht der
//!    Empfang hier im Log NICHT auf.
//! 2. **App-Ebene**: Der Handler feuert, aber `toggle_visible` no-op't
//!    (z. B. `game_running=false`, obwohl SC läuft → Detektions-Bug) —
//!    dann steht der Empfang im Log mit `matched=None`.
//!
//! Zusätzlich exposeiert `shortcut_diag_status` einen connector-artigen
//! Status-Snapshot für die VM-Probe: registrierte Combos, letzter
//! Hotkey-Empfang und das aktuell fokussierte Fenster inkl. Prozessname +
//! Elevation-Flag (H1: SC elevated → UIPI).

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

/// Dedupe-Fenster zwischen den zwei Dispatch-Pfaden (WM_HOTKEY-Normalpfad
/// und H5-LL-Hook-Fallback): dasselbe physische Tastendrücken darf nicht
/// doppelt toggeln — SC im Hintergrund feuert beide Wege.
const DEDUPE_WINDOW: std::time::Duration = std::time::Duration::from_millis(600);

/// Letzte Dispatch-Zeit je Slot (0=toggle, 1=click_through).
static RECENT_DISPATCH: std::sync::Mutex<[Option<std::time::Instant>; 2]> =
    std::sync::Mutex::new([None, None]);

/// Der EINE Dispatch-Punkt für Hotkey-Aktionen — beide Empfangswege
/// (WM_HOTKEY-Plugin-Handler und H5-LL-Hook-Fallback) laufen hier zusammen.
/// Dedupliziert Doppel-Feuer innerhalb `DEDUPE_WINDOW` (Log sichtbar).
pub fn dispatch_kind<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    kind: crate::overlay::ShortcutKind,
) {
    let idx = match kind {
        crate::overlay::ShortcutKind::Toggle => 0,
        crate::overlay::ShortcutKind::ClickThrough => 1,
    };
    {
        let mut recent = RECENT_DISPATCH.lock().expect("recent dispatch lock");
        if let Some(t) = recent[idx] {
            let elapsed = t.elapsed();
            if elapsed < DEDUPE_WINDOW {
                log::info!("hotkey dispatch deduped for slot {idx} ({elapsed:?} after last)");
                return;
            }
        }
        recent[idx] = Some(std::time::Instant::now());
    }
    match kind {
        crate::overlay::ShortcutKind::Toggle => crate::overlay::toggle_visible(app),
        crate::overlay::ShortcutKind::ClickThrough => {
            let _ = crate::overlay::toggle_click_through(app);
        }
    }
}
use tauri::Manager;

/// Letzter empfangener Shortcut-Event (beide Zustände: Pressed UND Released).
#[derive(Clone, Debug, Serialize)]
pub struct HotkeyEventRecord {
    /// Kanonischer Shortcut-String, wie das OS ihn geliefert hat
    /// (`Shortcut` ist `Display`-fähig: `"ctrl+shift+u"`).
    pub combo: String,
    /// Slot, auf den die Live-Bindings den Combo mappen — `None` = der
    /// Empfang kam, aber KEIN Slot matcht (→ App-Ebene-Bug, nicht OS-Ebene).
    pub matched: Option<String>,
    /// Zustand des Events (`Pressed` / `Released`).
    pub state: String,
    /// Unix-Sekunden des Empfangs.
    pub at_unix: u64,
    /// ISO-8601 (UTC) des Empfangs.
    pub at_iso: String,
}

/// Prozess-Name + Elevation des aktuell fokussierten Fensters (Windows).
#[derive(Clone, Debug, Serialize)]
pub struct FocusedWindowInfo {
    pub pid: u32,
    pub process: String,
    /// `TokenElevation`-Flag — H1 (UIPI/High-Integrity-Fokus).
    pub elevated: bool,
    /// Klassennamen des HWNDs (H2: Fullscreen-Exclusive / Fensterklasse).
    pub window_class: String,
    /// Rechteck des Fensters (links, oben, rechts, unten) — H2: Fullscreen?
    pub rect: [i32; 4],
    /// Das fokussierte HWND selbst (Diagnose für Fensterklassen-Rätsel).
    pub hwnd: u64,
}

/// Status-Snapshot für die VM-Probe (IPC `shortcut_diag_status`).
#[derive(Clone, Debug, Serialize)]
pub struct ShortcutDiagSnapshot {
    /// Persistierter Toggle-Combo (kanonisch) — der SOLL-Zustand.
    pub toggle_combo: Option<String>,
    /// Persistierter Click-Through-Combo (kanonisch).
    pub click_combo: Option<String>,
    /// Ist das global-shortcut-Plugin initialisiert (OS-Registrierung möglich)?
    pub plugin_active: bool,
    /// Letzter Hotkey-Empfang (verbleibt auch nach Neustart nicht —
    /// bewusst nur Runtime, damit die VM-Probe frische Timestamps sieht).
    pub last_event: Option<HotkeyEventRecord>,
    /// Fokussiertes Fenster zum Zeitpunkt des Abrufs.
    pub focused: Option<FocusedWindowInfo>,
    /// LL-Hook-Fallback-Zähler (Windows): `None` = Hook nie installiert
    /// (SC-Detektion fehlgeschlagen oder Install-Fehler). Steigende
    /// `events_total` bei fokussiertem SC = Hook sieht Tasten.
    pub ll_hook: Option<LlHookDiag>,
}

/// Diagnose-Mirror der LL-Hook-Zähler (hotkey_llhook::counters).
#[derive(Clone, Debug, Serialize)]
pub struct LlHookDiag {
    pub active: bool,
    pub installed_at: Option<String>,
    pub events_total: u64,
    pub keydowns_total: u64,
    pub matched_total: u64,
    pub last_key: Option<String>,
    pub last_down_at: Option<String>,
}

/// VK → lesbare Taste (Diagnose-Anzeige; unbekannt = Hex).
/// Wird nur im Windows-Hook-Zweig gebaut (ll_hook_diag); auf anderen OS toter,
/// aber dokumentierender Helfer — daher explizit erlaubt statt toter Code.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn vk_name(vk: u32) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(vk)
            .map(|c| c.to_string())
            .unwrap_or_default(),
        0x70..=0x7B => format!("F{}", vk - 0x6F),
        0xA0..=0xA1 => "shift".into(),
        0xA2..=0xA3 => "ctrl".into(),
        0xA4..=0xA5 => "alt".into(),
        0x5B | 0x5C => "win".into(),
        _ => format!("vk:{vk:#04x}"),
    }
}

#[cfg(target_os = "windows")]
fn ll_hook_diag() -> Option<LlHookDiag> {
    crate::hotkey_llhook::counters().map(|c| LlHookDiag {
        active: c.active,
        installed_at: (c.installed_unix != 0).then(|| iso8601(c.installed_unix)),
        events_total: c.events_total,
        keydowns_total: c.keydowns_total,
        matched_total: c.matched_total,
        last_key: (c.last_vk != 0).then(|| vk_name(c.last_vk)),
        last_down_at: (c.last_down_unix != 0).then(|| iso8601(c.last_down_unix)),
    })
}

#[cfg(not(target_os = "windows"))]
fn ll_hook_diag() -> Option<LlHookDiag> {
    None
}

/// Runtime-Store für den letzten Empfang (managed Tauri state).
#[derive(Default)]
pub struct HotkeyDiagState {
    pub last_event: std::sync::Mutex<Option<HotkeyEventRecord>>,
}

/// ISO-8601 (UTC, Sekundenpräzision) aus Unix-Sekunden — eigenes Formatting
/// statt chrono: die Dep ist bewusst nicht im Tree.
pub fn iso8601(unix: u64) -> String {
    let secs = unix % 60;
    let mins = (unix / 60) % 60;
    let hours = (unix / 3600) % 24;
    // Zivil-Kalender (Howard Hinnant) → J-M-D aus den Unix-Tagen.
    let days = unix / 86_400;
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}-{m:02}-{d:02}T{hours:02}:{mins:02}:{secs:02}Z")
}

/// Unix-Tage → (Jahr, Monat, Tag). Howard-Hinnant-Algorithmus.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Jetzt als (unix, iso) — der Einzigeinstieg der Sonden in die Uhrzeit.
pub fn now_record_parts() -> (u64, String) {
    let unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    (unix, iso8601(unix))
}

/// Sonde im global-shortcut-Handler: JEDE Event-Phase loggen und den
/// letzten Empfang für den Status-Snapshot ablegen. Muss VOR dem
/// `Pressed`-Filter und VOR dem Slot-Matching laufen, damit auch ein
/// nicht-matchender Empfang (App-Ebene-Bug) sichtbar bleibt.
pub fn record_event<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    combo: String,
    matched: Option<String>,
    state: String,
) {
    let (at_unix, at_iso) = now_record_parts();
    log::info!("hotkey received: combo={combo:?} matched={matched:?} state={state} at={at_iso}");
    if let Some(diag) = app.try_state::<HotkeyDiagState>() {
        *diag.last_event.lock().expect("hotkey diag lock") = Some(HotkeyEventRecord {
            combo,
            matched,
            state,
            at_unix,
            at_iso,
        });
    }
}

/// Slot-Namen für Log + Snapshot.
pub fn slot_name(kind: crate::overlay::ShortcutKind) -> &'static str {
    match kind {
        crate::overlay::ShortcutKind::Toggle => "toggle",
        crate::overlay::ShortcutKind::ClickThrough => "click_through",
    }
}

#[cfg(target_os = "windows")]
mod win {
    use super::FocusedWindowInfo;
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TokenElevation};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId,
    };

    /// Fokussiertes Fenster inkl. Prozess + Elevation (H1) und Fensterklasse
    /// + Rechteck (H2) lesen. Best-effort: Teilschläge werden zu `None`
    /// heruntergebrochen, statt die VM-Probe zu crashen.
    pub fn focused_window_info() -> Option<FocusedWindowInfo> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                return None;
            }
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == 0 {
                return None;
            }
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut elevated = false;
            let mut token = std::ptr::null_mut();
            if OpenProcessToken(handle, 0x0008 /* TOKEN_QUERY */, &mut token) != 0 {
                let mut te = TOKEN_ELEVATION { TokenIsElevated: 0 };
                let mut ret = 0u32;
                if GetTokenInformation(
                    token,
                    TokenElevation,
                    &mut te as *mut _ as *mut _,
                    std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                    &mut ret,
                ) != 0
                    && te.TokenIsElevated != 0
                {
                    elevated = true;
                }
                windows_sys::Win32::Foundation::CloseHandle(token);
            }
            let mut process = String::new();
            let mut len: u32 = 512;
            let mut buf = [0u16; 512];
            if QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len)
                != 0
                && len > 0
            {
                process = String::from_utf16_lossy(&buf[..len as usize]);
            }
            let mut class = String::new();
            let clen = GetClassNameW(hwnd, buf.as_mut_ptr(), 256);
            if clen > 0 {
                class = String::from_utf16_lossy(&buf[..clen as usize]);
            }
            let mut rect = [0i32; 4];
            let mut r = std::mem::zeroed();
            if GetWindowRect(hwnd, &mut r) != 0 {
                rect = [r.left, r.top, r.right, r.bottom];
            }
            windows_sys::Win32::Foundation::CloseHandle(handle);
            Some(FocusedWindowInfo {
                pid,
                process,
                elevated,
                window_class: class,
                rect,
                hwnd: hwnd as u64,
            })
        }
    }
}

// Defekt 1 (WK-R1): die Windows-Implementierung lebt in `mod win` — der
// aufrufende Scope (ShortcutDiagSnapshot + Test) braucht den Re-Export,
// sonst E0425 `cannot find function focused_window_info` NUR auf Windows.
#[cfg(target_os = "windows")]
use win::focused_window_info;

#[cfg(not(target_os = "windows"))]
fn focused_window_info() -> Option<FocusedWindowInfo> {
    None
}

/// IPC-Status-Snapshot für die VM-Probe.
#[cfg(desktop)]
#[tauri::command]
pub fn shortcut_diag_status<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> ShortcutDiagSnapshot {
    use tauri::Manager;

    let (toggle_combo, click_combo) = match app.try_state::<crate::overlay::ShortcutBindings>() {
        Some(bindings) => (
            bindings
                .current(crate::overlay::ShortcutKind::Toggle)
                .map(|s| s.to_string()),
            bindings
                .current(crate::overlay::ShortcutKind::ClickThrough)
                .map(|s| s.to_string()),
        ),
        None => (None, None),
    };
    ShortcutDiagSnapshot {
        toggle_combo,
        click_combo,
        plugin_active: crate::overlay::global_shortcuts_active(&app),
        last_event: app
            .try_state::<HotkeyDiagState>()
            .and_then(|d| d.last_event.lock().expect("hotkey diag lock").clone()),
        ll_hook: ll_hook_diag(),
        focused: focused_window_info(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso8601_known_dates() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601(1_700_000_000), "2023-11-14T22:13:20Z");
        // Schaltjahr-Rand: 2024-02-29T00:00:00Z = 1709164800
        assert_eq!(iso8601(1_709_164_800), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn slot_names() {
        use crate::overlay::ShortcutKind::*;
        assert_eq!(slot_name(Toggle), "toggle");
        assert_eq!(slot_name(ClickThrough), "click_through");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn focused_window_never_panics_on_empty_desktop() {
        // Hintergrund-Desktop ohne fokussiertes Fenster → None, kein Crash.
        let _ = focused_window_info();
    }
}
