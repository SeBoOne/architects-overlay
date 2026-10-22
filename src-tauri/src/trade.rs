//! Trade-data IPC layer: the commands the frontend palette calls.
//!
//! Three public commands plus one event:
//!
//! - `search(query, filter)` — cache-only FTS lookup, grouped into
//!   Commodities / Items / Locations. Never touches the network, so the
//!   overlay stays instant (< 50 ms is the goal; FTS5 prefix search is
//!   typically sub-ms). Freshness of the underlying reference data is the
//!   background sync task's job, not the keystroke path's.
//! - `get_prices(commodity, id?, kind?)` — price detail behind one expanded
//!   search row. `kind: "location"` (+ terminal id) returns the location
//!   aggregate grouped per terminal; `kind: "item"` (+ item id) returns an
//!   empty table (items are no commodities); otherwise per-commodity price
//!   points (provider policy: serve cache while ≤ 30 min old, otherwise
//!   fetch).
//! - `sync_status()` / `sync_now()` — reference-data freshness, entity
//!   counts, sync-in-progress and last error; `sync_now` triggers the
//!   on-demand sweep the UI needs for the first sync and the "Erneut
//!   versuchen" action.
//! - `get_marketplace(filters)` — live/cached marketplace offers with
//!   client-side region filter + price sort; `source` tells the UI whether the
//!   data came from the fresh short-TTL snapshot ("cache"), a live fetch
//!   ("live") or the last snapshot after a failed live fetch ("offline").
//! - `open_marketplace_listing(slug)` — opens the listing's uexcorp.space page
//!   in the system default browser (tauri-plugin-opener, external URL — no app
//!   navigation). Read-only, no write endpoints called anywhere in Phase 2.
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
    /// Guards the one-time start of the background TTL refresh loop.
    pub background: AtomicBool,
}

impl Default for SyncTracker {
    fn default() -> Self {
        Self {
            syncing: AtomicBool::new(false),
            last_error: Mutex::new(None),
            background: AtomicBool::new(false),
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
    /// Set (and `prices` flattened from it) when `kind: "location"` was
    /// requested: the location aggregate grouped by terminal. Null on the
    /// commodity/item path.
    pub location: Option<provider::LocationPrices>,
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

/// Marketplace response payloads (mirrored as TS types in +page.svelte).
#[derive(Clone, Debug, serde::Serialize)]
pub struct MarketResponse {
    /// "cache" = served from the fresh short-TTL snapshot · "live" = freshly
    /// fetched · "offline" = last snapshot after the live fetch failed.
    pub source: String,
    /// unix seconds the snapshot/response was assembled.
    pub fetched_at: u64,
    pub offers: Vec<provider::Offer>,
    /// Non-fatal note (e.g. last live fetch failed, showing stale snapshot).
    pub note: Option<String>,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Instant, cache-only search. `filters`: subset of ["commodity"|"item"|
/// "location"] — the palette's category chips AND `:c`/`:i`/`:l` prefix terms
/// (Arkanis-style). Empty = all categories.
#[tauri::command]
pub fn search(
    state: State<'_, TradeState>,
    query: String,
    filters: Option<Vec<String>>,
) -> Result<SearchResponse, String> {
    let query = query.trim();
    let filter_set = filters
        .unwrap_or_default()
        .into_iter()
        .collect::<std::collections::HashSet<String>>();
    // Empty query without category filters = nothing to show. With filters
    // (bare `:c`/`:i`/`:l`) the cache browse-mode lists the category instead.
    if query.is_empty() && filter_set.is_empty() {
        return Ok(SearchResponse {
            groups: Vec::new(),
            total: 0,
        });
    }

    let mut groups = Vec::new();
    let mut total = 0usize;
    for (kind, label, entity_kind) in GROUPS {
        if !filter_set.is_empty() && !filter_set.contains(*kind) {
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

/// Evaluate a math expression for the inline calculator (`=2*250+15%`).
/// Returns the computed value as a human-readable German string (never an
/// exception — malformed input becomes an error string the UI can show).
#[tauri::command]
pub fn math_eval(expr: String) -> Result<String, String> {
    let expr = expr.trim_start_matches('=').trim();
    match math_eval::evaluate(&expr) {
        Ok(v) if v.is_finite() => Ok(fmt_number(v)),
        Ok(_) => Err("Ergebnis ist nicht endlich (Division durch 0?)".to_string()),
        Err(e) => Err(e),
    }
}

/// German number formatting for the calculator result (no trailing zeros).
fn fmt_number(v: f64) -> String {
    let mut s = format!("{v}");
    // Collapse "0.30000000000000004"-style float noise to a clean short form.
    if s.contains('.') {
        let truncated = format!("{:.10}", v);
        let mut t = truncated.trim_end_matches('0');
        if t.ends_with('.') {
            t = &t[..t.len() - 1];
        }
        // Prefer the short form only when it round-trips (avoids 14 → 13.99999).
        s = t.to_string();
    }
    s
}

/// Price details behind one expanded search row. The frontend routes by the
/// hit's `kind`:
///
/// * `kind = "location"` (+ `id` = terminal id): returns the location
///   aggregate — every terminal of the location chain with its own buy/sell
///   table, in `location.groups` (feedback fix "Preise nicht geladen").
/// * `kind = "item"` (+ `id` = item id): items carry no commodity price
///   table unless they are commodities (those were deduped into the
///   commodity dataset at import). The detail comes back with an empty
///   `prices` list so the UI renders "nicht kaufbar / keine Preisdaten".
/// * default (or `kind = "commodity"`): per-commodity price points, cached
///   30 min, refetched live when stale. May also resolve to `Ok(empty)` for
///   a known item/terminal passed by name (defensive fallback).
#[tauri::command]
pub async fn get_prices(
    state: State<'_, TradeState>,
    commodity: String,
    id: Option<u64>,
    kind: Option<String>,
) -> Result<PriceDetail, String> {
    let fetched_at = now_unix();
    match kind.as_deref() {
        Some("location") => {
            let terminal_id =
                id.ok_or_else(|| "Location öffnen: fehlende Terminal-ID".to_string())?;
            let location = state
                .provider
                .location_prices(terminal_id)
                .await
                .map_err(friendly)?;
            let mut prices = Vec::new();
            for group in &location.groups {
                prices.extend(group.prices.iter().cloned());
            }
            Ok(PriceDetail {
                commodity: location.terminal.clone(),
                source: state.provider.name(),
                fetched_at,
                prices,
                location: Some(location),
            })
        }
        Some("item") => {
            // Items are no commodities: validate the id exists in the item
            // dataset and hand back an empty table (UI shows the
            // "nicht kaufbar" state instead of an error).
            let item_name = item_name(&state, id, &commodity).await?;
            Ok(PriceDetail {
                commodity: item_name,
                source: state.provider.name(),
                fetched_at,
                prices: Vec::new(),
                location: None,
            })
        }
        _ => {
            let prices = state.provider.prices(&commodity).await.map_err(friendly)?;
            Ok(PriceDetail {
                commodity: prices
                    .first()
                    .map(|p| p.commodity.clone())
                    .unwrap_or(commodity),
                source: state.provider.name(),
                fetched_at,
                prices,
                location: None,
            })
        }
    }
}

/// Resolve an item's display name (by id, falling back to the raw query).
async fn item_name(state: &TradeState, id: Option<u64>, raw: &str) -> Result<String, String> {
    let Some(item_id) = id else {
        return Ok(raw.to_string());
    };
    let entities = state
        .cache
        .entities_of(EntityKind::Items)
        .map_err(|e| format!("Cache-Fehler: {e}"))?;
    Ok(entities
        .iter()
        .find(|e| e.id == item_id)
        .map(|e| e.name.clone())
        .unwrap_or_else(|| raw.to_string()))
}

/// Snapshot of cache freshness / sync state for the footer status line.
#[tauri::command]
pub fn sync_status(state: State<'_, TradeState>) -> SyncStatus {
    build_status(&state.provider, &state.cache, &state.tracker)
}

/// Trigger the on-demand sync sweep (first sync, "Erneut versuchen"). Runs as an
/// async command so the sweep's tokio-based provider futures execute on Tauri's
/// async runtime; the [`SYNC_EVENT`] event announces completion so the UI can
/// drop its spinner.
#[tauri::command]
pub async fn sync_now(app: AppHandle, state: State<'_, TradeState>) -> Result<SyncStatus, String> {
    let provider = state.provider.clone();
    let cache = state.cache.clone();
    let tracker = state.tracker.clone();
    let handle = app.clone();

    // Only one sweep at a time; a concurrent call just re-reads the status.
    if !tracker.syncing.swap(true, Ordering::SeqCst) {
        let outcome: Result<(), String> = sweep(provider.as_ref(), cache.as_ref(), false).await;
        {
            let mut guard = tracker.last_error.lock().expect("tracker mutex poisoned");
            *guard = outcome.err();
        }
        tracker.syncing.store(false, Ordering::SeqCst);
        let _ = handle.emit(SYNC_EVENT, build_status(&provider, &cache, &tracker));
    }
    Ok(build_status(&state.provider, &state.cache, &state.tracker))
}

/// Marketplace offers, filtered client-side (q/location/region/sort) by the
/// provider. Served from the short-TTL cache when fresh; falls back to the last
/// snapshot (marked `source: "offline"`) when the live fetch fails but a
/// snapshot exists — the market view stays usable without a connection.
#[tauri::command]
pub async fn get_marketplace(
    state: State<'_, TradeState>,
    filters: provider::MarketplaceFilter,
) -> Result<MarketResponse, String> {
    // Was the snapshot already fresh before the call? Then the provider serves
    // it from cache (live only when item_id/operation filter forces a fetch).
    let cache_fresh = state.cache.marketplace_fresh().unwrap_or(false);
    match state.provider.marketplace_offers(filters).await {
        Ok(offers) => Ok(MarketResponse {
            source: if cache_fresh {
                "cache".to_string()
            } else {
                "live".to_string()
            },
            fetched_at: now_unix(),
            offers,
            note: None,
        }),
        Err(e) => {
            // Offline / rate-limited: serve the last snapshot if we have one,
            // else surface the error.
            match state.cache.marketplace_snapshot() {
                Ok(snapshot) if !snapshot.is_empty() => Ok(MarketResponse {
                    source: "offline".into(),
                    fetched_at: now_unix(),
                    offers: snapshot,
                    note: Some(friendly(e)),
                }),
                _ => Err(friendly(e)),
            }
        }
    }
}

/// Open one listing's page on uexcorp.space in the system default browser.
/// URL scheme verified against the live site (October 2026): both item and
/// service listings live under `/marketplace/item/info/{slug}/`. No slug → the
/// marketplace home. Pure external navigation; nothing is written to UEX.
#[tauri::command]
pub fn open_marketplace_listing(app: AppHandle, slug: String) -> Result<String, String> {
    let url = listing_deep_link(&slug);
    // Free-form opener API (no scope check — the URL comes from our own cache,
    // not from user/remote input). Opens in the system default browser.
    let _ = &app; // AppHandle kept for signature symmetry with sibling commands
    tauri_plugin_opener::open_url(&url, None::<&str>)
        .map_err(|e| format!("Browser nicht erreichbar: {e}"))?;
    Ok(url)
}

/// Build the external deep link for a listing slug.
fn listing_deep_link(slug: &str) -> String {
    let slug = slug.trim();
    if slug.is_empty() {
        "https://uexcorp.space/marketplace/".to_string()
    } else {
        format!("https://uexcorp.space/marketplace/item/info/{slug}/")
    }
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Start the background freshness/health loop, exactly once (guarded by
/// `tracker.background`). Safe from any thread: it spawns onto Tauri's own
/// async (Tokio) runtime via `tauri::async_runtime`, so the tokio-based
/// provider futures have a reactor even when the loop is booted from the
/// app-setup (main) thread — the setup thread itself has no Tokio context.
///
/// Booted once from `setup_trade` — **not** lazily from a command. That was the
/// B1 bug: with a warm cache and a dead network the loop never started, so the
/// `online` flag (driven by `tracker.last_error`) stayed optimistic green.
///
/// The first tick probes reachability almost immediately, independent of the
/// cache TTL, and every outcome is written into `tracker.last_error` and
/// broadcast via [`SYNC_EVENT`], so the frontend's `OFFLINE · CACHE …` banner
/// and `online` status update live.
pub fn start_background_refresh(
    app: &AppHandle,
    provider: &Arc<dyn DataProvider>,
    cache: &Arc<Cache>,
    tracker: &Arc<SyncTracker>,
) {
    if tracker.background.swap(true, Ordering::SeqCst) {
        return;
    }
    let provider = provider.clone();
    let cache = cache.clone();
    let tracker = tracker.clone();
    let handle = app.clone();
    // Marketplace-Sync (Phase 2): kurze TTL (5 min) → eigener, schneller Zyklus,
    // damit der Boards-Snapshot nahezu frisch bleibt. Rate-Limit-schonend: ein
    // /marketplace_listings-Request pro TTL-Fenster (≈12/h), plus der einmalige
    // Star-System-Fetch pro 12h. Fehler sind hier nicht fatal — der nächste Tick
    // versucht es erneut und die UI fällt auf den letzten Snapshot zurück.
    let mprovider = provider.clone();
    let mcache = cache.clone();
    _ = tauri::async_runtime::spawn(async move {
        // Erste Probe kurz nach Boot: Netz-Erreichbarkeit prüfen, unabhängig
        // von der TTL — ein frischer Cache mit totem Netz muss trotzdem als
        // offline erkannt werden.
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        background_tick(&tracker, &provider, &cache, &handle).await;
        loop {
            tokio::time::sleep(cache::sync::DEFAULT_INTERVAL).await;
            background_tick(&tracker, &provider, &cache, &handle).await;
        }
    });

    _ = tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(MARKET_REFRESH_INTERVAL).await;
            match (
                mcache.marketplace_fresh(),
                mprovider
                    .marketplace_offers(provider::MarketplaceFilter::default())
                    .await,
            ) {
                (Ok(true), _) => log::debug!("market: snapshot fresh, skip"),
                (_, Err(e)) => log::warn!("market: refresh failed: {e}"),
                (_, Ok(offers)) => log::info!("market: cached {} offers", offers.len()),
            }
        }
    });
}

/// Cadence of the background marketplace refresh (= the marketplace TTL, so we
/// never hammer the API: exactly one listing fetch per fresh window + failures
/// are retried on the next tick).
const MARKET_REFRESH_INTERVAL: std::time::Duration = match cache::TTL_MARKETPLACE {
    Some(d) => d,
    None => std::time::Duration::from_secs(300),
};

/// One background iteration: refresh stale datasets (computed via
/// [`sweep`] with `probe_if_fresh`), record the outcome and broadcast it.
async fn background_tick(
    tracker: &Arc<SyncTracker>,
    provider: &Arc<dyn DataProvider>,
    cache: &Arc<Cache>,
    handle: &AppHandle,
) {
    let outcome = sweep(provider.as_ref(), cache.as_ref(), true).await;
    {
        let mut guard = tracker.last_error.lock().expect("tracker mutex poisoned");
        *guard = outcome.err();
    }
    let _ = handle.emit(SYNC_EVENT, build_status(provider, cache, tracker));
}

/// Re-fetch every reference dataset older than its TTL. Stops at the first
/// failure and reports it (the sweep is all-or-nothing so the tracker stays
/// truthful about the "online" flag).
///
/// `probe_if_fresh`: when *nothing* is stale (warm cache), re-fetch the
/// commodities dataset once anyway as a real connectivity probe — the background
/// loop uses this so a blocked network is noticed even right after a clean
/// sync; the on-demand `sync_now` path passes `false` (it may not waste an
/// API quota on fresh data).
async fn sweep(
    provider: &dyn DataProvider,
    cache: &Cache,
    probe_if_fresh: bool,
) -> Result<(), String> {
    let mut fetched = false;
    for (_kind, label, entity_kind) in GROUPS {
        let fresh = cache
            .is_fresh(*entity_kind)
            .map_err(|e| format!("Cache-Fehler: {e}"))?;
        if fresh {
            continue;
        }
        fetched = true;
        log::info!("sync: fetching {label} (stale)");
        let entities = provider
            .fetch_entities(*entity_kind)
            .await
            .map_err(friendly)?;
        cache
            .store_entities(*entity_kind, &entities)
            .map_err(|e| format!("Cache-Fehler: {e}"))?;
        log::info!("sync: {label} stored ({} entries)", entities.len());
    }
    if probe_if_fresh && !fetched {
        // Warm cache → only a forced round-trip detects an outage. Stored, so
        // the probe is not pure waste (keeps commodities fresh).
        let entities = provider
            .fetch_entities(EntityKind::Commodities)
            .await
            .map_err(friendly)?;
        cache
            .store_entities(EntityKind::Commodities, &entities)
            .map_err(|e| format!("Cache-Fehler: {e}"))?;
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
    // Live snapshot of the worker's last error. The GUI thread (commands run on
    // the main thread under WebKitGTK) must NEVER block on the async worker's
    // mutex — a stuck sync must not freeze the overlay, so read with try_lock:
    // if the worker is mid-write, show no error and stay optimistic.
    let last_error: Option<String> = tracker
        .last_error
        .try_lock()
        .ok()
        .and_then(move |guard| guard.clone());
    let online = last_error.is_none();
    SyncStatus {
        provider: provider.name(),
        syncing: tracker.syncing.load(Ordering::SeqCst),
        last_error,
        online,
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
    use tauri::Manager;
    use tauri::test::mock_app;

    #[test]
    fn friendly_rate_limit_is_german_and_actionable() {
        assert_eq!(
            friendly(ProviderError::RateLimit {
                retry_after: Some(34)
            }),
            "HTTP 429 · Rate-Limit (Retry in 34 s)"
        );
        assert!(friendly(ProviderError::Timeout).contains("UEX"));
        assert!(
            friendly(ProviderError::Http {
                status: 500,
                body: "x".into()
            })
            .contains("500")
        );
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

    /// `search` accepts a set of category filters (from `:c`/`:i`/`:l` prefix
    /// terms) — an empty set means all categories come along.
    #[test]
    fn search_accepts_multiple_category_filters() {
        let app = mock_app();
        app.manage(TradeState {
            provider: Arc::new(StubProvider),
            cache: Arc::new(Cache::open_in_memory().unwrap()),
            tracker: Arc::new(SyncTracker::default()),
        });
        let state = app.state::<TradeState>();

        // None → every group is requested (all three present).
        let resp = search(state.clone(), "gold".into(), None).unwrap();
        assert_eq!(resp.groups.len(), 3, "no filter ⇒ all groups");

        // A single filter keeps one group.
        let resp = search(
            state.clone(),
            "gold".into(),
            Some(vec!["commodity".to_string()]),
        )
        .unwrap();
        assert_eq!(resp.groups.len(), 1);
        assert_eq!(resp.groups[0].kind, "commodity");

        // Two filters keep those two groups, in GROUPS order.
        let resp = search(
            state.clone(),
            "gold".into(),
            Some(vec!["item".to_string(), "location".to_string()]),
        )
        .unwrap();
        assert_eq!(resp.groups.len(), 2);
        assert_eq!(resp.groups[0].kind, "item");
        assert_eq!(resp.groups[1].kind, "location");

        let _ = app;
    }

    /// A bare category prefix (`:c` without free text) must reach the cache's
    /// browse path — the empty-query short-circuit only fires without filters.
    #[test]
    fn empty_query_with_category_filter_browses_not_shortcircuits() {
        let app = mock_app();
        app.manage(TradeState {
            provider: Arc::new(StubProvider),
            cache: Arc::new(Cache::open_in_memory().unwrap()),
            tracker: Arc::new(SyncTracker::default()),
        });
        let state = app.state::<TradeState>();

        // No filter + empty query → nothing (old short-circuit preserved).
        let resp = search(state.clone(), "".into(), None).unwrap();
        assert_eq!(resp.total, 0);
        assert!(resp.groups.is_empty());

        // Filter + empty query → the filtered group is served (browse mode;
        // empty in-memory cache yields 0 hits, but the group must exist).
        let resp = search(
            state.clone(),
            "".into(),
            Some(vec!["commodity".to_string()]),
        )
        .unwrap();
        assert_eq!(resp.groups.len(), 1);
        assert_eq!(resp.groups[0].kind, "commodity");
        assert_eq!(resp.total, 0);

        // Filter + empty query + another category → both groups, GROUPS order.
        let resp = search(
            state.clone(),
            "".into(),
            Some(vec!["item".to_string(), "location".to_string()]),
        )
        .unwrap();
        assert_eq!(resp.groups.len(), 2);
        assert_eq!(resp.groups[0].kind, "item");
        assert_eq!(resp.groups[1].kind, "location");

        let _ = app;
    }

    #[test]
    fn math_eval_inline_calculator() {
        // The UI strips the leading '=' before invoking; the command is
        // tolerant of it anyway.
        assert_eq!(math_eval("2*250+15%".into()).unwrap(), "500.15");
        assert_eq!(math_eval("=2*250+15%".into()).unwrap(), "500.15");
        assert_eq!(math_eval("(2+3)*4".into()).unwrap(), "20");
        assert_eq!(math_eval(" 10/4 ".into()).unwrap(), "2.5");
        // Malformed input → a German error string, never a panic.
        assert!(math_eval("2+3x".into()).is_err());
        assert!(math_eval("".into()).is_err());
        assert!(math_eval("1/0".into()).is_err()); // non-finite
    }

    #[test]
    fn listing_deep_link_builds_verified_url_scheme() {
        // URL scheme is the one verified against the live site (items AND
        // services live under /marketplace/item/info/{slug}/).
        assert_eq!(
            listing_deep_link("stark-compensator-2-UKCqAtSM3p"),
            "https://uexcorp.space/marketplace/item/info/stark-compensator-2-UKCqAtSM3p/"
        );
        // Whitespace-only / empty slug → marketplace home (no broken URL).
        assert_eq!(listing_deep_link(""), "https://uexcorp.space/marketplace/");
        assert_eq!(
            listing_deep_link("   "),
            "https://uexcorp.space/marketplace/"
        );
    }

    /// get_prices routes by `kind`: location → aggregate groups, item →
    /// empty table, default → commodity points. Verifies the backend contract
    /// the frontend mirrors in +page.svelte.
    #[tokio::test]
    async fn get_prices_routes_location_and_item_kinds() {
        use provider::{LocationPrices, TerminalPriceGroup};

        struct LocProvider;
        #[async_trait::async_trait]
        impl DataProvider for LocProvider {
            async fn search(
                &self,
                _e: provider::SearchEntities,
                _q: &str,
            ) -> provider::Result<Vec<SearchHit>> {
                Ok(vec![])
            }
            async fn prices(&self, _c: &str) -> provider::Result<Vec<provider::PricePoint>> {
                Ok(vec![provider::PricePoint {
                    commodity: "Agricium".into(),
                    commodity_id: 1,
                    terminal_id: 12,
                    location: "TDD Area 18".into(),
                    sell_price: 10_000.0,
                    buy_price: 0.0,
                    sell_stock: 301.0,
                    buy_stock: 0.0,
                    sell_status: 1,
                    buy_status: 0,
                    quality: 0,
                    updated: 1_792_341_485,
                }])
            }
            async fn location_prices(&self, _terminal_id: u64) -> provider::Result<LocationPrices> {
                Ok(LocationPrices {
                    location: "Pyro > Monox > Checkmate Station".into(),
                    terminal: "Commodity Shop - Admin - Checkmate".into(),
                    groups: vec![TerminalPriceGroup {
                        terminal_id: 436,
                        terminal_name: "Commodity Shop - Admin - Checkmate".into(),
                        prices: vec![provider::PricePoint {
                            commodity: "Agricultural Supplies".into(),
                            commodity_id: 3,
                            location: "Admin - Checkmate".into(),
                            terminal_id: 436,
                            sell_price: 0.0,
                            buy_price: 1108.0,
                            sell_stock: 0.0,
                            buy_stock: 0.0,
                            sell_status: 0,
                            buy_status: 7,
                            quality: 0,
                            updated: 1_792_355_842,
                        }],
                    }],
                })
            }
            async fn marketplace_offers(
                &self,
                _f: provider::MarketplaceFilter,
            ) -> provider::Result<Vec<provider::Offer>> {
                Ok(vec![])
            }
            async fn fetch_entities(
                &self,
                _k: EntityKind,
            ) -> provider::Result<Vec<provider::Entity>> {
                Ok(vec![])
            }
            fn name(&self) -> String {
                "loc".into()
            }
        }

        let app = mock_app();
        app.manage(TradeState {
            provider: Arc::new(LocProvider),
            cache: Arc::new(Cache::open_in_memory().unwrap()),
            tracker: Arc::new(SyncTracker::default()),
        });
        let state = app.state::<TradeState>();

        // location route
        let d = get_prices(
            state.clone(),
            "Checkmate".into(),
            Some(436),
            Some("location".into()),
        )
        .await
        .unwrap();
        let loc = d.location.expect("location route must fill `location`");
        assert_eq!(loc.groups.len(), 1);
        assert_eq!(loc.groups[0].prices[0].buy_price, 1108.0);
        assert_eq!(d.prices.len(), 1, "flattened prices mirror the groups");

        // item route → empty table, no location
        let d = get_prices(
            state.clone(),
            "CSP-68H Backpack".into(),
            Some(465),
            Some("item".into()),
        )
        .await
        .unwrap();
        assert!(d.prices.is_empty());
        assert!(d.location.is_none());

        // default route → commodity provider path
        let d = get_prices(state.clone(), "agricium".into(), None, None)
            .await
            .unwrap();
        assert_eq!(d.prices.len(), 1);
        assert_eq!(d.prices[0].sell_price, 10_000.0);
        assert!(d.location.is_none());

        let _ = app;
    }

    /// get_marketplace: no snapshot → provider error surfaces as a command
    /// error (UI shows the error pane); with snapshot → offline fallback.
    #[tokio::test]
    async fn get_marketplace_falls_back_to_snapshot_offline() {
        use provider::{ListingType, MarketOperation};

        struct FailingProvider;
        #[async_trait::async_trait]
        impl DataProvider for FailingProvider {
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
                Err(provider::ProviderError::Timeout)
            }
            async fn fetch_entities(
                &self,
                _k: EntityKind,
            ) -> provider::Result<Vec<provider::Entity>> {
                Ok(vec![])
            }
            fn name(&self) -> String {
                "failing".into()
            }
        }

        let app = mock_app();
        app.manage(TradeState {
            provider: Arc::new(FailingProvider),
            cache: Arc::new(Cache::open_in_memory().unwrap()),
            tracker: Arc::new(SyncTracker::default()),
        });
        let state = app.state::<TradeState>();

        // no snapshot → error surfaces
        let err = get_marketplace(state.clone(), provider::MarketplaceFilter::default())
            .await
            .unwrap_err();
        assert!(err.contains("Zeitüberschreitung"), "got {err}");

        // with a snapshot → offline fallback with note
        state
            .cache
            .store_marketplace(&[provider::Offer {
                id: 1,
                title: "x".into(),
                description: String::new(),
                item_id: 0,
                category_id: 0,
                operation: MarketOperation::Sell,
                listing_type: ListingType::Item,
                price: 1.0,
                currency: "UEC".into(),
                unit: "unit".into(),
                quantity: 1,
                location: "Levski".into(),
                region: "Stanton".into(),
                seller: "s".into(),
                quality: 0,
                updated: 1,
                expires: None,
                slug: "s".into(),
            }])
            .unwrap();
        let resp = get_marketplace(state.clone(), provider::MarketplaceFilter::default())
            .await
            .unwrap();
        assert_eq!(resp.source, "offline");
        assert_eq!(resp.offers.len(), 1);
        assert!(resp.note.is_some(), "offline fallback must carry the note");
        let _ = app;
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
