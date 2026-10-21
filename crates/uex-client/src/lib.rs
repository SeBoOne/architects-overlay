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
//! * **marketplace** — always live (`/marketplace_listings`), never served
//!   stale; the last snapshot is stored for offline display;
//! * **entities** — full reference datasets: `/commodities`, `/terminals`,
//!   `/items` (per category, items require `id_category`).

mod dto;
mod http;

use std::sync::Arc;

use cache::{Cache, CacheError};
use http::{DEFAULT_BASE_URL, DEFAULT_MIN_INTERVAL, UexHttpClient};
use provider::{
    DataProvider, Entity, EntityKind, ListingType, MarketOperation, MarketplaceFilter, Offer,
    PricePoint, ProviderError, Result, SearchEntities, SearchHit,
};

use dto::{CategoryRow, CommodityRow, ItemRow, ListingRow, PriceRow, TerminalRow};

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
        UexProvider::with_base_url(
            cache,
            api_key,
            DEFAULT_BASE_URL,
            DEFAULT_MIN_INTERVAL,
        )
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
            .or_else(|| hits.iter().find(|h| h.code.as_deref() == Some(query.trim())));
        let hit = exact.or(hits.first()).ok_or_else(|| {
            ProviderError::NotFound(format!(
                "commodity '{query}' not found in reference data"
            ))
        })?;
        Ok((hit.id, hit.name.clone()))
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
        let (id, name) = self.resolve_commodity(commodity).await?;

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
            .map(|r| PricePoint {
                commodity: if r.commodity_name.is_empty() { name.clone() } else { r.commodity_name },
                commodity_id: r.id_commodity,
                location: if r.terminal_name.is_empty() { format!("terminal {}", r.id_terminal) } else { r.terminal_name },
                terminal_id: r.id_terminal,
                sell_price: r.price_sell,
                buy_price: r.price_buy,
                sell_stock: r.scu_sell_avg,
                buy_stock: r.scu_buy_avg,
                sell_status: r.status_sell,
                buy_status: r.status_buy,
                quality: r.quality,
                updated: r.date_modified,
            })
            .collect::<Vec<_>>();
        self.cache.store_prices(&points).map_err(map_cache_err)?;
        Ok(points)
    }

    async fn marketplace_offers(&self, filters: MarketplaceFilter) -> Result<Vec<Offer>> {
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
        let query = filters.q.to_lowercase();
        let location = filters.location.to_lowercase();

        let offers = rows
            .into_iter()
            .filter(|r| {
                (query.is_empty()
                    || r.title.to_lowercase().contains(&query)
                    || r.description.to_lowercase().contains(&query)
                    || r.user_name.to_lowercase().contains(&query))
                    && (location.is_empty() || r.location.to_lowercase().contains(&location))
            })
            .map(|r| Offer {
                id: r.id,
                title: r.title,
                description: r.description,
                item_id: r.id_item,
                category_id: r.id_category,
                operation: r.operation.parse().unwrap_or(MarketOperation::Sell),
                listing_type: r.r#type.parse().unwrap_or(ListingType::Item),
                price: r.price.parse::<f64>().unwrap_or(0.0),
                currency: if r.currency.is_empty() { "UEC".into() } else { r.currency },
                unit: r.unit,
                quantity: r.in_stock,
                location: r.location,
                seller: r.user_name,
                quality: r.quality.unwrap_or(0),
                updated: r.date_added,
                expires: r.date_expiration,
            })
            .collect::<Vec<_>>();

        // Keep the snapshot for offline display (never served as fresh).
        self.cache.store_marketplace(&offers).map_err(map_cache_err)?;
        Ok(offers)
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
                            if !field.is_empty() && parts.last().map(String::as_str) != Some(field.as_str()) {
                                parts.push(field);
                            }
                        }
                        Entity {
                            id: r.id,
                            kind: EntityKind::Terminals,
                            name: r.displayname.or(r.fullname).unwrap_or_default(),
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
        }
    }

    fn name(&self) -> String {
        String::from("uex")
    }
}

fn map_cache_err(e: CacheError) -> ProviderError {
    ProviderError::Cache(e.to_string())
}

/// Re-export of wire DTOs for integration tests only. Not part of the API.
pub mod private {
    pub use crate::dto::{CategoryRow, CommodityRow, ItemRow, ListingRow, PriceRow, TerminalRow};
}
