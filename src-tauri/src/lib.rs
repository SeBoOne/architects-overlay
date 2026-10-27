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
mod inventory;
#[cfg(desktop)]
mod ipc_socket;
#[cfg(desktop)]
mod overlay;
#[cfg(desktop)]
mod providers;
#[cfg(desktop)]
mod routes;
#[cfg(desktop)]
mod trade;
#[cfg(desktop)]
mod tray;

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
            trade::math_eval,
            trade::get_prices,
            trade::sync_status,
            trade::sync_now,
            trade::get_marketplace,
            trade::open_marketplace_listing,
            inventory::inventory_list,
            inventory::inventory_locations,
            inventory::inventory_add,
            inventory::inventory_update,
            inventory::inventory_delete,
            routes::get_routes,
            routes::vehicles_scu,
            routes::location_suggest,
            app_settings::settings_get,
            app_settings::settings_set_palette,
            app_settings::settings_set_detail_variant,
            app_settings::settings_set_uex_api_key,
            app_settings::settings_set_commodity_source,
            app_settings::settings_set_scale,
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

                setup_trade(app)?;

                // Create the spotlight window, register its global shortcuts and start
                // the game-detection poller. Also creates our shared overlay state.
                overlay::init(app)?;

                // Local control socket: lets the desktop session (e.g. Hyprland
                // keybinds on Wayland, where global shortcuts are unreliable) drive
                // the overlay. Best-effort — must not block startup.
                ipc_socket::init(app.handle());

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

/// The source switch handed to the composite provider. This MUST be a CLONE
/// of the settings state's live switch (a clone shares the inner `Arc`), not
/// a freshly constructed switch from the boot value — a fresh switch would
/// freeze the boot source and a runtime flip in
/// `settings_set_commodity_source` would only apply after a restart
/// (regression t_c3d46cff). Single point of wiring so the regression test
/// below exercises the exact expression production uses.
#[cfg(desktop)]
pub(crate) fn composite_source_switch(
    state: &app_settings::SettingsState,
) -> crate::providers::SourceSwitch {
    state.source_switch.clone()
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

    let uex = uex_client::UexProvider::new(cache.clone(), api_key)
        .map_err(|e| std::io::Error::other(format!("UEX provider init: {e}")))?;
    let wiki = wiki_client::WikiProvider::new(cache.clone())
        .map_err(|e| std::io::Error::other(format!("wiki provider init: {e}")))?;

    // The settings state owns the LIVE commodity-source switch (the config
    // view flips it); the composite provider reads it on every price fetch.
    // CLONE — do not construct a second switch here, or the runtime flip in
    // `settings_set_commodity_source` would only apply after a restart.
    let settings_state = app.state::<app_settings::SettingsState>();
    let source = composite_source_switch(&settings_state);

    let composite = Arc::new(providers::CompositeProvider::new(
        uex,
        wiki,
        source,
        cache.clone(),
    ));
    let provider: Arc<dyn provider::DataProvider> = composite.clone();
    // The composite IS the wiki sync (its ensure helpers carry the
    // patch-driven TTLs).
    let wiki_sync: Arc<dyn providers::WikiSync> = composite.clone() as Arc<dyn providers::WikiSync>;

    let tracker = Arc::new(trade::SyncTracker::default());
    tauri::Manager::manage(
        app,
        trade::TradeState {
            provider: provider.clone(),
            cache: cache.clone(),
            tracker: tracker.clone(),
            wiki_sync: Some(wiki_sync.clone()),
        },
    );

    // Start the background freshness/health loop once at boot (see
    // trade::start_background_refresh). Independent of any on-demand command so
    // a warm cache with a dead network still flips the UI to OFFLINE — that
    // flips `tracker.last_error`, which the frontend's `online` flag reads.
    // Runs on Tauri's async (Tokio) runtime, exactly the context the
    // tokio-based provider futures need.
    let handle = app.handle();
    trade::start_background_refresh(handle, &provider, &cache, Some(&wiki_sync), &tracker);

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

#[cfg(all(test, desktop))]
mod wiring_tests {
    //! Regression t_c3d46cff: the composite provider must be built on the
    //! SAME live `SourceSwitch` the settings setter flips — a runtime flip
    //! (`settings_set_commodity_source`) must change the provider's source
    //! selection on the NEXT price fetch, without an app restart.
    //!
    //! The test drives the REAL wiring point (`composite_source_switch`, the
    //! exact expression `setup_trade` uses) and asserts the provider's
    //! QUOTENWAHL (which price table comes back), not just switch internals.
    //! Both clients are wired against unreachable hosts: every dataset is
    //! pre-seeded in the cache, so no HTTP happens and the offline failure
    //! paths are deterministic.

    use super::*;
    use cache::{Cache, SOURCE_UEX, SOURCE_WIKI};
    use provider::{DataProvider, Entity, EntityKind, PricePoint};
    use settings::CommoditySource;
    use std::sync::Arc;
    use std::time::Duration;
    use uex_client::UexProvider;
    use wiki_client::{CommodityRow, WikiProvider};

    fn point(commodity: &str, commodity_id: u64, terminal_id: u64, buy: f64) -> PricePoint {
        PricePoint {
            commodity: commodity.to_owned(),
            commodity_id,
            location: format!("terminal {terminal_id}"),
            terminal_id,
            sell_price: 0.0,
            buy_price: buy,
            sell_stock: 0.0,
            buy_stock: 0.0,
            sell_status: 0,
            buy_status: 1,
            quality: 0,
            updated: 1,
        }
    }

    #[tokio::test]
    async fn runtime_flip_changes_provider_source_selection() {
        let cache = Arc::new(Cache::open_in_memory().expect("cache"));

        // UEX reference dataset + price table: Agricium → terminal 12.
        cache
            .store_entities(
                EntityKind::Commodities,
                &[Entity {
                    id: 50,
                    kind: EntityKind::Commodities,
                    name: "Agricium".into(),
                    code: Some("agricium".into()),
                    subtitle: String::new(),
                }],
            )
            .expect("seed uex commodities");
        cache
            .store_prices_sourced(&[point("Agricium", 50, 12, 8_000.0)], SOURCE_UEX)
            .expect("seed uex prices");

        // Wiki reference dataset + price table: Laranite (uuid u2) →
        // terminal 778, DIFFERENT terminal id so the coming-back table
        // unambiguously names its source.
        let wiki = WikiProvider::with_base_url("http://127.0.0.1:2", Duration::ZERO, cache.clone())
            .expect("wiki");
        let wiki_entity = wiki.map_commodities(vec![CommodityRow {
            uuid: "u2".into(),
            name: "Laranite".into(),
            slug: "laranite".into(),
            ..Default::default()
        }]);
        cache
            .store_entities_sourced(EntityKind::Commodities, SOURCE_WIKI, &wiki_entity)
            .expect("seed wiki commodities");
        let wiki_id = wiki_entity[0].id;
        cache
            .store_prices_sourced(&[point("Laranite", wiki_id, 778, 8_379.0)], SOURCE_WIKI)
            .expect("seed wiki prices");

        // REAL wiring: same expression setup_trade uses (lib.rs
        // `composite_source_switch(&settings_state)`).
        let state = app_settings::SettingsState::load();
        let source = composite_source_switch(&state);
        let uex =
            UexProvider::with_base_url(cache.clone(), None, "http://127.0.0.1:1", Duration::ZERO)
                .expect("uex");
        let provider = crate::providers::CompositeProvider::new(uex, wiki, source, cache);

        // NORMALIZE to wiki first so the test does not depend on the boot
        // value of the developer's real config.
        state.set_commodity_source_live(CommoditySource::Wiki);
        assert_eq!(provider.name(), "wiki+uex (wiki)");

        // Wiki mode: the WIKI table comes back (terminal 778), UEX only
        // fills gaps — and UEX has no Laranite (offline), so nothing else.
        let points = provider.prices("Laranite").await.expect("wiki-mode prices");
        assert!(
            points
                .iter()
                .any(|p| p.terminal_id == 778 && p.buy_price == 8_379.0),
            "wiki mode must serve the wiki price table, got {points:?}"
        );
        assert!(
            !points.iter().any(|p| p.terminal_id == 12),
            "wiki mode must not leak the UEX-only table"
        );

        // FLIP via the setter path — the next fetch must use UEX WITHOUT a
        // restart. On the pre-fix revision (fresh switch from the boot value)
        // this stayed in wiki mode and the assertions below failed.
        state.set_commodity_source_live(CommoditySource::Uex);
        assert_eq!(provider.name(), "wiki+uex (uex)");

        // UEX mode: pure-UEX path — Agricium resolves in the UEX reference
        // data and serves the UEX table (terminal 12).
        let points = provider.prices("Agricium").await.expect("uex-mode prices");
        assert!(
            points
                .iter()
                .any(|p| p.terminal_id == 12 && p.buy_price == 8_000.0),
            "uex mode must serve the uex price table, got {points:?}"
        );
        // K6 (t_9b385831 P2): UEX mode + failing UEX fetch (offline here;
        // Laranite is absent from the UEX dataset) must FALL BACK to the
        // wiki table — the UI promises 'UEX primär (Wiki-Fallback)' (the
        // pre-K6 pure-UEX behavior asserted is_err() at exactly this point).
        let res = provider
            .prices("Laranite")
            .await
            .expect("uex mode: failing UEX fetch must fall back to the wiki table");
        assert!(
            res.iter()
                .any(|p| p.terminal_id == 778 && p.buy_price == 8_379.0),
            "uex-mode fallback must serve the wiki table, got {res:?}"
        );

        // Flip back — the provider follows again (no restart, both ways).
        state.set_commodity_source_live(CommoditySource::Wiki);
        let points = provider.prices("Laranite").await.expect("back to wiki");
        assert!(points.iter().any(|p| p.terminal_id == 778));
    }
}
