//! # uex_client
//!
//! UEX is the *first* implementation of [`provider::DataProvider`]. It talks to
//! the UEX API v2.0 (endpoints per the research doc), caches reference data and
//! prices in the SQLite [`cache`], and maps everything into source-agnostic
//! seam DTOs — no UEX type ever leaks to the UI.
//!
//! Behavior contract:
//!
//! * **search** — local-cache only (instant); reference data is refreshed
//!   lazily if stale, or by the background sync task ([`cache::sync`]);
//! * **prices** — cached up to 30 min, then live fetch
//!   `/commodities_prices?id_commodity=…`;
//! * **marketplace** — the UEX API accepts only `id`/`slug`/`username`/
//!   `id_item`/`operation` here (no text search, no sort, no pagination) and
//!   caps each response (1,000 rows only when `id_item`+`operation` combine).
//!   The fetch plan therefore narrows server-side wherever possible (q terms
//!   that name a known item → `id_item` fetches; otherwise the `sell` and
//!   `buy` windows are merged) and ALWAYS applies q/region filter + sort
//!   locally via [`provider::apply_marketplace_filters`]. Unfiltered reads
//!   serve/refresh the short-TTL snapshot (5 min); the snapshot is replaced
//!   only by general fetches, so the cache always holds the general view;
//! * **entities** — full reference datasets: `/commodities`, `/terminals`,
//!   `/items` (per category, items require `id_category`), `/star_systems`
//!   (region names for marketplace listings).

mod dto;
mod http;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use cache::{Cache, CacheError, TTL_COMMODITIES, TTL_PRICES, TTL_TERMINALS};
use http::{DEFAULT_BASE_URL, DEFAULT_MIN_INTERVAL, UexHttpClient};
use provider::{
    DataProvider, Entity, EntityKind, ListingType, LocationPrices, MarketOperation,
    MarketplaceFilter, Offer, PricePoint, ProviderError, Result, SearchEntities, SearchHit,
    TerminalPriceGroup,
    routes::{
        RouteCommodityMeta, RouteFilters, RouteList, RoutePriceRow, RouteTerminalMeta, TradeRoute,
    },
};

use dto::{
    CategoryRow, CommodityRow, ItemPriceRow, ItemRow, ListingRow, PriceRow, StarSystemRow,
    TerminalRow, VehicleRow,
};

/// Search result page size served from the cache.
const SEARCH_LIMIT: u32 = 25;
// Marketplace item-id resolution: EVERY FTS hit of each query term is
// resolved to an `id_item` fetch. Each per-item fetch returns the COMPLETE
// listing set of that item (up to the 1,000-row cap), so widening the
// resolution is what makes the "≥15 matching offers" gate reachable on
// broad terms (e.g. "parallax" matched 10+ items; the old top-3 cap lost
// UA-4433 with 32 offers). The term-match limit only bounds the FTS page
// size per term — the item dataset is small, so 100 is effectively "all".
const MARKET_ITEM_TERM_MATCH_LIMIT: u32 = 100;

/// UEX `/terminals` `type` values that carry player-facing buy/sell data and
/// therefore belong in search + location aggregates (Feedback-Runde 4 / P2).
/// Everything else (fuel docks incl. Landing/Cargo Services, refinery
/// processing, ship shops, rentals) is filtered out AT IMPORT — no UI-side
/// hiding, the dataset itself stays clean. Generic over the type field, no
/// name blacklist: today `commodity` (trade consoles), `commodity_raw`
/// (Refinery Ore Sales — players SELL ore there) and `item` (shops with
/// /items_prices rows).
pub fn is_trade_terminal_type(t: Option<&str>) -> bool {
    matches!(t, Some("commodity") | Some("commodity_raw") | Some("item"))
}

/// Concrete provider for the UEX source.
pub struct UexProvider {
    http: UexHttpClient,
    cache: Arc<Cache>,
}

impl UexProvider {
    /// Construct with the shared cache and an optional API key (read endpoints
    /// work without auth; the key is stored locally via `settings`).
    pub fn new(cache: Arc<Cache>, api_key: Option<String>) -> Result<UexProvider> {
        UexProvider::with_base_url(cache, api_key, DEFAULT_BASE_URL, DEFAULT_MIN_INTERVAL)
    }

    /// Constructor with an explicit base URL + pacing override (tests, staging).
    pub fn with_base_url(
        cache: Arc<Cache>,
        api_key: Option<String>,
        base_url: &str,
        min_interval: std::time::Duration,
    ) -> Result<UexProvider> {
        let http = UexHttpClient::new(base_url, api_key, None, min_interval)?;
        Ok(UexProvider { http, cache })
    }

    /// Ensure the reference dataset for `kind` is usable in the local cache.
    /// Stale-First: fresh → nop; STALE BUT PRESENT → serve as-is (the
    /// background sync keeps it warm — the UI must NEVER wait for a refetch
    /// that could also fail offline); genuinely EMPTY → blocking import.
    async fn ensure_fresh(&self, kind: EntityKind) -> Result<()> {
        if self.cache.is_fresh(kind).map_err(map_cache_err)? {
            return Ok(());
        }
        if self.cache.has_entities(kind).map_err(map_cache_err)? {
            log::info!(
                "stale-first: serving stale {kind} reference data (background sync will refresh)"
            );
            return Ok(());
        }
        let entities = self.fetch_entities(kind).await?;
        self.cache
            .store_entities(kind, &entities)
            .map_err(map_cache_err)?;
        Ok(())
    }

    /// Resolve a commodity by name/code (case-insensitive cache lookup; exact
    /// name match wins over substring/rank matches).
    async fn resolve_commodity(&self, query: &str) -> Result<(u64, String)> {
        self.ensure_fresh(EntityKind::Commodities).await?;
        let hits = self
            .cache
            .search(&[EntityKind::Commodities], query, 10)
            .map_err(map_cache_err)?;
        let exact = hits
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(query.trim()))
            .or_else(|| {
                hits.iter()
                    .find(|h| h.code.as_deref() == Some(query.trim()))
            });
        let hit = exact.or(hits.first()).ok_or_else(|| {
            ProviderError::NotFound(format!("commodity '{query}' not found in reference data"))
        })?;
        Ok((hit.id, hit.name.clone()))
    }

    /// Map star-system id → region name from the cached reference dataset.
    /// Fetches once when stale (12h TTL) — one extra request per TTL window.
    async fn star_system_names(&self) -> Result<HashMap<u64, String>> {
        self.ensure_fresh(EntityKind::StarSystems).await?;
        Ok(self
            .cache
            .entities_of(EntityKind::StarSystems)
            .map_err(map_cache_err)?
            .into_iter()
            .map(|e| (e.id, e.name))
            .collect())
    }

    /// Client-side filter + sort, shared by the live and cache paths so both
    /// views behave identically. Delegates to
    /// [`provider::apply_marketplace_filters`] — the same helper also filters
    /// the offline-snapshot fallback in `trade::get_marketplace`, so no path
    /// can hand the UI an unfiltered/unsorted list.
    fn apply_local(offers: Vec<Offer>, filters: &MarketplaceFilter) -> Vec<Offer> {
        provider::apply_marketplace_filters(offers, filters)
    }

    /// Resolve the free-text market query terms to item ids from the local
    /// reference cache. When a term names an item, the API can filter
    /// server-side (`id_item`) — that path returns the COMPLETE set of
    /// listings for the item (up to the 1,000-row cap unlocked by
    /// `id_item`+`operation`), instead of the newest-window slice every
    /// unfiltered fetch is limited to. Terms that match no item are ignored
    /// here and stay in the local q filter afterwards.
    async fn resolve_market_item_ids(&self, q: &str) -> Result<Vec<u64>> {
        let terms: Vec<&str> = q.split_whitespace().collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        self.ensure_fresh(EntityKind::Items).await?;
        let mut ids: Vec<u64> = Vec::new();
        for term in terms {
            // ALL FTS hits of the term — no top-N cap. Each extra id is one
            // `id_item` fetch that returns that item's COMPLETE listing set;
            // dedupe below keeps the fetch count at one per distinct id.
            let hits = self
                .cache
                .search(&[EntityKind::Items], term, MARKET_ITEM_TERM_MATCH_LIMIT)
                .map_err(map_cache_err)?;
            for hit in hits {
                if !ids.contains(&hit.id) {
                    ids.push(hit.id);
                }
            }
        }
        Ok(ids)
    }
}

#[async_trait::async_trait]
impl DataProvider for UexProvider {
    async fn search(&self, entities: SearchEntities, query: &str) -> Result<Vec<SearchHit>> {
        let kinds = entities.kinds();
        for kind in kinds {
            // Locations are owned by the wiki source — the UEX provider never
            // fetches them (the composite provider ensures that dataset).
            if *kind != EntityKind::Locations {
                self.ensure_fresh(*kind).await?;
            }
        }
        // Only UEX-sourced rows: the wiki rows in the shared cache belong to
        // the wiki provider and must not leak through this seam
        // implementation.
        self.cache
            .search_sourced(kinds, &[cache::SOURCE_UEX], query, &[], SEARCH_LIMIT)
            .map_err(map_cache_err)
    }

    async fn prices(&self, commodity: &str) -> Result<Vec<PricePoint>> {
        let (id, name) = match self.resolve_commodity(commodity).await {
            Ok(found) => found,
            Err(ProviderError::NotFound(_)) => {
                // Not a trade commodity — check whether the query is a known
                // item (or terminal) instead. Such rows carry no commodity
                // price table; an empty result is the honest answer and lets
                // the UI render the "nicht kaufbar / keine Preisdaten" state
                // instead of a hard error.
                return Ok(self.empty_prices_for_other(commodity).await?);
            }
            Err(other) => return Err(other),
        };

        // Serve cached prices while fresh (≤ 30 min).
        if self.cache.prices_fresh(id).map_err(map_cache_err)? {
            let cached = self.cache.prices_for(id).map_err(map_cache_err)?;
            if !cached.is_empty() {
                return Ok(cached);
            }
        }

        let rows: Vec<PriceRow> = self
            .http
            .get(
                "/commodities_prices",
                &[("id_commodity".to_string(), id.to_string())],
            )
            .await?;
        let points = rows
            .into_iter()
            .map(|r| map_price_point(r, &name))
            .collect::<Vec<_>>();
        self.cache.store_prices(&points).map_err(map_cache_err)?;
        Ok(points)
    }

    async fn location_prices(&self, terminal_id: u64) -> Result<LocationPrices> {
        self.location_prices_impl(terminal_id).await
    }

    async fn terminal_prices(&self, terminal_id: u64) -> Result<Vec<PricePoint>> {
        self.terminal_prices_impl(terminal_id).await
    }

    async fn item_prices(&self, item_id: u64) -> Result<Vec<PricePoint>> {
        self.item_prices_impl(item_id).await
    }

    async fn marketplace_offers(&self, filters: MarketplaceFilter) -> Result<Vec<Offer>> {
        // ------------------------------------------------------------------
        // Fetch plan — the UEX API accepts only id/slug/username/id_item/
        // operation on this endpoint (no text search, no sort, no pagination)
        // and caps the response at a hard row limit (1,000 only when id_item
        // AND operation are combined). A single unfiltered call therefore only
        // ever sees the newest window of the market — q terms and price sort
        // over that slice looked "ignored" whenever the real matches lay
        // outside it (the entire WTS / all market, vs. the small WTB set).
        //
        // The plan narrows server-side wherever the API allows:
        // * explicit `item_id` → one `id_item` fetch (+ `operation`),
        // * q terms that name a known item → `id_item` fetches (complete per-
        //   item sets, `operation` unlocks the 1,000-row cap),
        // * otherwise → merge the `operation=sell` and `operation=buy`
        //   windows (double the coverage of the old single fetch), then
        //   filter + sort everything locally via `apply_marketplace_filters`.
        // ------------------------------------------------------------------
        let resolved_ids = if filters.item_id.is_some() {
            Vec::new()
        } else {
            self.resolve_market_item_ids(&filters.q).await?
        };
        // Item-scoped reads are filter-specific — never cached or used to
        // replace the general snapshot.
        let cacheable =
            filters.item_id.is_none() && resolved_ids.is_empty() && filters.operation.is_none();
        if cacheable && self.cache.marketplace_fresh().map_err(map_cache_err)? {
            let snapshot = self.cache.marketplace_snapshot().map_err(map_cache_err)?;
            return Ok(Self::apply_local(snapshot, &filters));
        }

        let plans = self.build_market_plans(&filters, &resolved_ids);

        // Region names: resolve id_star_system → name via the star-systems
        // dataset (lazily fetched once, then 12h TTL in the cache).
        let regions = self.star_system_names().await?;

        let mut offers: Vec<Offer> = Vec::new();
        let mut seen: std::collections::HashSet<u64> = std::collections::HashSet::new();
        for params in &plans {
            let rows: Vec<ListingRow> = self.http.get("/marketplace_listings", params).await?;
            for r in rows {
                if !seen.insert(r.id) {
                    continue;
                }
                let region = if r.id_star_system != 0 {
                    regions.get(&r.id_star_system).cloned().unwrap_or_default()
                } else {
                    String::new()
                };
                offers.push(Offer {
                    id: r.id,
                    title: r.title,
                    description: r.description,
                    item_id: r.id_item,
                    category_id: r.id_category,
                    operation: r.operation.parse().unwrap_or(MarketOperation::Sell),
                    listing_type: r.r#type.parse().unwrap_or(ListingType::Item),
                    price: r.price.parse::<f64>().unwrap_or(0.0),
                    currency: if r.currency.is_empty() {
                        "UEC".into()
                    } else {
                        r.currency
                    },
                    unit: r.unit,
                    quantity: r.in_stock,
                    location: r.location,
                    region,
                    seller: r.user_name,
                    quality: r.quality.unwrap_or(0),
                    updated: r.date_added,
                    expires: r.date_expiration,
                    slug: r.slug,
                });
            }
        }

        // Keep the snapshot for cache/offline display. Only the general
        // (unfiltered) fetch plan replaces it — a filter-specific fetch would
        // poison the cache.
        if cacheable {
            self.cache
                .store_marketplace(&offers)
                .map_err(map_cache_err)?;
        }
        Ok(Self::apply_local(offers, &filters))
    }

    async fn fetch_entities(&self, kind: EntityKind) -> Result<Vec<Entity>> {
        match kind {
            EntityKind::Commodities => {
                let rows: Vec<CommodityRow> = self.http.get("/commodities", &[]).await?;
                Ok(rows
                    .into_iter()
                    .map(|r| Entity {
                        id: r.id,
                        kind: EntityKind::Commodities,
                        name: r.name,
                        code: r.code,
                        subtitle: r.kind.unwrap_or_default(),
                    })
                    .collect())
            }
            EntityKind::Terminals => {
                let rows: Vec<TerminalRow> = self.http.get("/terminals", &[]).await?;
                Ok(rows
                    .into_iter()
                    // P2 (Feedback-Runde 4): only terminals whose UEX type
                    // carries buy/sell data enter the cache. Service
                    // terminals (fuel/landing, refinery, rentals, ship
                    // shops) are dropped at import — the dataset itself is
                    // clean, no UI-side filtering.
                    .filter(|r| is_trade_terminal_type(r.r#type.as_deref()))
                    .map(|r| {
                        let mut parts = Vec::new();
                        for field in [
                            r.star_system_name,
                            r.planet_name,
                            r.moon_name,
                            r.orbit_name,
                            r.space_station_name,
                            r.city_name,
                            r.outpost_name,
                            r.poi_name,
                        ]
                        .into_iter()
                        .flatten()
                        {
                            if !field.is_empty()
                                && parts.last().map(String::as_str) != Some(field.as_str())
                            {
                                parts.push(field);
                            }
                        }
                        Entity {
                            id: r.id,
                            kind: EntityKind::Terminals,
                            // Live feedback: search results must show the
                            // complete terminal name so the user recognizes
                            // the stand ("Commodity Shop - Admin - Checkmate")
                            // without guessing IDs — fullname is the most
                            // complete field, then name, then the bare
                            // displayname. The hierarchy stays in `subtitle`.
                            name: r.fullname.or(r.name).or(r.displayname).unwrap_or_default(),
                            code: r.code,
                            subtitle: parts.join(" > "),
                        }
                    })
                    .filter(|e| !e.name.chars().all(|c| c.is_ascii_digit()) && !e.name.is_empty())
                    .collect())
            }
            EntityKind::Items => {
                // /items has no bulk list — enumerate categories (research doc).
                let categories: Vec<CategoryRow> = self.http.get("/categories", &[]).await?;
                let mut seen = std::collections::HashSet::new();
                let mut entities = Vec::new();
                for category in categories {
                    let rows: Option<Vec<ItemRow>> = self
                        .http
                        .get_optional(
                            "/items",
                            &[("id_category".to_string(), category.id.to_string())],
                        )
                        .await?;
                    for r in rows.unwrap_or_default() {
                        if !seen.insert(r.id) {
                            continue;
                        }
                        // Dedup (live feedback): commodities also surface in
                        // /items with is_commodity=1 (empty url_store/uuid).
                        // They are represented by the commodities dataset —
                        // importing them as items would duplicate every trade
                        // good in search. Filter at the source, never in the
                        // UI (rule of this task).
                        if r.is_commodity == 1 {
                            continue;
                        }
                        let mut subtitle = Vec::new();
                        if let Some(section) = r.section.filter(|s| !s.is_empty()) {
                            subtitle.push(section);
                        }
                        if let Some(company) = r.company_name.filter(|s| !s.is_empty()) {
                            subtitle.push(company);
                        }
                        entities.push(Entity {
                            id: r.id,
                            kind: EntityKind::Items,
                            name: r.name,
                            code: r.slug,
                            subtitle: subtitle.join(" · "),
                        });
                    }
                }
                Ok(entities)
            }
            EntityKind::StarSystems => {
                let rows: Vec<StarSystemRow> = self.http.get("/star_systems", &[]).await?;
                Ok(rows
                    .into_iter()
                    .map(|r| Entity {
                        id: r.id,
                        kind: EntityKind::StarSystems,
                        name: r.name,
                        code: r.code,
                        subtitle: String::new(),
                    })
                    .collect())
            }
            // Locations are owned by the wiki source (rich hierarchy, stored
            // under source=wiki). The UEX provider has no location dataset —
            // its terminal rows carry the trade terminals. An empty result
            // keeps the seam exhaustive without a fabricated fetch.
            EntityKind::Locations => Ok(Vec::new()),
        }
    }

    async fn trade_routes(&self, filters: &RouteFilters) -> Result<RouteList> {
        self.trade_routes_impl(filters).await
    }

    async fn vehicles_scu(&self) -> Result<HashMap<String, f64>> {
        self.vehicles_scu_impl().await
    }

    fn name(&self) -> String {
        String::from("uex")
    }
}

// ---------------------------------------------------------------------------
// Handelsrouten (Karte t_128b7547)
// ---------------------------------------------------------------------------

/// Fact-Check (im Task dokumentiert): UEX bietet KEINEN Routen-Endpoint —
/// `/commodities_routes` & Co. antworten 404, `/commodities_prices` verlangt
/// zwingend `id_commodity` ODER `id_terminal`. Die Routen entstehen daher
/// LOKAL: ein Sweep über alle Commodity-Terminals füllt den
/// `terminal_prices`-Cache, die Berechnung (bester Einkauf → bester Verkauf
/// je Ware) läuft in `provider::routes::compute_routes_full`.
///
/// Schiffs-SCU: UEX `/vehicles` trägt `scu` (Cargoraum) — der Hangar-Filter
/// löst Schiffsname → SCU darüber (siehe `vehicles_scu`); das ersetzt das im
/// Task erwogene statische Paket und bleibt komplett UEX-Daten.
///
/// Entfernung: UEX hat keine Koordinaten — Distanz = Hierarchie-Proxy
/// (siehe `provider::routes`-Doku).
const ROUTES_SWEEP_TTL: Duration = TTL_PRICES; // 30 min
/// Terminal-Flags ändern sich selten — Referenz-Daten-TTL.
const ROUTES_TERMINAL_FLAGS_TTL: Duration = TTL_TERMINALS; // 12 h
const ROUTES_COMMODITY_FLAGS_TTL: Duration = TTL_COMMODITIES; // 1 h
const ROUTE_META_TERMINALS: &str = "route_meta:terminals";
const ROUTE_META_COMMODITIES: &str = "route_meta:commodities";
const ROUTE_META_SWEEP: &str = "route_meta:sweep";

impl UexProvider {
    fn route_meta_fresh(&self, key: &str, ttl: Duration) -> Result<bool> {
        match self.cache.route_meta_get(key).map_err(map_cache_err)? {
            Some((ts, _)) => Ok(now_unix_secs().saturating_sub(ts) < ttl.as_secs()),
            None => Ok(false),
        }
    }

    /// Is a route_meta payload PRESENT at all (any age)? Stale-but-present
    /// data is SERVED (stale-first); only a missing payload forces the
    /// blocking fetch. TTL-based refresh happens in the background
    /// ([`revalidate_route_data]).
    fn route_meta_present(&self, key: &str) -> Result<bool> {
        match self.cache.route_meta_get(key).map_err(map_cache_err)? {
            Some((_, payload)) => Ok(!payload.is_empty()),
            None => Ok(false),
        }
    }

    async fn fetch_route_terminal_flags(&self) -> Result<()> {
        // /terminals liefert ALLE Terminals (auch Nicht-Handel); die
        // Flags werden nur für Trade-Terminals gehalten.
        let rows: Vec<TerminalRow> = self.http.get("/terminals", &[]).await?;
        let flags: HashMap<u64, RouteTerminalMeta> = rows
            .iter()
            .filter(|r| is_trade_terminal_type(r.r#type.as_deref()))
            .map(|r| {
                let ground = [r.city_name.as_deref(), r.outpost_name.as_deref(), r.poi_name.as_deref()]
                    .into_iter().flatten().any(|s| !s.is_empty())
                    // Weder Boden- noch Orbit-Kontext (z. B. POI auf Mond
                    // ohne Ortsnamen) zählt als Boden — der Normalfall.
                    || ([r.space_station_name.as_deref(), r.orbit_name.as_deref()]
                        .into_iter().flatten().all(|s| s.is_empty()));
                let mut chain_parts = Vec::new();
                for field in [
                    r.star_system_name.clone(),
                    r.planet_name.clone(),
                    r.moon_name.clone(),
                    r.orbit_name.clone(),
                    r.space_station_name.clone(),
                    r.city_name.clone(),
                    r.outpost_name.clone(),
                    r.poi_name.clone(),
                ]
                .into_iter()
                .flatten()
                {
                    if !field.is_empty()
                        && chain_parts.last().map(String::as_str) != Some(field.as_str())
                    {
                        chain_parts.push(field);
                    }
                }
                (
                    r.id,
                    RouteTerminalMeta {
                        autoload: r.is_auto_load != 0,
                        cargo_center: r.is_cargo_center != 0,
                        ground,
                        chain: chain_parts.join(" > "),
                    },
                )
            })
            .collect();
        let payload =
            serde_json::to_string(&flags).map_err(|e| ProviderError::Json(e.to_string()))?;
        self.cache
            .route_meta_set(ROUTE_META_TERMINALS, now_unix_secs(), &payload)
            .map_err(map_cache_err)?;
        Ok(())
    }

    /// TTL-Expired → refresh, otherwise nop. Background-only (stale-first
    /// serving never waits for this).
    async fn refresh_route_terminal_flags(&self) -> Result<()> {
        if !self.route_meta_fresh(ROUTE_META_TERMINALS, ROUTES_TERMINAL_FLAGS_TTL)? {
            self.fetch_route_terminal_flags().await?;
        }
        Ok(())
    }

    async fn ensure_route_terminal_flags(&self) -> Result<HashMap<u64, RouteTerminalMeta>> {
        // Stale-First: old payload serves; the background revalidation
        // refreshes it on its TTL.
        if !self.route_meta_present(ROUTE_META_TERMINALS)? {
            self.fetch_route_terminal_flags().await?;
        }
        self.load_route_terminal_flags()
    }

    fn load_route_terminal_flags(&self) -> Result<HashMap<u64, RouteTerminalMeta>> {
        match self
            .cache
            .route_meta_get(ROUTE_META_TERMINALS)
            .map_err(map_cache_err)?
        {
            Some((_, json)) => serde_json::from_str(&json)
                .map_err(|e| ProviderError::Json(format!("route terminal flags: {e}"))),
            None => Ok(HashMap::new()),
        }
    }

    async fn fetch_route_commodity_flags(&self) -> Result<()> {
        let rows: Vec<CommodityRow> = self.http.get("/commodities", &[]).await?;
        let flags: HashMap<u64, RouteCommodityMeta> = rows
            .iter()
            .map(|r| {
                (
                    r.id,
                    RouteCommodityMeta {
                        illegal: r.is_illegal != 0,
                        sellable: r.is_sellable != 0,
                        buyable: r.is_buyable != 0,
                    },
                )
            })
            .collect();
        let payload =
            serde_json::to_string(&flags).map_err(|e| ProviderError::Json(e.to_string()))?;
        self.cache
            .route_meta_set(ROUTE_META_COMMODITIES, now_unix_secs(), &payload)
            .map_err(map_cache_err)?;
        Ok(())
    }

    async fn refresh_route_commodity_flags(&self) -> Result<()> {
        if !self.route_meta_fresh(ROUTE_META_COMMODITIES, ROUTES_COMMODITY_FLAGS_TTL)? {
            self.fetch_route_commodity_flags().await?;
        }
        Ok(())
    }

    async fn ensure_route_commodity_flags(&self) -> Result<HashMap<u64, RouteCommodityMeta>> {
        // Stale-First: old payload serves; the background revalidation
        // refreshes it on its TTL.
        if !self.route_meta_present(ROUTE_META_COMMODITIES)? {
            self.fetch_route_commodity_flags().await?;
        }
        match self
            .cache
            .route_meta_get(ROUTE_META_COMMODITIES)
            .map_err(map_cache_err)?
        {
            Some((_, json)) => serde_json::from_str(&json)
                .map_err(|e| ProviderError::Json(format!("route commodity flags: {e}"))),
            None => Ok(HashMap::new()),
        }
    }

    /// Ein Sweep-Durchlauf: jedes Trade-Terminal, dessen Preis-Tabelle nicht
    /// mehr frisch ist, wird live nachgezogen (`/commodities_prices?
    /// id_terminal=…` in den `terminal_prices`-Cache). Frische Terminals
    /// kosten keinen Request. Gibt die Anzahl NACHGEZOGENER Terminals zurück.
    async fn sweep_route_terminals(
        &self,
        flags: &HashMap<u64, RouteTerminalMeta>,
    ) -> Result<usize> {
        let mut refetched = 0usize;
        let mut failures = 0usize;
        // Deterministische Reihenfolge (kleinste id zuerst) — stabile Logs.
        let mut ids: Vec<u64> = flags.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let fresh = self
                .cache
                .terminal_prices_fresh(id)
                .map_err(map_cache_err)?;
            if fresh {
                continue;
            }
            match self.terminal_prices_impl(id).await {
                Ok(_) => refetched += 1,
                Err(ProviderError::NotFound(_)) => refetched += 1, // leeres Terminal
                Err(e) => {
                    failures += 1;
                    log::warn!("routes sweep: terminal {id} failed: {e}");
                }
            }
        }
        if refetched == 0 && failures > 0 {
            return Err(ProviderError::Http {
                status: 0,
                body: format!("routes sweep failed for all {failures} stale terminals"),
            });
        }
        self.cache
            .route_meta_set(ROUTE_META_SWEEP, now_unix_secs(), "1")
            .map_err(map_cache_err)?;
        Ok(refetched)
    }

    // ---- Stale-First serving + precomputated Profit-Matrix (K1) ----

    /// Dataset EPOCH of the price data: terminal-price (count, max
    /// fetched_at) + the two flag timestamps. The matrix artifact is ONLY
    /// invalidated when this moves — i.e. exactly when new price data
    /// landed, never on TTL expiry alone.
    fn matrix_epoch(&self) -> Result<String> {
        let (count, max_fetched) = self.cache.terminal_price_stats().map_err(map_cache_err)?;
        let tf = match self
            .cache
            .route_meta_fetched_at(ROUTE_META_TERMINALS)
            .map_err(map_cache_err)?
        {
            Some(ts) => ts,
            None => 0,
        };
        let cf = match self
            .cache
            .route_meta_fetched_at(ROUTE_META_COMMODITIES)
            .map_err(map_cache_err)?
        {
            Some(ts) => ts,
            None => 0,
        };
        Ok(format!("tp:{count}:{max_fetched};tf:{tf};cf:{cf}"))
    }

    /// Is the Sweep marker older than its TTL (or absent)? → serving is
    /// (possibly) stale and a background revalidation is worthwhile.
    pub fn route_sweep_stale(&self) -> Result<bool> {
        Ok(
            match self
                .cache
                .route_meta_fetched_at(ROUTE_META_SWEEP)
                .map_err(map_cache_err)?
            {
                Some(ts) => now_unix_secs().saturating_sub(ts) >= ROUTES_SWEEP_TTL.as_secs(),
                None => true,
            },
        )
    }

    /// Fast-Lane inputs available = the serving lanes answer WITHOUT the
    /// network: terminal-price rows + terminal flag payload + terminal
    /// names (entities). The Composite uses this to decide whether a stale
    //  marker is worth scheduling a background Sweep.
    pub fn route_inputs_present(&self) -> Result<bool> {
        let (rows_count, _) = self.cache.terminal_price_stats().map_err(map_cache_err)?;
        let flags_present = self.route_meta_present(ROUTE_META_TERMINALS)?;
        let names_present = self
            .cache
            .has_entities(EntityKind::Terminals)
            .map_err(map_cache_err)?;
        Ok(rows_count > 0 && flags_present && names_present)
    }

    fn sweep_ts(&self) -> Result<u64> {
        match self
            .cache
            .route_meta_get(ROUTE_META_SWEEP)
            .map_err(map_cache_err)?
        {
            Some((ts, _)) => Ok(ts),
            None => Ok(self
                .cache
                .route_matrix_built_at()
                .map_err(map_cache_err)?
                .unwrap_or(0)),
        }
    }

    /// (rows of terminal_prices as RoutePriceRow-Vector)
    fn collect_route_rows(&self) -> Result<Vec<RoutePriceRow>> {
        let mut rows: Vec<RoutePriceRow> = Vec::new();
        for (_terminal_id, points) in self
            .cache
            .terminal_price_rows_all()
            .map_err(map_cache_err)?
        {
            for p in points {
                rows.push(RoutePriceRow {
                    commodity_id: p.commodity_id,
                    commodity: p.commodity,
                    terminal_id: p.terminal_id,
                    buy_price: p.buy_price,
                    sell_price: p.sell_price,
                    sell_stock: p.sell_stock,
                    buy_status: p.buy_status,
                    sell_status: p.sell_status,
                    quality: p.quality,
                    updated: p.updated,
                });
            }
        }
        Ok(rows)
    }

    fn terminal_names_map(&self) -> Result<HashMap<u64, String>> {
        Ok(self
            .cache
            .entities_of(EntityKind::Terminals)
            .map_err(map_cache_err)?
            .into_iter()
            .map(|e| (e.id, e.name))
            .collect())
    }

    /// Persist the artifact + its dataset epoch (invalidate-when-new-data).
    fn persist_route_matrix(&self, matrix: &[TradeRoute]) -> Result<()> {
        self.cache
            .store_route_matrix(matrix)
            .map_err(map_cache_err)?;
        let epoch = self.matrix_epoch()?;
        self.cache
            .route_matrix_epoch_set(epoch, now_unix_secs())
            .map_err(map_cache_err)?;
        Ok(())
    }

    async fn trade_routes_impl(&self, filters: &RouteFilters) -> Result<RouteList> {
        // ---- Stale-First fast lanes: serve what the CACHE has — the UI
        // opens immediately, the refresh happens in the background
        // (CompositeProvider schedules revalidate_route_data). A TTL expiry
        // must NEVER trigger a blocking re-sweep in the foreground again. ----
        let (rows_count, _) = self.cache.terminal_price_stats().map_err(map_cache_err)?;
        let flags_present = self.route_meta_present(ROUTE_META_TERMINALS)?;
        let names_present = self
            .cache
            .has_entities(EntityKind::Terminals)
            .map_err(map_cache_err)?;
        let inputs_present = rows_count > 0 && flags_present && names_present;
        let sweeping = self.route_sweep_stale()?;

        if inputs_present {
            // (a) No terminal-SIDE filter: the persisted Profit-Matrix answers
            // directly — precomputated artifact, epoch-guarded.
            if !filters.has_terminal_side_filters() {
                let cur_epoch = self.matrix_epoch()?;
                let stored = self.cache.route_matrix_epoch_get().map_err(map_cache_err)?;
                if stored.as_deref() == Some(&cur_epoch) {
                    let matrix = self.cache.route_matrix_rows().map_err(map_cache_err)?;
                    let swept_at = self.sweep_ts()?;
                    return Ok(RouteList {
                        routes: provider::routes::routes_from_matrix(&matrix, filters),
                        swept_at,
                        refreshing: sweeping,
                    });
                }
                // Epoch moved (new price data since the last artifact) →
                // recompute the MATRIX once from the cached rows (NO network),
                // persist it, then serve.
                let terminal_flags = self.load_route_terminal_flags()?;
                let commodity_flags = self.ensure_route_commodity_flags().await?; // blocking only when the payload is missing
                let terminal_names = self.terminal_names_map()?;
                let rows = self.collect_route_rows()?;
                let matrix = provider::routes::compute_routes_full(
                    &rows,
                    &terminal_flags,
                    &terminal_names,
                    &commodity_flags,
                    &RouteFilters::default(),
                );
                self.persist_route_matrix(&matrix)?;
                let swept_at = self.sweep_ts()?;
                return Ok(RouteList {
                    routes: provider::routes::routes_from_matrix(&matrix, filters),
                    swept_at,
                    refreshing: sweeping,
                });
            }
            // (b) Terminal-SIDE filters: re-scan the CURRENT (possibly stale)
            // rows — the best Buy→Sell pair changes with the filter, the
            // epoch-guarded matrix cannot answer. Still NO sweep, NO API.
            let terminal_flags = self.load_route_terminal_flags()?;
            let commodity_flags = self.ensure_route_commodity_flags().await?;
            let terminal_names = self.terminal_names_map()?;
            let rows = self.collect_route_rows()?;
            let routes = provider::routes::compute_routes_full(
                &rows,
                &terminal_flags,
                &terminal_names,
                &commodity_flags,
                filters,
            );
            let swept_at = self.sweep_ts()?;
            return Ok(RouteList {
                routes,
                swept_at,
                refreshing: sweeping,
            });
        }

        // ---- Full path: cold/empty cache (nothing to serve) — blocking
        // import of the reference data + one Sweep, then persist the matrix. ----
        self.ensure_fresh(EntityKind::Commodities).await?;
        self.ensure_fresh(EntityKind::Terminals).await?;
        let terminal_flags = self.ensure_route_terminal_flags().await?;
        let commodity_flags = self.ensure_route_commodity_flags().await?;

        // Sweep: bei Frische kein Netzwerk; sonst nur nachziehen, was stale
        // ist.
        let swept_at = match self
            .cache
            .route_meta_get(ROUTE_META_SWEEP)
            .map_err(map_cache_err)?
        {
            Some((ts, _)) if now_unix_secs().saturating_sub(ts) < ROUTES_SWEEP_TTL.as_secs() => ts,
            _ => {
                self.sweep_route_terminals(&terminal_flags).await?;
                now_unix_secs()
            }
        };

        let terminal_names = self.terminal_names_map()?;
        let rows = self.collect_route_rows()?;
        // Persist the precomputated artifact (unfiltered base — serves later
        // default queries without any row-scan; epoch guards invalidation).
        // Lane (a): compute ONCE (default filters), persist, query locally.
        let routes: Vec<TradeRoute> = match filters.has_terminal_side_filters() {
            false => {
                let matrix = provider::routes::compute_routes_full(
                    &rows,
                    &terminal_flags,
                    &terminal_names,
                    &commodity_flags,
                    &RouteFilters::default(),
                );
                self.persist_route_matrix(&matrix)?;
                provider::routes::routes_from_matrix(&matrix, filters)
            }
            true => provider::routes::compute_routes_full(
                &rows,
                &terminal_flags,
                &terminal_names,
                &commodity_flags,
                filters,
            ),
        };
        Ok(RouteList {
            routes,
            swept_at,
            refreshing: false,
        })
    }

    /// Background revalidation (stale-while-revalidate): refresh expired
    /// reference flags, sweep every terminal whose price table expired,
    /// rebuild the Profit-Matrix artifact. NEVER the foreground — the
    /// CompositeProvider schedules this on a live view open. Rate-limit
    /// unchanged: at most one Sweep per TTL window (marker-guarded).
    pub async fn revalidate_route_data(&self) -> Result<usize> {
        self.refresh_route_terminal_flags().await?;
        self.refresh_route_commodity_flags().await?;
        let terminal_flags = self.load_route_terminal_flags()?;
        let refetched = self.sweep_route_terminals(&terminal_flags).await?;
        let commodity_flags = self.load_route_commodity_flags()?;
        let terminal_names = self.terminal_names_map()?;
        let rows = self.collect_route_rows()?;
        let matrix = provider::routes::compute_routes_full(
            &rows,
            &terminal_flags,
            &terminal_names,
            &commodity_flags,
            &RouteFilters::default(),
        );
        self.persist_route_matrix(&matrix)?;
        Ok(refetched)
    }

    fn load_route_commodity_flags(&self) -> Result<HashMap<u64, RouteCommodityMeta>> {
        match self
            .cache
            .route_meta_get(ROUTE_META_COMMODITIES)
            .map_err(map_cache_err)?
        {
            Some((_, json)) => serde_json::from_str(&json)
                .map_err(|e| ProviderError::Json(format!("route commodity flags: {e}"))),
            None => Ok(HashMap::new()),
        }
    }

    /// Schiffs-SCU-Kapazitäten aus UEX `/vehicles` (Case-insensitive
    /// Namens- oder Slug-Match). Für den Hangar-Filter im Frontend.
    async fn vehicles_scu_impl(&self) -> Result<HashMap<String, f64>> {
        let rows: Vec<VehicleRow> = self.http.get("/vehicles", &[]).await?;
        let mut out = HashMap::new();
        for r in rows {
            let scu = r.scu.max(0.0);
            if let Some(name) = r.name.as_deref().filter(|s| !s.is_empty()) {
                out.insert(name.to_lowercase(), scu);
            }
            if let Some(full) = r.name_full.as_deref().filter(|s| !s.is_empty()) {
                out.insert(full.to_lowercase(), scu);
            }
            if let Some(slug) = r.slug.as_deref().filter(|s| !s.is_empty()) {
                out.insert(slug.to_lowercase(), scu);
            }
        }
        Ok(out)
    }
}

/// Unix-Sekunden (Lokalzeit unabhängig).
fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl UexProvider {
    /// Fetch plan for one market query (P3, Feedback-Runde 4 — hybrid):
    ///
    /// * explicit `item_id` → ONE `id_item` fetch (+ `operation`),
    /// * free-text q → `id_item` fetches for every term that names a known
    ///   item (complete per-item sets), PLUS the full offer window
    ///   (`operation=sell` + `operation=buy`, or only the selected
    ///   operation) — q may also match titles/sellers/locations the item
    ///   resolution never sees, so the id-only slice under-reported. Both
    ///   plan families are MERGED client-side (`seen`-dedupe) and the
    ///   COMPLETE set is then filtered + sorted by
    ///   `apply_marketplace_filters`, so "Parallax + Min-Q 900" lists every
    ///   matching offer instead of a newest-window slice.
    /// * no q and no ids → the general sell+buy windows (snapshot path).
    ///
    /// The extra full windows only run when the query is non-empty; the
    /// empty-q fast path (cacheable snapshot) stays untouched (rate limit).
    fn build_market_plans(
        &self,
        filters: &MarketplaceFilter,
        resolved_ids: &[u64],
    ) -> Vec<Vec<(String, String)>> {
        let mut plans: Vec<Vec<(String, String)>> = Vec::new();
        if let Some(item_id) = filters.item_id {
            let mut params = vec![("id_item".to_string(), item_id.to_string())];
            if let Some(op) = filters.operation {
                params.push(("operation".to_string(), op.as_str().to_string()));
            }
            plans.push(params);
        } else {
            for id in resolved_ids {
                let mut params = vec![("id_item".to_string(), id.to_string())];
                if let Some(op) = filters.operation {
                    params.push(("operation".to_string(), op.as_str().to_string()));
                }
                plans.push(params);
            }
            if !filters.q.trim().is_empty() {
                // Hybrid: add the FULL offer window(s) on top of the
                // id_item fetches so the local q/quality/sort filter runs
                // over the complete market, not just the resolved slice.
                match filters.operation {
                    Some(op) => {
                        plans.push(vec![("operation".to_string(), op.as_str().to_string())])
                    }
                    None => {
                        for op in [MarketOperation::Sell, MarketOperation::Buy] {
                            plans.push(vec![("operation".to_string(), op.as_str().to_string())]);
                        }
                    }
                }
            } else if resolved_ids.is_empty() {
                // Empty q, nothing resolved → the general windows (only
                // reachable when `operation` is set, else the snapshot
                // fast path already returned).
                if let Some(op) = filters.operation {
                    plans.push(vec![("operation".to_string(), op.as_str().to_string())]);
                } else {
                    for op in [MarketOperation::Sell, MarketOperation::Buy] {
                        plans.push(vec![("operation".to_string(), op.as_str().to_string())]);
                    }
                }
            }
        }
        plans
    }

    /// Look `query` up in the locally cached non-commodity reference rows
    /// (items, then terminals). An exact match means "exists here, but has no
    /// commodity prices" → `Ok(vec![])`; no match stays a NotFound.
    async fn empty_prices_for_other(&self, query: &str) -> Result<Vec<PricePoint>> {
        let q = query.trim();
        for kind in [EntityKind::Items, EntityKind::Terminals] {
            self.ensure_fresh(kind).await?;
            let hits = self.cache.search(&[kind], q, 10).map_err(map_cache_err)?;
            if hits.iter().any(|h| h.name.eq_ignore_ascii_case(q)) {
                return Ok(Vec::new());
            }
        }
        Err(ProviderError::NotFound(format!(
            "commodity '{query}' not found in reference data"
        )))
    }

    /// Price points of ONE terminal (buy/sell per commodity, plus item prices
    /// for non-commodity shops like Guns/Medical). Cached 30 min, else fetched
    /// live via `/commodities_prices?id_terminal=…` and
    /// `/items_prices?id_terminal=…`.
    async fn terminal_prices_impl(&self, terminal_id: u64) -> Result<Vec<PricePoint>> {
        // Fresh marker = "this terminal was checked recently" — an EMPTY
        // cached table is a valid state (fuel/refinery docks trade nothing)
        // and must be served, not refetched every open.
        if self
            .cache
            .terminal_prices_fresh(terminal_id)
            .map_err(map_cache_err)?
        {
            return self
                .cache
                .terminal_prices_for(terminal_id)
                .map_err(map_cache_err);
        }
        let rows: Vec<PriceRow> = self
            .http
            .get(
                "/commodities_prices",
                &[("id_terminal".to_string(), terminal_id.to_string())],
            )
            .await?;
        let mut points = rows
            .into_iter()
            .map(|r| map_price_point(r, ""))
            .collect::<Vec<_>>();
        // Merge item prices (weapons/armor/shop items) so non-commodity
        // terminals show their shop inventories alongside commodities. This
        // is a *best-effort* enrich: a single malformed /items_prices row (or
        // endpoint outage) must never sink the commodity table — fall back to
        // empty item rows (QA runde 2: ein `null` in price_sell riss die
        // ganze Terminal-Tabelle runter).
        let item_rows: Vec<ItemPriceRow> = self
            .http
            .get_optional(
                "/items_prices",
                &[("id_terminal".to_string(), terminal_id.to_string())],
            )
            .await
            .unwrap_or_default()
            .unwrap_or_default();
        points.extend(item_rows.into_iter().map(|r| map_item_price_point(r, "")));
        self.cache
            .store_terminal_prices(terminal_id, &points)
            .map_err(map_cache_err)?;
        Ok(points)
    }

    /// Cached price table of ONE item across all terminals that sell it
    /// (the item detail view). Fetched live via `/items_prices?id_item=…`
    /// when the cache is stale.
    async fn item_prices_impl(&self, item_id: u64) -> Result<Vec<PricePoint>> {
        if self
            .cache
            .item_prices_fresh(item_id)
            .map_err(map_cache_err)?
        {
            return self.cache.item_prices_for(item_id).map_err(map_cache_err);
        }
        let rows: Vec<ItemPriceRow> = self
            .http
            .get_optional(
                "/items_prices",
                &[("id_item".to_string(), item_id.to_string())],
            )
            .await?
            .unwrap_or_default();
        let points = rows
            .into_iter()
            .map(|r| map_item_price_point(r, ""))
            .collect::<Vec<_>>();
        self.cache
            .store_item_prices(item_id, &points)
            .map_err(map_cache_err)?;
        Ok(points)
    }

    /// Aggregate the prices of a location: resolve the clicked terminal from
    /// the reference cache, find every terminal that shares its location
    /// chain (UEX prices are terminal-scoped; one location = all terminals
    /// under one chain, e.g. the four Checkmate terminals of one station),
    /// then fetch each terminal's commodity table.
    async fn location_prices_impl(&self, terminal_id: u64) -> Result<LocationPrices> {
        self.ensure_fresh(EntityKind::Terminals).await?;
        let terminals = self
            .cache
            .entities_of(EntityKind::Terminals)
            .map_err(map_cache_err)?;
        let target = terminals
            .iter()
            .find(|e| e.id == terminal_id)
            .ok_or_else(|| {
                ProviderError::NotFound(format!(
                    "terminal {terminal_id} not found in reference data"
                ))
            })?;
        let chain = target.subtitle.clone();
        if chain.is_empty() {
            return Err(ProviderError::NotFound(format!(
                "terminal {terminal_id} has no location chain"
            )));
        }

        // Sibling terminals of the same location (same chain). The clicked
        // terminal is always included, so its own prices are never lost.
        let mut groups = Vec::new();
        for terminal in terminals.iter().filter(|e| e.subtitle == chain) {
            let points = self.terminal_prices_impl(terminal.id).await?;
            groups.push(TerminalPriceGroup {
                terminal_id: terminal.id,
                terminal_name: terminal.name.clone(),
                prices: points,
            });
        }
        // Alphabetisch nach Terminal-Name (t_dc152764): die Location-Detail-
        // Ansicht gruppiert je Terminal — stabile, lesbare Reihenfolge.
        groups.sort_by(|a, b| {
            a.terminal_name
                .to_lowercase()
                .cmp(&b.terminal_name.to_lowercase())
        });

        Ok(LocationPrices {
            location: chain,
            terminal: target.name.clone(),
            groups,
        })
    }
}

fn map_cache_err(e: CacheError) -> ProviderError {
    ProviderError::Cache(e.to_string())
}

/// Map a `/commodities_prices` row (commodity- OR terminal-scoped — same wire
/// shape) onto the seam [`PricePoint`]. `fallback_commodity` is used when the
/// row omits the name (ageing API rows); the terminal name falls back to a
/// readable id form.
fn map_price_point(r: PriceRow, fallback_commodity: &str) -> PricePoint {
    PricePoint {
        commodity: if r.commodity_name.is_empty() {
            if fallback_commodity.is_empty() {
                format!("commodity {}", r.id_commodity)
            } else {
                fallback_commodity.to_string()
            }
        } else {
            r.commodity_name
        },
        commodity_id: r.id_commodity,
        location: if r.terminal_name.is_empty() {
            format!("terminal {}", r.id_terminal)
        } else {
            r.terminal_name
        },
        terminal_id: r.id_terminal,
        sell_price: r.price_sell,
        buy_price: r.price_buy,
        sell_stock: r.scu_sell_avg,
        buy_stock: r.scu_buy_avg,
        sell_status: r.status_sell,
        buy_status: r.status_buy,
        quality: r.quality,
        updated: r.date_modified,
    }
}

/// Map a `/items_prices` row onto the seam [`PricePoint`]. Items only carry a
/// buy price (the terminal buys nothing back from the player — `price_sell`
/// is always 0); the item name is used as the commodity label so the same
/// frontend table renders both commodity and item prices.
fn map_item_price_point(r: ItemPriceRow, fallback_item: &str) -> PricePoint {
    PricePoint {
        commodity: if r.item_name.is_empty() {
            if fallback_item.is_empty() {
                format!("item {}", r.id_item)
            } else {
                fallback_item.to_string()
            }
        } else {
            r.item_name
        },
        commodity_id: r.id_item,
        location: if r.terminal_name.is_empty() {
            format!("terminal {}", r.id_terminal)
        } else {
            r.terminal_name
        },
        terminal_id: r.id_terminal,
        sell_price: r.price_sell,
        buy_price: r.price_buy,
        sell_stock: 0.0,
        buy_stock: 0.0,
        sell_status: if r.price_sell > 0.0 { 1 } else { 0 },
        buy_status: if r.price_buy > 0.0 { 1 } else { 0 },
        quality: 0,
        updated: r.date_modified,
    }
}

/// Re-export of wire DTOs for integration tests only. Not part of the API.
pub mod private {
    pub use crate::dto::{
        CategoryRow, CommodityRow, ItemPriceRow, ItemRow, ListingRow, PriceRow, StarSystemRow,
        TerminalRow,
    };
    pub use crate::http::DEFAULT_BASE_URL;
}

#[cfg(test)]
mod tests {
    use super::*;
    use provider::{ListingType, MarketOperation, OfferSort};

    /// One test offer with a compact default.
    fn offer(id: u64, title: &str, location: &str, seller: &str) -> Offer {
        Offer {
            id,
            title: title.into(),
            description: String::new(),
            item_id: 0,
            category_id: 0,
            operation: MarketOperation::Sell,
            listing_type: ListingType::Item,
            price: 100.0 + id as f64,
            currency: "UEC".into(),
            unit: "scu".into(),
            quantity: 1,
            location: location.into(),
            region: "Stanton".into(),
            seller: seller.into(),
            quality: 900,
            updated: 1000 + id,
            expires: None,
            slug: "slug".into(),
        }
    }

    fn filter(q: &str) -> MarketplaceFilter {
        MarketplaceFilter {
            q: q.into(),
            ..Default::default()
        }
    }

    fn ids(offers: &[Offer]) -> Vec<u64> {
        offers.iter().map(|o| o.id).collect()
    }

    #[test]
    fn market_q_matches_name_type_terminal_and_seller() {
        let offers = vec![
            offer(1, "Gold Ore (Raw)", "Port Tressler", "HaulerCo"),
            offer(2, "Kartographie-Service", "Lorville — TDD", "ServiceGmbH"),
            offer(3, "Hull-C Charter", "Everus Harbor", "RentABoat"),
        ];
        // Name
        assert_eq!(
            ids(&UexProvider::apply_local(offers.clone(), &filter("gold"))),
            vec![offers[0].id],
            "q must match the offer title"
        );
        // Terminal / location
        assert_eq!(
            ids(&UexProvider::apply_local(
                offers.clone(),
                &filter("tressler")
            )),
            vec![offers[0].id],
            "q must match the offer location (Terminal)"
        );
        // Verkäufer
        assert_eq!(
            ids(&UexProvider::apply_local(
                offers.clone(),
                &filter("rentaboat")
            )),
            vec![offers[2].id],
            "q must match the seller"
        );
    }

    #[test]
    fn market_q_matches_listing_type_and_operation() {
        let mut svc = offer(2, "Kartographie-Service", "Lorville", "s");
        svc.listing_type = ListingType::Service;
        let mut buy = offer(3, "Hull-C Charter", "Everus Harbor", "s");
        buy.operation = MarketOperation::Buy;
        let offers = vec![offer(1, "Gold Ore", "Port Tressler", "s"), svc, buy];

        let hits = ids(&UexProvider::apply_local(
            offers[..2].to_vec(),
            &filter("service"),
        ));
        assert_eq!(hits, vec![2], "listing type service must be searchable");

        let hits = ids(&UexProvider::apply_local(
            offers[..3].to_vec(),
            &filter("buy"),
        ));
        assert_eq!(hits, vec![3], "operation buy must be searchable");
    }

    #[test]
    fn market_q_multi_term_and_case_insensitive() {
        let gold_tressler = offer(1, "Gold Ore (Raw)", "Port Tressler", "HaulerCo");
        let tressler_iron = offer(2, "Iron Ore (Raw)", "Port Tressler", "OrbitalMining");
        let gold_everus = offer(3, "Gold Ore (Refined)", "Everus Harbor", "HaulerCo");

        let hits = ids(&UexProvider::apply_local(
            vec![
                gold_tressler.clone(),
                tressler_iron.clone(),
                gold_everus.clone(),
            ],
            &filter("tressler gold"),
        ));
        assert_eq!(hits, vec![gold_tressler.id], "each term must hit somewhere");

        // Case-insensitivity, incl. mixed-case input.
        let hits = ids(&UexProvider::apply_local(
            vec![gold_tressler.clone(), tressler_iron.clone(), gold_everus],
            &filter("TRESSLER Gold"),
        ));
        assert_eq!(hits, vec![gold_tressler.id]);

        // A term that hits nowhere drops every offer.
        let hits = UexProvider::apply_local(
            vec![gold_tressler, tressler_iron.clone()],
            &filter("tressler nickel"),
        );
        assert!(hits.is_empty(), "unknown term must not be silently dropped");
        let _ = tressler_iron;
    }

    #[test]
    fn market_q_empty_keeps_all_and_sorts() {
        let mut old = offer(1, "Old", "Lorville", "s");
        old.updated = 10;
        let mut young = offer(2, "New", "Everus Harbor", "s");
        young.updated = 20;

        // Empty q → everything, default sort newest first.
        let offers = vec![young.clone(), old.clone()];
        let hits = UexProvider::apply_local(offers.clone(), &filter(""));
        assert_eq!(hits.len(), 2, "empty q must not filter");

        let hits = UexProvider::apply_local(offers.clone(), &filter("   "));
        assert_eq!(hits.len(), 2, "whitespace-only q must not filter");

        // Sort and q combine (price_asc).
        let mut cheap = offer(1, "Cheap Gold", "Lorville", "s");
        cheap.price = 50.0;
        let mut dear = offer(2, "Dear Gold", "Everus Harbor", "s");
        dear.price = 500.0;
        let f = MarketplaceFilter {
            q: "gold".into(),
            sort: Some(OfferSort::PriceAsc),
            ..Default::default()
        };
        let hits = UexProvider::apply_local(vec![dear, cheap], &f);
        assert_eq!(hits[0].price, 50.0, "sort must combine with q");
    }

    /// P2 (Feedback-Runde 4): der Typ-Filter lässt nur Kauf/Verkauf-Terminals
    /// durch — generisch über das UEX `type`-Feld, keine Namens-Blacklist.
    #[test]
    fn trade_terminal_type_filter_is_generic() {
        assert!(super::is_trade_terminal_type(Some("commodity")));
        assert!(super::is_trade_terminal_type(Some("item")));
        // Refinery Ore Sales: Spieler VERKAUFEN Erz hier → bleibt.
        assert!(super::is_trade_terminal_type(Some("commodity_raw")));
        assert!(!super::is_trade_terminal_type(Some("fuel")));
        assert!(!super::is_trade_terminal_type(Some("refinery")));
        assert!(!super::is_trade_terminal_type(Some("vehicle_rent")));
        assert!(!super::is_trade_terminal_type(Some("vehicle_buy")));
        assert!(!super::is_trade_terminal_type(None));
    }

    /// P3 (Feedback-Runde 4): hybrid-Fetch-Plan — bei Freitext-q laufen die
    /// id_item-Fetches UND das volle Offer-Fenster (beide Operationen),
    /// damit apply_marketplace_filters über das KOMPLETTE Set filtert. Leere
    /// q ohne ids bleibt beim General-Fetch (Snapshot-Pfad, Rate-Limit).
    #[test]
    fn market_plan_is_hybrid_for_free_text_queries() {
        let provider = UexProvider::with_base_url(
            Arc::new(Cache::open_in_memory().unwrap()),
            None,
            "http://localhost:1",
            std::time::Duration::from_secs(0),
        )
        .unwrap();

        // Freitext mit aufgelösten ids: id_item-Plans + sell- und buy-Fenster.
        let plans = provider.build_market_plans(&filter("parallax"), &[11, 22]);
        assert_eq!(plans.len(), 4, "2 id_item + sell + buy: {plans:?}");
        assert!(plans[0].contains(&("id_item".to_string(), "11".to_string())));
        assert!(plans[1].contains(&("id_item".to_string(), "22".to_string())));
        assert!(
            plans
                .iter()
                .any(|p| p.contains(&("operation".to_string(), "sell".to_string()))),
            "full sell window required: {plans:?}"
        );
        assert!(
            plans
                .iter()
                .any(|p| p.contains(&("operation".to_string(), "buy".to_string()))),
            "full buy window required: {plans:?}"
        );

        // Freitext OHNE ids: trotzdem hybrid (q matcht Titel/Verkäufer/Ort).
        let plans = provider.build_market_plans(&filter("parallax"), &[]);
        assert_eq!(plans.len(), 2, "sell + buy windows only: {plans:?}");

        // Freitext + Operation-Filter: ein Fenster, mit der Operation.
        let mut f = filter("parallax");
        f.operation = Some(MarketOperation::Buy);
        let plans = provider.build_market_plans(&f, &[11]);
        assert_eq!(plans.len(), 2, "id_item + buy window: {plans:?}");
        assert!(plans[1].contains(&("operation".to_string(), "buy".to_string())));

        // Leere q ohne ids/operation: General-Fetch (sell + buy), wie bisher.
        let plans = provider.build_market_plans(&filter(""), &[]);
        assert_eq!(plans.len(), 2, "general windows unchanged: {plans:?}");

        // Explizite item_id: UNVERÄNDERT genau ein Plan (kein Voll-Fetch).
        let mut f = filter("");
        f.item_id = Some(7);
        let plans = provider.build_market_plans(&f, &[]);
        assert_eq!(plans.len(), 1);
        assert!(plans[0].contains(&("id_item".to_string(), "7".to_string())));
    }
}
