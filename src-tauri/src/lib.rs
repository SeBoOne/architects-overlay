// Tauri v2 app shell. Owns the overlay window scaffold (see `overlay`): the
// spotlight window, its global shortcuts, click-through and the Star-Citizen
// process poller. The `overlay` module is desktop-only (the global-shortcut
// plugin and /proc scanning are not mobile concepts), so it and its IPC
// commands are cfg-gated. The `trade` module adds the UEX-backed search /
// prices / sync commands the palette frontend calls — also desktop-only
// (it needs the app-data-dir cache path).

use std::sync::Arc;

#[cfg(desktop)]
mod app_settings;
mod hangar;
#[cfg(desktop)]
mod hyprland_sync;
#[cfg(desktop)]
mod info;
#[cfg(desktop)]
mod inventory;
#[cfg(desktop)]
mod ipc_socket;
#[cfg(desktop)]
mod overlay;
#[cfg(desktop)]
mod providers;
#[cfg(desktop)]
mod routes;
mod scwiki_sync;
#[cfg(desktop)]
mod trade;
#[cfg(desktop)]
mod tray;
#[cfg(desktop)]
mod updater;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    #[cfg(desktop)]
    {
        builder = builder.invoke_handler(tauri::generate_handler![
            overlay::overlay_get_state,
            overlay::overlay_set_click_through,
            overlay::overlay_set_visible,
            trade::search,
            hangar::hangar_list,
            hangar::hangar_add,
            hangar::hangar_update,
            hangar::hangar_delete,
            hangar::hangar_import,
            hangar::hangar_export,
            scwiki_sync::modul_compat_status,
            scwiki_sync::modul_compat_refresh,
            scwiki_sync::modul_for_ship,
            trade::math_eval,
            trade::get_prices,
            trade::sync_status,
            trade::sync_now,
            trade::refresh_item_prices_now,
            trade::refresh_commodity_prices_now,
            trade::prices_sync_ages,
            trade::get_marketplace,
            trade::get_my_offers,
            trade::get_my_trades,
            trade::get_my_negotiations,
            trade::get_negotiation_messages,
            trade::send_negotiation_message,
            trade::marketplace_photo,
            trade::marketplace_advertise,
            trade::marketplace_delete,
            trade::get_categories,
            trade::item_suggest,
            trade::trade_add,
            trade::trade_edit,
            trade::trade_remove,
            trade::commodity_suggest,
            trade::terminal_suggest,
            trade::trade_location_suggest,
            trade::trade_commodity_suggest,
            trade::marketplace_confirm_mode,
            trade::open_marketplace_listing,
            trade::open_negotiation_deal,
            trade::open_marketplace_trades,
            trade::open_marketplace_trades_home,
            trade::pending_offers_remove,
            trade::open_uex_api_apps,
            trade::open_uex_account,
            trade::open_uex_trade_log,
            trade::open_created_listing,
            info::open_info_link,
            inventory::inventory_list,
            inventory::inventory_locations,
            inventory::inventory_add,
            inventory::inventory_update,
            inventory::inventory_delete,
            routes::get_routes,
            routes::get_route_locations,
            routes::vehicles_scu,
            routes::location_suggest,
            app_settings::settings_get,
            app_settings::settings_set_updater_enabled,
            app_settings::settings_set_updater_interval,
            updater::updater_status,
            updater::updater_check_now,
            updater::updater_install,
            app_settings::settings_set_palette,
            app_settings::settings_set_detail_variant,
            app_settings::settings_set_uex_api_key,
            app_settings::settings_set_uex_secret_key,
            app_settings::settings_resolve_uex_username,
            app_settings::settings_set_marketplace_confirm,
            app_settings::settings_set_scale,
            app_settings::settings_set_shortcut_toggle,
            app_settings::settings_set_shortcut_click_through,
            app_settings::settings_set_shortcuts_sync,
            app_settings::settings_set_mode_hotkey,
            app_settings::settings_reset_mode_hotkeys,
            overlay::overlay_config_closed,
        ]);
    }

    builder
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            #[cfg(all(desktop, target_os = "linux", debug_assertions))]
            log::info!(
                "WM_CLASS probe: prgname={:?} gdk_program_class={:?}",
                glib::prgname().map(|s| s.to_string()),
                unsafe { std::ffi::CStr::from_ptr(gdk_sys::gdk_get_program_class()) }
                    .to_string_lossy()
                    .into_owned(),
            );

            // Pin the WM_CLASS to the documented lowercase class BEFORE the
            // first window map (verified live mechanism, t_68c09e76): GDK
            // overwrites any pre-init preset with its capitalized-prgname
            // default at display open — but window realize re-reads the
            // global program class when writing XSetClassHint. setup() runs
            // after display open and before any window maps, so this is the
            // one point where the pin actually lands.
            #[cfg(all(desktop, target_os = "linux"))]
            pin_wm_class();

            #[cfg(desktop)]
            {
                // Settings first — the frontend reads its snapshot at boot to
                // apply data-palette / data-detail-variant.
                tauri::Manager::manage(app, app_settings::SettingsState::load());
                tauri::Manager::manage(app, app_settings::SyncNote::default());
                // UC t_d323be7e: Update-State + periodischer Check (Start +
                // alle N Stunden, Karte Umsetzung 1). Eigener Thread, kein
                // Block des Setups — Fehler landen nur in der Note.
                tauri::Manager::manage(app, updater::UpdaterState::default());
                updater::start_background_check(app.handle().clone());

                setup_trade(app)?;

                // Create the spotlight window, register its global shortcuts and start
                // the game-detection poller. Also creates our shared overlay state.
                overlay::init(app)?;

                // Local control socket: lets the desktop session (e.g. Hyprland
                // keybinds on Wayland, where global shortcuts are unreliable) drive
                // the overlay. Best-effort — must not block startup.
                ipc_socket::init(app.handle());

                // C2b (t_1adbc017): Wayland/Omarchy-Driftsync beim Start —
                // nur wenn Consent (`[shortcuts] sync_omarchy`) gesetzt ist,
                // ein Hyprland läuft und das OmaSettings-Plugin existiert.
                // Läuft im Thread mit kurzem Verzug: Setup muss davon nichts
                // blockieren, ein Fehler ist rein sichtbares Log + Note.
                {
                    let handle = app.handle().clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(1500));
                        app_settings::startup_hypr_sync(&handle);
                    });
                }

                // System tray (Overlay-Toggle / Config öffnen / Beenden).
                // Fails soft internally (see tray::init).
                tray::init(app)?;
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}

/// Pin the X11/WM_CLASS window identity to exactly the documented lowercase
/// class `architects-overlay` (root-cause fix, review finding t_159d16a2 →
/// card t_68c09e76).
///
/// tao creates Linux windows as `gtk::ApplicationWindow`. GDK derives the
/// WM_CLASS *class* from the program name, capitalizing its first letter
/// (`architects-overlay` → `Architects-overlay`) at display open — it
/// overwrites any preset made earlier there. Window realize, however,
/// re-reads the global program class when writing XSetClassHint. That
/// mechanism was established empirically on the live session (pre-init pin:
/// no effect; setup-time pin before first map: `hyprctl clients` finally
/// showed lowercase class, rules applied, focus dispatch `ok`).
///
/// Hyprland windowrules and `focuswindow class:…` match case-sensitively,
/// so the mismatch silently killed float/pin AND the keyboard-focus bridge —
/// the overlay ran as a normal tiled client. Call this from `setup`, after
/// display open and before any window maps. Both setters only store strings;
/// the unsafe FFI call passes a NUL-terminated C string into a function
/// documented to strdup it (the safe gdk binding would panic pre-init).
#[cfg(all(desktop, target_os = "linux"))]
fn pin_wm_class() {
    let class = overlay::SPOTLIGHT_WM_CLASS;
    glib::set_prgname(Some(class));
    let c_class = std::ffi::CString::new(class).expect("class contains no NUL");
    unsafe { gdk_sys::gdk_set_program_class(c_class.as_ptr()) };
}

/// Open the local SQLite cache, build the UEX provider and start the
/// background reference-data sync. Shared state lands in `TradeState` for the
/// IPC commands.
#[cfg(desktop)]
fn setup_trade(app: &tauri::App) -> tauri::Result<()> {
    use tauri::Manager;

    let cache_path = app.path().app_local_data_dir()?.join("cache.sqlite3");
    let cache = Arc::new(cache::Cache::open(&cache_path).map_err(|e| {
        std::io::Error::other(format!("cache open at {}: {e}", cache_path.display()))
    })?);

    // API key is optional (read endpoints work without it); a broken config
    // must not prevent the overlay from starting.
    let api_key = settings::Settings::load()
        .ok()
        .and_then(|s| s.uex_api_key().map(str::to_owned));
    // M2a-Fix (t_2f197d1c): persönlicher Secret-Key ist ein EIGENER Slot —
    // User-scoped Reads (/user_trades, eigene marketplace_listings) senden ihn
    // als `secret-key`-Header; der App-Token bleibt reiner Bearer.
    let secret_key = settings::Settings::load()
        .ok()
        .and_then(|s| s.uex_secret_key().map(str::to_owned));

    let uex = uex_client::UexProvider::new(cache.clone(), api_key)
        .map_err(|e| std::io::Error::other(format!("UEX provider init: {e}")))?
        .with_secret_key(secret_key);

    // PS (t_c84e545d): uex-only composite — no wiki provider, no source
    // switch. Old persisted `commodity_source` values are ignored by the
    // settings crate (unknown key).
    let composite = Arc::new(providers::CompositeProvider::new(uex, cache.clone()));
    let provider: Arc<dyn provider::DataProvider> = composite.clone();

    let tracker = Arc::new(trade::SyncTracker::default());
    tauri::Manager::manage(
        app,
        trade::TradeState {
            provider: provider.clone(),
            cache: cache.clone(),
            tracker: tracker.clone(),
            composite: composite.clone(),
            write_gate: Arc::new(trade::WriteGate::new()),
        },
    );

    // Start the background freshness/health loop once at boot (see
    // trade::start_background_refresh). Independent of any on-demand command so
    // a warm cache with a dead network still flips the UI to OFFLINE — that
    // flips `tracker.last_error`, which the frontend's `online` flag reads.
    // Runs on Tauri's async (Tokio) runtime, exactly the context the
    // tokio-based provider futures need. The hourly commodity-price sync and
    // the settings-driven manual refreshes also run through this handle —
    // NEITHER is coupled to overlay visibility (PS t_c84e545d).
    let handle = app.handle();
    trade::start_background_refresh(handle, &provider, &cache, &composite, &tracker);

    // S3 (t_988e132f): stündlicher Modul-Kompatibilitäts-Check + erster Lauf
    // kurz nach dem Boot — unabhängig vom Overlay sichtbar (wie price-sync).
    scwiki_sync::spawn_modul_sync(cache.clone());

    // Dev shell: a second, hidden window (label "main") showing the same
    // SvelteKit palette as the spotlight. Release builds must NOT create it —
    // an unused webview would add a ~70 MiB WebKitWebProcess (review A2). The
    // config-level window was removed from tauri.conf.json; only debug/dev
    // builds get one, created here programmatically.
    #[cfg(debug_assertions)]
    {
        let _ = tauri::WebviewWindowBuilder::new(
            app,
            "main",
            tauri::WebviewUrl::App("index.html".into()),
        )
        .title("SC Architects Overlay")
        .inner_size(800., 600.)
        .resizable(true)
        .visible(false)
        .build();
    }

    Ok(())
}
