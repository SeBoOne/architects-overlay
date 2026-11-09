//! System tray icon with the three menu actions from the tray spec:
//! **Overlay ein-/ausblenden**, **Config öffnen**, **Beenden**.
//!
//! Design decisions (from `tray-icon-entscheidung.md`, task t_ee57b8d6):
//!
//! - Rust-side `MenuBuilder`/`TrayIconBuilder` (Option A) — three fixed menu
//!   points need no JS API.
//! - **Beenden = `app.exit(0)`** (Option C): the app runs as a transient
//!   systemd user unit (`architects-overlay.service`, `Restart=no`), so a
//!   clean exit code 0 terminates the unit without a restart loop. No
//!   `systemctl` subprocess involved — the same binary also runs without
//!   systemd (manual start, later Windows builds).
//! - Tray creation fails soft: a missing tray daemon (e.g. no status-notifier
//!   provider on some Wayland setups) must not prevent the overlay from
//!   starting. Linux-only until the Windows pipeline exists; the code is
//!   desktop-gated like the rest of the shell modules.
//!
//! The icon is the Drake brand emblem (`brand/logo/logo.svg`), 32×32 RGBA,
//! embedded at compile time (see `brand/logo/rebuild-icons.sh`).

use std::sync::Mutex;

use tauri::Manager;
use tauri::menu::MenuItem;

/// M3 (t_d9c09acf, Scope 1): Handles aller i18n-relevanten Tray-Menüpunkte.
/// Tauri 2 erlaubt `MenuItem::set_text`/`CheckMenuItem::set_text` zur
/// Laufzeit (menu_item.rs `pub fn set_text`, belegt an tauri 2.12.4) — der
/// Tray zieht deshalb beim Sprachwechsel LIVE nach, kein Neustart nötig
/// (Sebo-Beschluss §9.1.7). Fail-soft: fehlt der Tray, tut apply_language
/// nichts.
pub struct TrayMenuItems<R: tauri::Runtime>(pub Mutex<Option<TrayHandles<R>>>);

pub struct TrayHandles<R: tauri::Runtime> {
    toggle: MenuItem<R>,
    window_toggle: tauri::menu::CheckMenuItem<R>,
    config: MenuItem<R>,
    quit: MenuItem<R>,
}

/// Pure Label-Mapping-Funktion (unit-getestet): de → deutsche Tray-Labels,
/// sonst en-Default. Bewusst NUR en/de (Rust-Seite kennt keine Locale-Files;
/// Community-Sprachen zeigen tray-seitig den en-Default — dokumentierte
/// Grenze, Frontend-Pipeline bleibt der Ort für beliebige Sprachen).
pub fn labels(de: bool) -> (&'static str, &'static str, &'static str, &'static str) {
    if de {
        (
            "Overlay ein-/ausblenden",
            "Fenstermodus zeigen/verstecken",
            "Config öffnen",
            "Beenden",
        )
    } else {
        (
            "Toggle overlay",
            "Toggle window mode",
            "Open config",
            "Quit",
        )
    }
}

/// M3 Scope 1: Tray-Labels live auf die aktuell persistierte Sprache
/// umschalten (von `settings_set_language` aufgerufen — der einzige Pfad,
/// der die Sprache ändert). Kein Neustart-Angebot nötig: set_text macht die
/// Neustart-Fallback-Rückfrage (Toast/Modal) obsolet.
pub fn apply_language<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let de = crate::app_settings::SettingsState::current_language_is_de();
    let (toggle_label, window_label, config_label, quit_label) = labels(de);
    let Some(state) = app.try_state::<TrayMenuItems<R>>() else {
        return;
    };
    let guard = state.0.lock().expect("tray menu lock");
    let Some(h) = guard.as_ref() else {
        return;
    };
    let _ = h.toggle.set_text(toggle_label);
    let _ = h.window_toggle.set_text(window_label);
    let _ = h.config.set_text(config_label);
    let _ = h.quit.set_text(quit_label);
}

#[cfg(desktop)]
pub fn init<R: tauri::Runtime>(app: &tauri::App<R>) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::tray::TrayIconBuilder;

    let de = crate::app_settings::SettingsState::current_language_is_de();
    let (toggle_label, window_label, config_label, quit_label) = labels(de);
    let toggle = MenuItem::with_id(app, "toggle", toggle_label, true, None::<&str>)?;
    let window_toggle = tauri::menu::CheckMenuItem::with_id(
        app,
        "window_toggle",
        window_label,
        true,
        false,
        None::<&str>,
    )?;
    let config = MenuItem::with_id(app, "config", config_label, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", quit_label, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&toggle, &window_toggle, &config, &separator, &quit])?;

    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-drake-32.png"))
        .expect("embedded tray icon decodes");

    // M3: Handles für die Live-Sprachumschaltung (apply_language) merken.
    app.manage(TrayMenuItems::<R>(Mutex::new(Some(TrayHandles {
        toggle: toggle.clone(),
        window_toggle: window_toggle.clone(),
        config: config.clone(),
        quit: quit.clone(),
    }))));

    // M1: Handle für den Checkbox-Zustand an window.rs übergeben.
    if let Some(item) = app.try_state::<crate::window::WindowTrayItem<R>>() {
        *item.0.lock().expect("tray item lock") = Some(window_toggle);
    }

    let result = TrayIconBuilder::with_id("architects-overlay-tray")
        .icon(icon)
        .tooltip("SC Architects Overlay")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle" => crate::overlay::toggle_visible(app),
            // M1: Checkbox-Zustand zieht window::toggle_visible selbst nach
            // (show/hide rufen sync_tray_check).
            "window_toggle" => crate::window::toggle_visible(app),
            "config" => crate::overlay::open_config_from_tray(app),
            // Clean exit (code 0): the systemd unit is transient with
            // `Restart=no` (verified in the tray triage), so the unit stops
            // normally — no restart loop, no `systemctl` subprocess.
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app);

    if let Err(err) = result {
        // Fail soft: tray unavailable (no status-notifier daemon) is a UX
        // limitation, not a startup blocker. All functionality stays reachable
        // via global shortcuts and the arch-overlay IPC socket.
        log::warn!("tray icon unavailable ({err}) — continuing without tray");
    }

    Ok(())
}

#[cfg(not(desktop))]
pub fn init<R: tauri::Runtime>(_app: &tauri::App<R>) -> tauri::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_de_and_en_are_complete_and_distinct() {
        let (t_de, w_de, c_de, q_de) = labels(true);
        let (t_en, w_en, c_en, q_en) = labels(false);
        for (de, en) in [(t_de, t_en), (w_de, w_en), (c_de, c_en), (q_de, q_en)] {
            assert!(!de.is_empty() && !en.is_empty());
            assert_ne!(de, en, "de/en-Label identisch — Mapping kaputt");
        }
    }
}
