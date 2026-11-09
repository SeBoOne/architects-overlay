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

#[cfg(desktop)]
pub fn init<R: tauri::Runtime>(app: &tauri::App<R>) -> tauri::Result<()> {
    use tauri::Manager;
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::tray::TrayIconBuilder;

    // T2 (t_c5904bcb): Tray-Labels folgen der persistierten Sprache
    // (de → deutsch, sonst en-Default). Das Tray-Menü wird beim
    // Sprachwechsel nicht live neu gebaut — erst nach Neustart (dokumentiert).
    let de = crate::app_settings::SettingsState::current_language_is_de();
    let (toggle_label, config_label, quit_label) = if de {
        ("Overlay ein-/ausblenden", "Config öffnen", "Beenden")
    } else {
        ("Toggle overlay", "Open config", "Quit")
    };
    // M1 (t_90cb34ee, Scope 5): „Fenstermodus zeigen/verstecken" als Checkbox
    // unterhalb `toggle` — Zustand wird von window::show/hide live nachgezogen
    // (window::WindowTrayItem). i18n-Pattern wie oben; Live-Sprachwechsel
    // bleibt M3 (Sebo §9.1.7, Feinschliff).
    let window_label = if de {
        "Fenstermodus zeigen/verstecken"
    } else {
        "Toggle window mode"
    };
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
