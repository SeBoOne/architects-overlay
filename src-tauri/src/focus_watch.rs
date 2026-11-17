//! Windows-Fokus-Wächter — Pins-Lifecycle wie der Linux-Socket-Pfad
//! (overlay.rs `handle_focus_event`): Pins-only gemappt + fremder Fokus →
//! Pins unmapppen; Rückkehr zu SC → Pins neu mappen.
//!
//! Win-Befund (2026-11-17, t_aad53dd3-Follow-up): der Linux-Pfad lief über
//! Hyprland-Socket-Events, der Windows-Stub war ein No-op — Pins blieben
//! über jedem Fremd-Fokus sichtbar. Diese Datei schließt die Lücke mit einem
//! passiven Poller (GetForegroundWindow, 400 ms, HWND-Dedupe):
//!
//! - KEIN aktives Fokus-Dispatching (v2-Vertrags-Geist: beobachten, nie
//!   korrigieren — der v2-Ban gilt für Workspace-/Fokus-Korrektur-Dispatches,
//!   nicht fürs passive Lesen).
//! - SC-Erkennung über den PROCESS-Namen des Vordergrund-Fensters
//!   (QueryFullProcessImageNameW), nicht über die Fensterklasse — der
//!   Unreal-Fenstername ist nicht stabil, der exe-Name schon (gleiche
//!   Konvention wie GAME_WINDOW_CLASS "starcitizen.exe", case-insensitive).
//! - Eigene Prozesse (Palette, Dialoge) sind kein Fremd-Fokus (pid-Guard).

/// Poll-Intervall des Vordergrund-Wächters. Ein Fokus-Wechsel braucht keine
/// Latenz unter einer menschlichen Tab-Geschwindigkeit; 400 ms kosten
/// vernachlässigbar (eine WinAPI-Abfrage + bei Wechsel ein OpenProcess).
#[cfg(windows)]
const POLL_MS: u64 = 400;

/// Pure Entscheidung: sollen die Pins JETZT ungemappt werden?
/// Spiegel des Linux-Zweigs in overlay.rs (fremder Fokus, pins-only aktiv):
/// - Palette muss verborgen sein (user_visible=false) — bei offener Palette
///   ist der Fokus ohnehin uns (Eigener-prozess-Guard greift).
/// - Pins aktiv (enabled ∧ gepinnte>0) UND Spiel läuft (§9.1.5).
/// - Fenster aktuell gemappt UND nicht schon dormant (kein Doppel-Hide).
fn should_unmap_pins(
    user_visible: bool,
    pins_active: bool,
    game_running: bool,
    mapped: bool,
    dormant: bool,
) -> bool {
    !user_visible && pins_active && game_running && mapped && !dormant
}

/// Reaktion auf einen Vordergrund-Wechsel (App-Logik, plattformübergreifend
/// gehalten und unit-getestet; `is_sc` = Vordergrund gehört zu Star Citizen).
fn handle_fg_change<R: tauri::Runtime>(app: &tauri::AppHandle<R>, is_sc: bool) {
    if is_sc {
        // Rückkehr zu SC → Pins neu mappen (nur wenn dormant, sonst wäre
        // jeder SC-Fokus-Wechsel ein refresh — gleiches Swap-Muster wie der
        // Linux-Pfad: swap(false) ist true genau beim ersten Wechsel).
        if crate::overlay::PINS_DORMANT.swap(false, Ordering::SeqCst) {
            log::warn!("focus-watch: SC refocused -> re-map pins");
            crate::overlay::refresh_visibility(app);
        }
        return;
    }
    let (user_visible, game_running) = app
        .try_state::<crate::overlay::OverlayState>()
        .map(|st| {
            (
                st.user_visible.load(Ordering::SeqCst),
                st.game_running.load(Ordering::SeqCst),
            )
        })
        .unwrap_or((false, false));
    let pins_active = {
        let (pins_enabled, pinned) = crate::pins::pins_activity(app);
        pins_enabled && pinned > 0
    };
    if !user_visible
        && pins_active
        && game_running
        && !crate::overlay::PINS_DORMANT.load(Ordering::SeqCst)
    {
        let win = app.get_webview_window(crate::overlay::SPOTLIGHT_LABEL);
        let mapped = win
            .as_ref()
            .map(|w| w.is_visible().unwrap_or(false))
            .unwrap_or(false);
        if should_unmap_pins(user_visible, pins_active, game_running, mapped, false) {
            log::warn!("focus-watch: foreign focus while pins-only -> unmap pins");
            crate::overlay::PINS_DORMANT.store(true, Ordering::SeqCst);
            if let Some(w) = win {
                let _ = w.hide();
            }
        }
    }
}

/// Wächter beim App-Start starten (passiv, kein Lifetime-Management — der
/// Thread lebt bis zum Prozessende, Kostet außerhalb der Dedepe nur einen
/// 400-ms-Timer). Non-Windows: No-op.
pub fn install<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    #[cfg(target_os = "windows")]
    {
        let app = app.clone();
        let spawned = std::thread::Builder::new()
            .name("arch-focus-watch".into())
            .spawn(move || watch_thread(app));
        match spawned {
            Ok(_) => log::info!("focus-watch thread spawned (400ms foreground poll)"),
            Err(err) => log::warn!("focus-watch thread spawn failed: {err}"),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app; // Linux: Hyprland-Socket-Pfad in overlay.rs deckt das ab.
    }
}

#[cfg(target_os = "windows")]
fn watch_thread<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };

    let mut last_hwnd: usize = 0;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(POLL_MS));
        let fg = unsafe { GetForegroundWindow() };
        let fg_addr = fg as usize;
        if fg_addr == last_hwnd || fg.is_null() {
            continue;
        }
        last_hwnd = fg_addr;
        let mut pid: u32 = 0;
        unsafe { GetWindowThreadProcessId(fg, &mut pid) };
        // Eigene Fenster (Palette, Dialoge): kein Fremd-Fokus, kein SC-Wechsel.
        if pid == 0 || pid == std::process::id() {
            continue;
        }
        let is_sc = (|| {
            let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
            if handle.is_null() {
                return false;
            }
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len) };
            unsafe { CloseHandle(handle) };
            if ok == 0 {
                return false;
            }
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            std::path::Path::new(&path)
                .file_name()
                .map(|f| f.to_string_lossy().eq_ignore_ascii_case("starcitizen.exe"))
                .unwrap_or(false)
        })();
        handle_fg_change(&app, is_sc);
    }
}

// ---------------------------------------------------------------------------
// Tests (reine Logik — Spiegel des Linux-Pins-Lifecycle-Vertrags)
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmap_requires_pins_only_context() {
        // pins-only aktiv + gemappt + fremder Fokus → unmap.
        assert!(should_unmap_pins(false, true, true, true, false));
        // Palette offen → nie (User sieht bewusst).
        assert!(!should_unmap_pins(true, true, true, true, false));
        // Spiel aus → §9.1.5 (kein Kanal).
        assert!(!should_unmap_pins(false, true, false, true, false));
        // Bereits dormant → kein Doppel-Hide.
        assert!(!should_unmap_pins(false, true, true, true, true));
        // Fenster gar nicht gemappt → nothing to do.
        assert!(!should_unmap_pins(false, true, true, false, false));
        // Keine aktiven Pins → nichts zu unmappen.
        assert!(!should_unmap_pins(false, false, true, true, false));
    }
}
