// Tauri v2 app shell. Owns the overlay window scaffold (see `overlay`): the
// spotlight window, its global shortcuts, click-through and the Star-Citizen
// process poller. The `overlay` module is desktop-only (the global-shortcut
// plugin and /proc scanning are not mobile concepts), so it and its IPC
// commands are cfg-gated.

#[cfg(desktop)]
mod overlay;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  let mut builder = tauri::Builder::default();

  #[cfg(desktop)]
  {
    builder = builder.invoke_handler(tauri::generate_handler![
      overlay::overlay_get_state,
      overlay::overlay_set_click_through,
    ]);
  }

  builder
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }

      // Create the spotlight window, register its global shortcuts and start
      // the game-detection poller. Also creates our shared overlay state.
      #[cfg(desktop)]
      overlay::init(app)?;

      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while building tauri application");
}