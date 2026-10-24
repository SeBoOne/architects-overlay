//! Handelsrouten (Karte t_128b7547): IPC-Commands für die Routen-Ansicht im
//! Marktmodus. Die eigentliche Beschaffung (UEX-Sweep) und Berechnung leben
//! im `uex_client`-/`provider`-Crate — hier sitzt nur die Tauri-Schicht.

use std::collections::HashMap;

use provider::routes::{RouteFilters, RouteList, TradeRoute};

use crate::trade::{TradeState, friendly};

/// Response der Routen-Abfrage.
#[derive(Clone, serde::Serialize)]
pub struct RoutesResponse {
    /// Quellen-Label des Providers (z. B. "wiki+uex (wiki)") — die Routen
    /// selbst kommen IMMER aus dem UEX-Sweep (bewusste Entscheidung).
    pub source: String,
    /// Unix-Sekunden des letzten Routen-Sweeps (Datenstand-Anzeige).
    pub swept_at: u64,
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
