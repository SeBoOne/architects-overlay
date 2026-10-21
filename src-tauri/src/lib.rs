// Tauri v2 app shell. Owns the overlay window scaffold (see `overlay`): the
// spotlight window, its global shortcuts, click-through and the Star-Citizen
// process poller. The `overlay` module is desktop-only (the global-shortcut
// plugin and /proc scanning are not mobile concepts), so it and its IPC
// commands are cfg-gated. The `trade` module adds the UEX-backed search /
// prices / sync commands the palette frontend calls — also desktop-only
// (it needs the app-data-dir cache path).

use std::sync::Arc;

#[cfg(desktop)]
mod ipc_socket;
#[cfg(desktop)]
mod overlay;
#[cfg(desktop)]
mod trade;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    #[cfg(desktop)]
    {
        builder = builder.invoke_handler(tauri::generate_handler![
            overlay::overlay_get_state,
            overlay::overlay_set_click_through,
            overlay::overlay_set_visible,
            overlay::ping,
            trade::search,
            trade::get_prices,
            trade::sync_status,
            trade::sync_now,
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

            #[cfg(desktop)]
            {
                setup_trade(app)?;

                // Create the spotlight window, register its global shortcuts and start
                // the game-detection poller. Also creates our shared overlay state.
                overlay::init(app)?;

                // Local control socket: lets the desktop session (e.g. Hyprland
                // keybinds on Wayland, where global shortcuts are unreliable) drive
                // the overlay. Best-effort — must not block startup.
                ipc_socket::init(app.handle());
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}

/// Open the local SQLite cache, build the UEX provider and start the
/// background reference-data sync. Shared state lands in `TradeState` for the
/// IPC commands.
#[cfg(desktop)]
fn setup_trade(app: &tauri::App) -> tauri::Result<()> {
    use tauri::Manager;

    let cache_path = app
        .path()
        .app_local_data_dir()?
        .join("cache.sqlite3");
    let cache = Arc::new(cache::Cache::open(&cache_path).map_err(|e| {
        std::io::Error::other(format!("cache open at {}: {e}", cache_path.display()))
    })?);

    // API key is optional (read endpoints work without it); a broken config
    // must not prevent the overlay from starting.
    let api_key = settings::Settings::load()
        .ok()
        .and_then(|s| s.uex_api_key().map(str::to_owned));

    let provider: Arc<dyn provider::DataProvider> = Arc::new(
        uex_client::UexProvider::new(cache.clone(), api_key).map_err(|e| {
            std::io::Error::other(format!("UEX provider init: {e}"))
        })?,
    );

    let tracker = Arc::new(trade::SyncTracker::default());
    app.manage(trade::TradeState {
        provider: provider.clone(),
        cache: cache.clone(),
        tracker: tracker.clone(),
    });

    // The background freshness loop (commodities 1h / items 4h / terminals 12h)
    // is started lazily on the first on-demand sweep — see `trade::sync_now` /
    // `trade::maybe_start_background`. It runs on Tauri's async (Tokio) runtime,
    // which is exactly the context the tokio-based provider futures need.

    Ok(())
}
