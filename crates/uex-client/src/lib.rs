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
//! * **marketplace** — served from the short-TTL snapshot (5 min) for
//!   API-parameter-free reads; `id_item`/`operation` filters always fetch live
//!   `/marketplace_listings`; the snapshot is replaced on unfiltered fetches
//!   only, so the cache always holds the general market view;
//! * **entities** — full reference datasets: `/commodities`, `/terminals`,
//!   `/items` (per category, items require `id_category`), `/star_systems`
//!   (region names for marketplace listings).

mod dto;
mod http;

use std::collections::HashMap;
use std::sync::Arc;

use cache::{Cache, CacheError};
use http::{DEFAULT_BASE_URL, DEFAULT_MIN_INTERVAL, UexHttpClient};
use provider::{
    DataProvider, Entity, EntityKind, ListingType, LocationPrices, MarketOperation,
    MarketplaceFilter, Offer, OfferSort, PricePoint, ProviderError, Result, SearchEntities,
    SearchHit, TerminalPriceGroup,
};

use dto::{CategoryRow, CommodityRow, ItemRow, ListingRow, PriceRow, StarSystemRow, TerminalRow};

/// Search result page size served from the cache.
const SEARCH_LIMIT: u32 = 25;
/// Marketplace page size (API default 100; up to 1000 when item+operation set).
const MARKETPLACE_DEFAULT_LIMIT: u32 = 100;
const MARKETPLACE_MAX_LIMIT: u32 = 1000;

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

    /// Ensure the reference dataset for `kind` is in the local cache (fetch +
    /// store when stale). Nop otherwise.
    async fn ensure_fresh(&self, kind: EntityKind) -> Result<()> {
        if self.cache.is_fresh(kind).map_err(map_cache_err)? {
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
    /// views behave identically. All filters except item_id/operation are
    /// applied here (the API exposes no text/sort parameters).
    fn apply_local(mut offers: Vec<Offer>, filters: &MarketplaceFilter) -> Vec<Offer> {
        let query = filters.q.to_lowercase();
        let location = filters.location.to_lowercase();
        let region = filters.region.to_lowercase();
        offers.retain(|o| {
            (query.is_empty()
                || o.title.to_lowercase().contains(&query)
                || o.description.to_lowercase().contains(&query)
                || o.seller.to_lowercase().contains(&query))
                && (location.is_empty() || o.location.to_lowercase().contains(&location))
                && (region.is_empty() || o.region.to_lowercase().contains(&region))
        });
        match filters.sort.unwrap_or(OfferSort::Newest) {
            OfferSort::Newest => {
                offers.sort_by(|a, b| b.updated.cmp(&a.updated).then(b.id.cmp(&a.id)))
            }
            OfferSort::PriceAsc => offers.sort_by(|a, b| a.price.total_cmp(&b.price)),
            OfferSort::PriceDesc => offers.sort_by(|a, b| b.price.total_cmp(&a.price)),
        }
        offers
    }
}

#[async_trait::async_trait]
impl DataProvider for UexProvider {
    async fn search(&self, entities: SearchEntities, query: &str) -> Result<Vec<SearchHit>> {
        let kinds = entities.kinds();
        for kind in kinds {
            self.ensure_fresh(*kind).await?;
        }
        self.cache
            .search(kinds, query, SEARCH_LIMIT)
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

    async fn marketplace_offers(&self, filters: MarketplaceFilter) -> Result<Vec<Offer>> {
        // Short-TTL cache (Phase 2): API-parameter-free reads are served from
        // the last snapshot while it is < TTL_MARKETPLACE old, keeping the
        // offers view instant and rate-limit-schonend. item_id/operation are
        // API params — they always fetch live (cache would be filter-specific).
        let cacheable = filters.item_id.is_none() && filters.operation.is_none();
        if cacheable && self.cache.marketplace_fresh().map_err(map_cache_err)? {
            let snapshot = self.cache.marketplace_snapshot().map_err(map_cache_err)?;
            return Ok(Self::apply_local(snapshot, &filters));
        }

        let mut params: Vec<(String, String)> = Vec::new();
        if let Some(item_id) = filters.item_id {
            params.push(("id_item".to_string(), item_id.to_string()));
        }
        if let Some(op) = filters.operation {
            params.push(("operation".to_string(), op.as_str().to_string()));
        }
        let limit = if filters.limit == 0 {
            MARKETPLACE_DEFAULT_LIMIT
        } else {
            filters.limit.clamp(1, MARKETPLACE_MAX_LIMIT)
        };
        params.push(("limit".to_string(), limit.to_string()));

        let rows: Vec<ListingRow> = self.http.get("/marketplace_listings", &params).await?;

        // Region names: resolve id_star_system → name via the star-systems
        // dataset (lazily fetched once, then 12h TTL in the cache).
        let regions = self.star_system_names().await?;

        let offers: Vec<Offer> = rows
            .into_iter()
            .map(|r| {
                let region = if r.id_star_system != 0 {
                    regions.get(&r.id_star_system).cloned().unwrap_or_default()
                } else {
                    String::new()
                };
                Offer {
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
                }
            })
            .collect();

        // Keep the snapshot for cache/offline display. Only API-parameter-free
        // fetches replace it — a filter-specific fetch would poison the cache.
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
        }
    }

    fn name(&self) -> String {
        String::from("uex")
    }
}

impl UexProvider {
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

    /// Price points of ONE terminal (buy/sell per commodity). Cached 30 min,
    /// else fetched live via `/commodities_prices?id_terminal=…` (the
    /// terminal-scoped variant of the same endpoint used for commodities).
    async fn terminal_prices(&self, terminal_id: u64) -> Result<Vec<PricePoint>> {
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
        let points = rows
            .into_iter()
            .map(|r| map_price_point(r, ""))
            .collect::<Vec<_>>();
        self.cache
            .store_terminal_prices(terminal_id, &points)
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
            let points = self.terminal_prices(terminal.id).await?;
            groups.push(TerminalPriceGroup {
                terminal_id: terminal.id,
                terminal_name: terminal.name.clone(),
                prices: points,
            });
        }
        groups.sort_by_key(|g| g.terminal_id);

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

/// Re-export of wire DTOs for integration tests only. Not part of the API.
pub mod private {
    pub use crate::dto::{
        CategoryRow, CommodityRow, ItemRow, ListingRow, PriceRow, StarSystemRow, TerminalRow,
    };
}
