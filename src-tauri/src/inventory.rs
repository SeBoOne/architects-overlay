//! Local, private inventory IPC (Karte t_1bab1fea, v0.3.0 K2).
//!
//! Four commands over the cache's `inventory` table — no provider, no
//! network, no sync: the data is the user's own stock, written only by this
//! app. Entries either reference a cached entity (commodity/item) or are a
//! free-text line; locations likewise (location entity or free-text depot).
//! Errors cross the IPC boundary as human-readable German strings (same
//! contract as the trade commands).

use cache::inventory::{InventoryEntry, NewInventoryEntry};
use tauri::State;

use crate::trade::TradeState;

/// All positions, in the overview's default order (location → name →
/// quality DESC).
#[tauri::command]
pub fn inventory_list(state: State<'_, TradeState>) -> Result<Vec<InventoryEntry>, String> {
    state
        .cache
        .inventory_list()
        .map_err(|e| format!("Inventar-Fehler: {e}"))
}

/// Distinct location names for the add-form datalist.
#[tauri::command]
pub fn inventory_locations(state: State<'_, TradeState>) -> Result<Vec<String>, String> {
    state
        .cache
        .inventory_locations()
        .map_err(|e| format!("Inventar-Fehler: {e}"))
}

/// Add one position, returns the stored row (with id + timestamps).
#[tauri::command]
pub fn inventory_add(
    state: State<'_, TradeState>,
    entry: NewInventoryEntry,
) -> Result<InventoryEntry, String> {
    let id = state
        .cache
        .inventory_add(entry)
        .map_err(|e| format!("Inventar-Fehler: {e}"))?;
    Ok(state
        .cache
        .inventory_list()
        .map_err(|e| format!("Inventar-Fehler: {e}"))?
        .into_iter()
        .find(|e| e.id == id)
        .unwrap_or_else(|| panic!("just-inserted inventory row {id} vanished")))
}

/// Overwrite one position (full-row edit).
#[tauri::command]
pub fn inventory_update(
    state: State<'_, TradeState>,
    id: u64,
    entry: NewInventoryEntry,
) -> Result<(), String> {
    state
        .cache
        .inventory_update(id, entry)
        .map_err(|e| format!("Inventar-Fehler: {e}"))
}

/// Delete one position.
#[tauri::command]
pub fn inventory_delete(state: State<'_, TradeState>, id: u64) -> Result<(), String> {
    state
        .cache
        .inventory_delete(id)
        .map_err(|e| format!("Inventar-Fehler: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cache::Cache;
    use provider::{DataProvider, EntityKind, SearchHit};
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
        ) -> provider::Result<Vec<SearchHit>> {
            Ok(vec![])
        }
        async fn prices(&self, _c: &str) -> provider::Result<Vec<provider::PricePoint>> {
            Ok(vec![])
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

    fn entry(name: &str, quality: u32, loc: &str) -> NewInventoryEntry {
        NewInventoryEntry {
            entity_kind: Some("commodity".into()),
            entity_id: Some(50),
            name: name.into(),
            quality,
            location_id: None,
            location_name: loc.into(),
            quantity: 2.0,
            price: None,
        }
    }

    #[test]
    fn add_update_delete_over_ipc_shapes() {
        let app = mock_app();
        app.manage(crate::trade::TradeState {
            provider: Arc::new(StubProvider),
            cache: Arc::new(Cache::open_in_memory().unwrap()),
            tracker: Arc::new(crate::trade::SyncTracker::default()),
            wiki_sync: None,
        });
        let state = app.state::<TradeState>();

        let added = inventory_add(state.clone(), entry("Gold", 900, "Heim-Hangar"))
            .expect("add must succeed");
        assert_eq!(added.name, "Gold");
        assert_eq!(added.quality, 900);

        inventory_update(state.clone(), added.id, entry("Gold", 364, "Depot"))
            .expect("update must succeed");
        let list = inventory_list(state.clone()).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].quality, 364);
        assert_eq!(list[0].location_name, "Depot");

        let locs = inventory_locations(state.clone()).unwrap();
        assert_eq!(locs, ["Depot"]);

        inventory_delete(state.clone(), added.id).unwrap();
        assert!(inventory_list(state.clone()).unwrap().is_empty());

        // Quality validation surfaces as a German IPC error.
        let err = inventory_add(state.clone(), entry("Gold", 1001, "x")).unwrap_err();
        assert!(err.contains("Qualität"), "got {err}");

        let _ = app;
    }
}
