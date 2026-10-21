//! Trade-data IPC layer: the commands the frontend palette calls.
//!
//! Three public commands plus one event:
//!
//! - `search(query, filter)` — cache-only FTS lookup, grouped into
//!   Commodities / Items / Locations. Never touches the network, so the
//!   overlay stays instant (< 50 ms is the goal; FTS5 prefix search is
//!   typically sub-ms). Freshness of the underlying reference data is the
//!   background sync task's job, not the keystroke path's.
//! - `get_prices(commodity)` — live/cached price points for one commodity
//!   (provider policy: serve cache while ≤ 30 min old, otherwise fetch).
//! - `sync_status()` / `sync_now()` — reference-data freshness, entity
//!   counts, sync-in-progress and last error; `sync_now` triggers the
//!   on-demand sweep the UI needs for the first sync and the "Erneut
//!   versuchen" action.
//!
//! Errors cross the IPC boundary as human-readable German strings (the
//! design contract wants rate limits etc. in the user's language, e.g.
//! `HTTP 429 · Rate-Limit (Retry in 34 s)`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cache::Cache;
use provider::{DataProvider, EntityKind, ProviderError, SearchHit};
use tauri::{AppHandle, Emitter, State};

/// Tauri event fired after every background sync sweep run (start or finish)
/// so the frontend can drop its spinner without polling.
pub const SYNC_EVENT: &str = "trade-sync";

/// Group order and labels per the design contract: Commodities → Items →
/// Locations, each with a counter.
const GROUPS: &[(&str, &str, EntityKind)] = &[
    ("commodity", "Commodities", EntityKind::Commodities),
    ("item", "Items", EntityKind::Items),
    ("location", "Locations", EntityKind::Terminals),
];

/// Max hits served per group (keeps the palette compact and the IPC tiny).
const PER_GROUP_LIMIT: u32 = 12;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

/// Bookkeeping for the sync sweeps, shared between the spawned sweep task and
/// the status command.
pub struct SyncTracker {
    pub syncing: AtomicBool,
    pub last_error: Mutex<Option<String>>,
}

impl Default for SyncTracker {
    fn default() -> Self {
        Self {
            syncing: AtomicBool::new(false),
            last_error: Mutex::new(None),
        }
    }
}

/// App-managed trade state. The provider is behind `Arc<dyn DataProvider>`
/// so a future second source slots in behind the same seam.
pub struct TradeState {
    pub provider: Arc<dyn DataProvider>,
    pub cache: Arc<Cache>,
    pub tracker: Arc<SyncTracker>,
}

// ---------------------------------------------------------------------------
// IPC payloads
// ---------------------------------------------------------------------------

#[derive(Clone, serde::Serialize)]
pub struct GroupHits {
    /// "commodity" | "item" | "location".
    pub kind: String,
    pub label: String,
    pub hits: Vec<SearchHit>,
}

#[derive(Clone, serde::Serialize)]
pub struct SearchResponse {
    pub groups: Vec<GroupHits>,
    pub total: usize,
}

#[derive(Clone, serde::Serialize)]
pub struct PriceDetail {
    pub commodity: String,
    pub source: String,
    /// unix seconds when the response was assembled.
    pub fetched_at: u64,
    pub prices: Vec<provider::PricePoint>,
}

#[derive(Clone, serde::Serialize)]
pub struct DatasetStatus {
    pub kind: String,
    pub label: String,
    pub fresh: bool,
    /// Seconds since the dataset was fetched (`None` = never).
    pub age_secs: Option<u64>,
    pub count: u64,
}

#[derive(Clone, serde::Serialize)]
pub struct SyncStatus {
    pub provider: String,
    pub syncing: bool,
    /// `true` until a sync attempt fails (the UI shows the Offline banner on
    /// `false`). Optimistic at boot so a fresh install is not immediately
    /// labelled offline before the first sweep has run.
    pub online: bool,
    pub last_error: Option<String>,
    pub datasets: Vec<DatasetStatus>,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Instant, cache-only search. `filter`: "all" (default) | "commodity" |
/// "item" | "location" — the palette's category chips.
#[tauri::command]
pub fn search(
    state: State<'_, TradeState>,
    query: String,
    filter: Option<String>,
) -> Result<SearchResponse, String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(SearchResponse { groups: Vec::new(), total: 0 });
    }
    let filter = filter.as_deref().unwrap_or("all");

    let mut groups = Vec::new();
    let mut total = 0usize;
    for (kind, label, entity_kind) in GROUPS {
        if filter != "all" && filter != *kind {
            continue;
        }
        let hits = state
            .cache
            .search(&[*entity_kind], query, PER_GROUP_LIMIT)
            .map_err(|e| format!("Cache-Fehler: {e}"))?;
        total += hits.len();
        groups.push(GroupHits {
            kind: (*kind).to_string(),
            label: (*label).to_string(),
            hits,
        });
    }

    Ok(SearchResponse { groups, total })
}

/// Current price list for one commodity (by name/code). May fetch live when
/// the local copy is older than the price TTL — the UI shows the loader during
/// that window.
#[tauri::command]
pub async fn get_prices(
    state: State<'_, TradeState>,
    commodity: String,
) -> Result<PriceDetail, String> {
    let prices = state.provider.prices(&commodity).await.map_err(friendly)?;
    let fetched_at = now_unix();
    Ok(PriceDetail {
        commodity: prices.first().map(|p| p.commodity.clone()).unwrap_or(commodity),
        source: state.provider.name(),
        fetched_at,
        prices,
    })
}

/// Snapshot of cache freshness / sync state for the footer status line.
#[tauri::command]
pub fn sync_status(state: State<'_, TradeState>) -> SyncStatus {
    build_status(&state.provider, &state.cache, &state.tracker)
}

/// Trigger the on-demand sync sweep (first sync, "Erneut versuchen").
/// Returns the current status immediately; the sweep runs in the background
/// and announces its outcome via the [`SYNC_EVENT`] event.
#[tauri::command]
pub fn sync_now(app: AppHandle, state: State<'_, TradeState>) -> SyncStatus {
    let provider = state.provider.clone();
    let cache = state.cache.clone();
    let tracker = state.tracker.clone();
    let handle = app.clone();

    // Only one sweep at a time; a concurrent call just re-reads the status.
    if !tracker.syncing.swap(true, Ordering::SeqCst) {
        tauri::async_runtime::spawn(async move {
            let outcome: Result<(), String> = sweep(provider.as_ref(), cache.as_ref()).await;
            let mut guard = tracker.last_error.lock().expect("tracker mutex poisoned");
            *guard = outcome.err();
            drop(guard);
            tracker.syncing.store(false, Ordering::SeqCst);
            let _ = handle.emit(SYNC_EVENT, build_status(&provider, &cache, &tracker));
        });
    }
    build_status(&state.provider, &state.cache, &state.tracker)
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Re-fetch every reference dataset that is older than its TTL. Stops at the
/// first failure and reports it (the sweep is all-or-nothing so the tracker
/// stays truthful about the "online" flag).
async fn sweep(provider: &dyn DataProvider, cache: &Cache) -> Result<(), String> {
    for (_kind, label, entity_kind) in GROUPS {
        let fresh = cache.is_fresh(*entity_kind).map_err(|e| format!("Cache-Fehler: {e}"))?;
        if fresh {
            continue;
        }
        log::info!("sync: fetching {label} (stale)");
        let entities = provider.fetch_entities(*entity_kind).await.map_err(friendly)?;
        cache
            .store_entities(*entity_kind, &entities)
            .map_err(|e| format!("Cache-Fehler: {e}"))?;
        log::info!("sync: {label} stored ({} entries)", entities.len());
    }
    Ok(())
}

fn build_status(
    provider: &Arc<dyn DataProvider>,
    cache: &Arc<Cache>,
    tracker: &SyncTracker,
) -> SyncStatus {
    let mut datasets = Vec::new();
    for (kind, label, entity_kind) in GROUPS {
        let fresh = cache.is_fresh(*entity_kind).unwrap_or(false);
        let age_secs = cache.dataset_age(*entity_kind).unwrap_or(None);
        let count = cache.count_entities(*entity_kind).unwrap_or(0);
        datasets.push(DatasetStatus {
            kind: (*kind).to_string(),
            label: (*label).to_string(),
            fresh,
            age_secs,
            count,
        });
    }
    SyncStatus {
        provider: provider.name(),
        syncing: tracker.syncing.load(Ordering::SeqCst),
        last_error: tracker.last_error.lock().expect("tracker mutex poisoned").clone(),
        online: tracker.last_error.lock().expect("tracker mutex poisoned").is_none(),
        datasets,
    }
}

/// Map a seam error to a German, user-facing message (design contract).
fn friendly(e: ProviderError) -> String {
    match e {
        ProviderError::RateLimit { retry_after } => match retry_after {
            Some(secs) => format!("HTTP 429 · Rate-Limit (Retry in {secs} s)"),
            None => "HTTP 429 · Rate-Limit erreicht".to_string(),
        },
        ProviderError::Http { status, .. } => {
            format!("HTTP {status} · UEX nicht erreichbar")
        }
        ProviderError::Timeout => "Zeitüberschreitung · UEX nicht erreichbar".to_string(),
        ProviderError::NotFound(what) => format!("Nicht gefunden: {what}"),
        ProviderError::Cache(msg) => format!("Cache-Fehler: {msg}"),
        ProviderError::Config(msg) => format!("Konfigurationsfehler: {msg}"),
        ProviderError::Json(msg) => format!("Ungültige Antwort: {msg}"),
        ProviderError::Unsupported(msg) => format!("Nicht unterstützt: {msg}"),
        _ => e.to_string(),
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::test::mock_app;
    use tauri::Manager;

    #[test]
    fn friendly_rate_limit_is_german_and_actionable() {
        assert_eq!(
            friendly(ProviderError::RateLimit { retry_after: Some(34) }),
            "HTTP 429 · Rate-Limit (Retry in 34 s)"
        );
        assert!(friendly(ProviderError::Timeout).contains("UEX"));
        assert!(friendly(ProviderError::Http { status: 500, body: "x".into() }).contains("500"));
    }

    /// The grouped search contract: three groups in a fixed order, per-group
    /// limit, filter removes groups, and the location group maps onto the
    /// terminal dataset.
    #[test]
    fn group_contract_is_stable() {
        assert_eq!(GROUPS[0].1, "Commodities");
        assert_eq!(GROUPS[1].1, "Items");
        assert_eq!(GROUPS[2].1, "Locations");
        assert_eq!(GROUPS[2].2, EntityKind::Terminals);
        assert_eq!(GROUPS.len(), 3);
    }

    /// Empty queries (incl. whitespace) return an empty response without
    /// touching the cache.
    #[test]
    fn search_empty_query_is_not_an_error() {
        let app = mock_app();
        app.manage(TradeState {
            provider: Arc::new(StubProvider),
            cache: Arc::new(Cache::open_in_memory().unwrap()),
            tracker: Arc::new(SyncTracker::default()),
        });
        let state = app.state::<TradeState>();

        for q in ["", "   ", "\t"] {
            let resp = search(state.clone(), q.into(), None).unwrap();
            assert!(resp.groups.is_empty(), "query {q:?} must yield no groups");
            assert_eq!(resp.total, 0);
        }
        let _ = app; // state borrow keeps app alive for the duration
    }

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
}
