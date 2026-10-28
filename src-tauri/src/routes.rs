//! Handelsrouten (Karte t_128b7547): IPC-Commands für die Routen-Ansicht im
//! Marktmodus. Die eigentliche Beschaffung (UEX-Sweep) und Berechnung leben
//! im `uex_client`-/`provider`-Crate — hier sitzt nur die Tauri-Schicht.

use std::collections::HashMap;

use provider::routes::{RouteFilters, RouteList, TradeRoute};

use provider::EntityKind;

use crate::trade::{TradeState, friendly};

/// Response der Routen-Abfrage.
#[derive(Clone, serde::Serialize)]
pub struct RoutesResponse {
    /// Quellen-Label des Providers (z. B. "wiki+uex (wiki)") — die Routen
    /// selbst kommen IMMER aus dem UEX-Sweep (bewusste Entscheidung).
    pub source: String,
    /// Unix-Sekunden des letzten Routen-Sweeps (Datenstand-Anzeige).
    pub swept_at: u64,
    /// Stale-While-Revalidate: Sweep-Marker older than its TTL → the served
    /// data is (possibly) veralt, a background refresh runs (UI: ↻ chip).
    pub refreshing: bool,
    pub routes: Vec<TradeRoute>,
}

/// Handelsrouten abfragen: bester Einkauf → bester Verkauf je Ware, mit den
/// Filtern des Routen-Views. Der erste Aufruf (oder nach 30-min-TTL) stößt
/// einen Sweep über alle Commodity-Terminals an (~1 min bei kaltem Cache,
/// danach instant aus dem Cache).
#[tauri::command]
pub async fn get_routes(
    state: tauri::State<'_, crate::trade::TradeState>,
    filters: RouteFilters,
) -> Result<RoutesResponse, String> {
    let list: RouteList = state
        .provider
        .trade_routes(&filters)
        .await
        .map_err(friendly)?;
    Ok(RoutesResponse {
        source: state.provider.name(),
        swept_at: list.swept_at,
        refreshing: list.refreshing,
        routes: list.routes,
    })
}

/// Schiffs-SCU-Kapazitäten aus UEX `/vehicles` (name/name_full/slug → SCU,
/// alles lowercase). Das Frontend matcht Hangar-Schiffe dagegen und setzt
/// die SCU-Kapazität des Routen-Filters automatisch.
#[tauri::command]
pub async fn vehicles_scu(
    state: tauri::State<'_, TradeState>,
) -> Result<HashMap<String, f64>, String> {
    // Der Command braucht den UEX-Client direkt (nicht über den Composite-
    // Seam): die SCU-Tabelle ist UEX-Referenzdatum, kein Preis.
    let map = state.provider.vehicles_scu().await.map_err(friendly)?;
    Ok(map)
}

/// Alternativ-Tabelle (F2): alle Kauf-/Verkaufsorte einer Ware aus dem
/// LOKALEN Routen-Datensatz (kein neuer UEX-Call — nur Cache-Lesung). Im
/// Routen-Detail der gewählten Ware: Kauforte (Preis aufsteigend) links,
/// Verkaufsorte (Preis absteigend) rechts, aktuelle Kombi markiert.
///
/// rename_all = "snake_case" (RF2 t_71e2f03d): Tauri v2 mappt top-level
/// Command-Args per Default camelCase (→ JS `commodityId`), die Svelte-
/// Komponente sendet aber `commodity_id` (snake, wie alle Filter-Payloads).
/// Ohne dieses Attribut ist `commodity_id` NIE angebunden → invoke rejected →
/// „keine Alternativorte", obwohl die Stammdaten da sind. Der zugehörige
/// IPC-Regressiontest (`get_route_locations_ipc_accepts_snake_case_commodity_arg`)
/// dispatcht den Command über die ECHTE IPC-Schicht.
#[tauri::command(rename_all = "snake_case")]
pub async fn get_route_locations(
    state: tauri::State<'_, crate::trade::TradeState>,
    commodity_id: u64,
) -> Result<Option<provider::routes::RouteCommodityLocations>, String> {
    state
        .provider
        .route_locations(commodity_id)
        .await
        .map_err(friendly)
}

/// Autocomplete Start-/Zielort (P15c, Karte t_5fbd43b6): Locations ONLY
/// (EntityKind::Locations — Wiki+UEX merged: "Checkmate", "Levski",
/// "ARC-L1 Wild Forest Station" …). NEVER Shops/Terminals: those live as
/// EntityKind::Terminals and never enter this list (search/Detail keeps
/// showing them — the Routen-Filter just may only name Locations).
/// Rank: starts-with first, then contains; name-dedupe case-insensitive,
/// max 12 suggestions, leerer query → leerer List.
#[tauri::command]
pub fn location_suggest(
    state: tauri::State<'_, crate::trade::TradeState>,
    query: String,
) -> Result<Vec<String>, String> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let entities = state
        .cache
        .entities_of(EntityKind::Locations)
        .map_err(|e| format!("Cache-Fehler: {e}"))?;
    let mut starts: Vec<String> = Vec::new();
    let mut contains: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for e in entities {
        let name = e.name.trim();
        let key = name.to_lowercase();
        if key.is_empty() || seen.contains(&key) {
            continue;
        }
        seen.insert(key.clone());
        if key.starts_with(&q) {
            starts.push(name.to_string());
        } else if key.contains(&q) {
            contains.push(name.to_string());
        }
    }
    starts.append(&mut contains);
    starts.truncate(12);
    Ok(starts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cache::Cache;
    use provider::{DataProvider, Entity, EntityKind};
    use std::sync::Arc;
    use tauri::Manager;
    use tauri::test::mock_app;

    struct StubProvider;

    #[async_trait::async_trait]
    impl DataProvider for StubProvider {
        async fn search(
            &self,
            _e: provider::SearchEntities,
            _q: &str,
        ) -> provider::Result<Vec<provider::SearchHit>> {
            Ok(vec![])
        }
        async fn prices(&self, _c: &str) -> provider::Result<Vec<provider::PricePoint>> {
            Ok(vec![])
        }
        async fn route_locations(
            &self,
            commodity_id: u64,
        ) -> provider::Result<Option<provider::routes::RouteCommodityLocations>> {
            Ok(Some(provider::routes::RouteCommodityLocations {
                commodity_id,
                commodity: "Gold".into(),
                buys: vec![provider::routes::RouteLocation {
                    terminal_id: 1,
                    terminal: "HDMS-Anderson".into(),
                    chain: "Stanton > Hurston".into(),
                    price: 24000.0,
                    stock: 500.0,
                    status: 3,
                    autoload: false,
                    cargo_deck: false,
                    ground: true,
                }],
                sells: vec![provider::routes::RouteLocation {
                    terminal_id: 2,
                    terminal: "Port Olisar".into(),
                    chain: "Stanton > Crusader".into(),
                    price: 31000.0,
                    stock: 1000.0,
                    status: 4,
                    autoload: true,
                    cargo_deck: false,
                    ground: false,
                }],
            }))
        }
        async fn marketplace_offers(
            &self,
            _f: provider::MarketplaceFilter,
        ) -> provider::Result<Vec<provider::Offer>> {
            Ok(vec![])
        }
        async fn fetch_entities(&self, _k: EntityKind) -> provider::Result<Vec<provider::Entity>> {
            Ok(vec![])
        }
        fn name(&self) -> String {
            "stub".into()
        }
    }

    fn entity(id: u64, kind: EntityKind, name: &str, subtitle: &str) -> Entity {
        Entity {
            id,
            kind,
            name: name.into(),
            code: None,
            subtitle: subtitle.into(),
        }
    }

    /// P15c (Karte t_5fbd43b6): Start-/Zielort-Autocomplete lists Locations
    /// ONLY — never Shops/Terminals (kind=location from Wiki+UEX).
    #[test]
    fn location_suggest_lists_only_locations_p15c() {
        let app = mock_app();
        let cache = Arc::new(Cache::open_in_memory().unwrap());
        cache
            .store_entities(
                EntityKind::Locations,
                &[
                    entity(901, EntityKind::Locations, "Levski", "Nyx > Levski"),
                    entity(
                        902,
                        EntityKind::Locations,
                        "Checkmate Station",
                        "ArcCorp > Landing Zone > Checkmate Station",
                    ),
                    entity(
                        903,
                        EntityKind::Locations,
                        "ARC-L1 Wild Forest Station",
                        "ArcCorp > Arc-L1 > Wild Forest Station",
                    ),
                ],
            )
            .unwrap();
        cache
            .store_entities(
                EntityKind::Terminals,
                &[
                    entity(
                        436,
                        EntityKind::Terminals,
                        "Commodity Shop - Admin - Checkmate",
                        "ArcCorp > Landing Zone > Checkmate",
                    ),
                    entity(
                        437,
                        EntityKind::Terminals,
                        "Guns - Checkmate",
                        "ArcCorp > Landing Zone > Checkmate",
                    ),
                ],
            )
            .unwrap();
        app.manage(crate::trade::TradeState {
            provider: Arc::new(StubProvider),
            cache,
            tracker: Arc::new(crate::trade::SyncTracker::default()),
            wiki_sync: None,
            write_gate: Arc::new(crate::trade::WriteGate::new()),
        });
        let state = app.state::<TradeState>();

        // starts-with first ("Checkmate Station"); "Commodity Shop ... Checkmate"
        // and "Guns - Checkmate" (Terminals) dorfnet appear — P15c-Kriterium.
        let sug = location_suggest(state.clone(), "ch".to_string()).unwrap();
        assert_eq!(sug, vec!["Checkmate Station"]);

        let lev = location_suggest(state.clone(), "lev".to_string()).unwrap();
        assert_eq!(lev, vec!["Levski"]);

        // contains-Position: Freestring-Filters find the inner Name.
        let forest = location_suggest(state.clone(), "wild forest".to_string()).unwrap();
        assert_eq!(forest, vec!["ARC-L1 Wild Forest Station"]);

        // Leerer Query → leeer List (no dump of all locations).
        assert!(
            location_suggest(state.clone(), "".to_string())
                .unwrap()
                .is_empty()
        );

        // Case-Insensitiv.
        let arc = location_suggest(state.clone(), "arc".to_string()).unwrap();
        assert_eq!(arc, vec!["ARC-L1 Wild Forest Station"]);
    }

    /// RF2 (t_71e2f03d): REAL-IPC-Regressiontest. Der F2-UI-Probe (smoke-stub)
    /// hat `get_route_locations` mit handgemachten Dummies beantwortet und damit
    /// die echte Tauri-Argument-Abbildung übersprungen. Der Live-App schickt
    /// Svelte aber `{ commodity_id }` (snake_case) — Tauri v2 erwartet für
    /// top-level Command-Args per Default camelCase (`commodityId`). Bei NUR
    /// dieser einen Mehrwort-Arg wurde sie nie getroffen → invoke rejected
    /// → Frontend-Catch → „keine Alternativorte", obwohl die Stammdaten da sind.
    /// Der Test dispatcht den registrierten Command über die ECHTE IPC-Schicht
    /// (get_ipc_response) mit exakt dem Frontend-Payload und erzwingt damit,
    /// dass jeder künftige Wahr-Snake-Arg freiwillig per rename_all gekoppelt
    /// sein muss.
    #[test]
    fn get_route_locations_ipc_accepts_snake_case_commodity_arg() {
        use tauri::Manager;
        use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder, mock_context, noop_assets};

        let app = mock_builder()
            .invoke_handler(tauri::generate_handler![super::get_route_locations])
            .manage(crate::trade::TradeState {
                provider: Arc::new(StubProvider),
                cache: Arc::new(Cache::open_in_memory().unwrap()),
                tracker: Arc::new(crate::trade::SyncTracker::default()),
                wiki_sync: None,
            write_gate: Arc::new(crate::trade::WriteGate::new()),
            })
            .build(tauri::test::mock_context(noop_assets()))
            .unwrap();

        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();

        // Exakt das Argument-Objekt, das die Svelte-Komponente (`+page.svelte`)
        // für `get_route_locations`, { commodity_id: cur.commodity_id } sendet.
        let body: tauri::ipc::InvokeBody = serde_json::json!({ "commodity_id": 33 }).into();
        let req = tauri::webview::InvokeRequest {
            cmd: "get_route_locations".into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "tauri://localhost".parse().unwrap(),
            body,
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        };

        let res = get_ipc_response(&webview, req);
        // Wenn Tauri das snake_case-Arg nicht abbildet, schlägt die IPC-Args-
        // Deserialisierung fehl → `res` ist ein serde_json-Err "missing field".
        let ok = res.expect("get_route_locations({commodity_id}) muss über echte IPC ok sein");
        let locs: provider::routes::RouteCommodityLocations = ok.deserialize().unwrap();
        assert_eq!(locs.commodity_id, 33);
        assert_eq!(locs.buys.len(), 1);
        assert_eq!(locs.buys[0].terminal, "HDMS-Anderson");
        assert_eq!(locs.sells[0].terminal, "Port Olisar");
    }
}
