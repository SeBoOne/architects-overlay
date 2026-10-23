//! # wiki_client
//!
//! Client for the community-maintained Star Citizen Wiki API
//! (`api.star-citizen.wiki`, OpenAPI v3 — see `https://docs.star-citizen.wiki`).
//!
//! Planned role (research report `t_f74f0a41`): reference source for **items**
//! (name, type, classification, manufacturer, multilingual description),
//! **commodities** (mining/resource metadata) and **locations** (rich
//! parent-child hierarchy, jurisdiction, quantum travel). The Wiki API has no
//! shop/terminal data (deprecated upstream) and no marketplace listings —
//! those stay with the UEX provider.
//!
//! Behavior contract:
//!
//! * **pacing** — search endpoints are documented at 60 req/min per IP, and
//!   list endpoints practically 403 after ~15-20 burst requests without
//!   delay; every request is paced (default 1s → ≤ 60/min, configurable for
//!   tests);
//! * **pagination** — list endpoints return `{data, links, meta}` pages of up
//!   to 200 rows; [`WikiHttpClient::get_all`] transparently walks all pages
//!   (hard page cap guards against runaway loops);
//! * **errors** — transport errors map to [`ProviderError`] (timeout, http,
//!   rate limit); the Wiki API uses standard HTTP status codes, so 403/429
//!   with rate-limit hints become [`ProviderError::RateLimit`].

mod dto;
mod http;

use std::sync::Arc;

use provider::{Entity, EntityKind, Result};

pub use dto::{CommodityRow, Detail, ItemRow, LocationRow};
pub use http::{DEFAULT_BASE_URL, DEFAULT_MIN_INTERVAL, WikiHttpClient};

/// WikiProvider is a stub provider for the Wiki API. It is not yet wired into
/// the UI (no cache, no marketplace) — Step 1 delivers the crate structure,
/// DTOs, HTTP layer and entity fetching.
pub struct WikiProvider {
    http: WikiHttpClient,
    _shared_cache: Option<Arc<()>>, // placeholder until cache wiring lands
}

impl WikiProvider {
    /// Construct against the production API.
    pub fn new() -> Result<WikiProvider> {
        WikiProvider::with_base_url(DEFAULT_BASE_URL, DEFAULT_MIN_INTERVAL)
    }

    /// Constructor with explicit base URL + pacing override (tests, staging).
    pub fn with_base_url(base_url: &str, min_interval: std::time::Duration) -> Result<WikiProvider> {
        let http = WikiHttpClient::new(base_url, min_interval)?;
        Ok(WikiProvider {
            http,
            _shared_cache: None,
        })
    }

    /// Fetch the complete commodities dataset (mining/resource metadata).
    pub async fn fetch_commodities(&self) -> Result<Vec<CommodityRow>> {
        self.http.get_all("/api/commodities", &[]).await
    }

    /// Fetch the complete items dataset (~12.6k rows; paginated at 200/page).
    pub async fn fetch_items(&self) -> Result<Vec<ItemRow>> {
        self.http.get_all("/api/items", &[]).await
    }

    /// Fetch the complete locations dataset (~2k rows).
    pub async fn fetch_locations(&self) -> Result<Vec<LocationRow>> {
        self.http.get_all("/api/locations", &[]).await
    }

    /// Map a fetched dataset onto seam [`Entity`] rows. The wiki ID space is
    /// UUID-based (not numeric like UEX) — entity `id` is therefore a hash of
    /// the UUID, and the UUID itself rides in `code` so downstream cache
    /// layers can map back without collisions against UEX ids.
    pub fn map_commodities(&self, rows: Vec<CommodityRow>) -> Vec<Entity> {
        rows.into_iter()
            .map(|r| Entity {
                id: dto::stable_id(&r.uuid),
                kind: EntityKind::Commodities,
                name: r.name,
                code: Some(r.slug),
                subtitle: r.commodity_groups.unwrap_or_default().join(" > "),
            })
            .collect()
    }

    pub fn map_items(&self, rows: Vec<ItemRow>) -> Vec<Entity> {
        rows.into_iter()
            .map(|r| Entity {
                id: dto::stable_id(&r.uuid),
                kind: EntityKind::Items,
                name: r.name,
                code: Some(r.slug),
                subtitle: {
                    let mut parts = Vec::new();
                    if let Some(m) = r.manufacturer.filter(|m| !m.name.is_empty()) {
                        parts.push(m.name);
                    }
                    if let Some(c) = r.classification.filter(|c| !c.is_empty()) {
                        parts.push(c);
                    }
                    parts.join(" · ")
                },
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_commodities_uses_uuid_hash_as_id_and_slug_as_code() {
        let p = WikiProvider::with_base_url("http://localhost:1", std::time::Duration::ZERO)
            .expect("provider");
        let rows = vec![dto::CommodityRow {
            uuid: "11111111-1111-1111-1111-111111111111".into(),
            name: "Agricium".into(),
            slug: "agricium".into(),
            commodity_groups: Some(vec!["Raw".into()]),
            ..Default::default()
        }];
        let entities = p.map_commodities(rows);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].name, "Agricium");
        assert_eq!(entities[0].code.as_deref(), Some("agricium"));
        assert_eq!(entities[0].subtitle, "Raw");
        assert_ne!(entities[0].id, 0, "uuid hash must be non-zero");
    }

    #[test]
    fn map_items_carries_manufacturer_and_classification() {
        let p = WikiProvider::with_base_url("http://localhost:1", std::time::Duration::ZERO)
            .expect("provider");
        let rows = vec![dto::ItemRow {
            uuid: "22222222-2222-2222-2222-222222222222".into(),
            name: "Calico Arms Desert".into(),
            slug: "calico-arms-desert".into(),
            classification: Some("FPS.Armor.Arms".into()),
            manufacturer: Some(dto::ManufacturerRef {
                name: "Kastak Arms".into(),
                code: Some("KSAR".into()),
            }),
            ..Default::default()
        }];
        let entities = p.map_items(rows);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].subtitle, "Kastak Arms · FPS.Armor.Arms");
    }
}
