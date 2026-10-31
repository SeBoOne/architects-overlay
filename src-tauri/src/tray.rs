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
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::tray::TrayIconBuilder;

    let toggle = MenuItem::with_id(app, "toggle", "Overlay ein-/ausblenden", true, None::<&str>)?;
    let config = MenuItem::with_id(app, "config", "Config öffnen", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Beenden", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&toggle, &config, &separator, &quit])?;

    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-drake-32.png"))
        .expect("embedded tray icon decodes");

    let result = TrayIconBuilder::with_id("architects-overlay-tray")
        .icon(icon)
        .tooltip("SC Architects Overlay")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle" => crate::overlay::toggle_visible(app),
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
