//! # wiki_client
//!
//! Client for the community-maintained Star Citizen Wiki API
//! (`api.star-citizen.wiki`, OpenAPI v3 — see `https://docs.star-citizen.wiki`).
//!
//! Role (research report `t_f74f0a41`, second data source beside UEX):
//!
//! * **commodity prices** — the commodity DETAIL endpoint
//!   (`GET /api/commodities/{uuid}`) embeds `uex_prices.purchase[]` with
//!   terminal buy/sell prices in the UEX terminal id space (live-verified:
//!   Agricium — buy 8379 aUEC @ LEVSKI, sell 9800 @ STANTG). Wiki price rows
//!   are cached under `source = 'wiki'` and are the primary commodity price
//!   source; UEX stays the fallback for gaps.
//! * **locations** — the rich parent-child hierarchy (~2k rows) feeds the
//!   Locations search group; price aggregates still open through UEX
//!   terminals, which remain the shop/terminal catalog.
//! * **NOT from the wiki** — item shop prices (deprecated upstream, no
//!   replacement), marketplace listings and terminals (stay UEX).
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

use cache::{Cache, CacheError, SOURCE_WIKI};
use provider::{Entity, EntityKind, PricePoint, ProviderError, Result};

pub use dto::{
    CommodityDetail, CommodityRow, Detail, ItemRow, LocationRow, UexPriceEntry, UexPrices,
};
pub use http::{DEFAULT_BASE_URL, DEFAULT_MIN_INTERVAL, WikiHttpClient};

/// Wiki provider: fetches the wiki reference datasets into the shared cache
/// (`source = 'wiki'`) and serves commodity prices from the detail endpoint.
/// The full `DataProvider` seam is implemented by the composite provider in
/// the app layer (`src-tauri::providers`), which combines this source with
/// UEX and applies the fallback/dedupe policy.
pub struct WikiProvider {
    http: WikiHttpClient,
    cache: Arc<Cache>,
}

impl WikiProvider {
    /// Construct against the production API with the shared cache.
    pub fn new(cache: Arc<Cache>) -> Result<WikiProvider> {
        WikiProvider::with_base_url(DEFAULT_BASE_URL, DEFAULT_MIN_INTERVAL, cache)
    }

    /// Constructor with explicit base URL + pacing override (tests, staging).
    pub fn with_base_url(
        base_url: &str,
        min_interval: std::time::Duration,
        cache: Arc<Cache>,
    ) -> Result<WikiProvider> {
        let http = WikiHttpClient::new(base_url, min_interval)?;
        Ok(WikiProvider { http, cache })
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

    /// Fetch ONE commodity's detail (the list endpoint carries no prices;
    /// only this endpoint embeds the `uex_prices` terminal table).
    pub async fn fetch_commodity_detail(&self, uuid: &str) -> Result<CommodityDetail> {
        self.http
            .get_detail(&format!("/api/commodities/{uuid}"))
            .await
    }

    /// Ensure the wiki commodities dataset is cached (fetch + store under
    /// `source = 'wiki'` when the patch-driven TTL has expired). Nop otherwise.
    pub async fn ensure_commodities(&self) -> Result<()> {
        if self
            .cache
            .is_fresh_sourced(EntityKind::Commodities, SOURCE_WIKI)
            .map_err(map_cache_err)?
        {
            return Ok(());
        }
        let rows = self.fetch_commodities().await?;
        let entities = self.map_commodities(rows);
        self.cache
            .store_entities_sourced(EntityKind::Commodities, SOURCE_WIKI, &entities)
            .map_err(map_cache_err)?;
        Ok(())
    }

    /// Ensure the wiki locations dataset is cached (same policy as
    /// [`WikiProvider::ensure_commodities`]).
    pub async fn ensure_locations(&self) -> Result<()> {
        if self
            .cache
            .is_fresh_sourced(EntityKind::Locations, SOURCE_WIKI)
            .map_err(map_cache_err)?
        {
            return Ok(());
        }
        let rows = self.fetch_locations().await?;
        let entities = self.map_locations(rows);
        self.cache
            .store_entities_sourced(EntityKind::Locations, SOURCE_WIKI, &entities)
            .map_err(map_cache_err)?;
        Ok(())
    }

    /// Resolve a wiki commodity by name (case-insensitive; exact match wins,
    /// then slug/code) against the cached wiki dataset. NotFound when the
    /// dataset has no such commodity.
    pub async fn resolve_commodity(&self, query: &str) -> Result<(String, String)> {
        self.ensure_commodities().await?;
        let q = query.trim();
        let entities = self
            .cache
            .entities_of_sourced(EntityKind::Commodities, Some(SOURCE_WIKI))
            .map_err(map_cache_err)?;
        let exact = entities
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(q))
            .or_else(|| {
                entities
                    .iter()
                    .find(|e| e.name.to_lowercase().contains(&q.to_lowercase()))
            });
        let hit = exact.ok_or_else(|| {
            ProviderError::NotFound(format!(
                "commodity '{query}' not found in wiki reference data"
            ))
        })?;
        let uuid = hit
            .code
            .clone()
            .ok_or_else(|| ProviderError::Other(format!("wiki commodity '{query}' has no uuid")))?;
        Ok((uuid, hit.name.clone()))
    }

    /// Terminal price points for one commodity, sourced from the wiki detail
    /// endpoint (`uex_prices.purchase`). Cached under `source = 'wiki'` with
    /// the 30-min price TTL; duplicate terminal ids are deduped (the wiki
    /// table can repeat a terminal across game versions — first row wins,
    /// which is the most recent one upstream).
    pub async fn prices_for(&self, commodity: &str) -> Result<Vec<PricePoint>> {
        let (uuid, name) = self.resolve_commodity(commodity).await?;
        let id = dto::stable_id(&uuid);

        // Serve cached wiki prices while fresh (≤ 30 min).
        if self
            .cache
            .prices_fresh_sourced(id, SOURCE_WIKI)
            .map_err(map_cache_err)?
        {
            let cached = self
                .cache
                .prices_for_sourced(id, Some(SOURCE_WIKI))
                .map_err(map_cache_err)?;
            if !cached.is_empty() {
                return Ok(cached);
            }
        }

        let detail = self.fetch_commodity_detail(&uuid).await?;
        let points = Self::map_price_entries(&name, id, detail.uex_prices.purchase);
        self.cache
            .store_prices_sourced(&points, SOURCE_WIKI)
            .map_err(map_cache_err)?;
        Ok(points)
    }

    /// Map a fetched dataset onto seam [`Entity`] rows. The wiki ID space is
    /// UUID-based (not numeric like UEX) — entity `id` is therefore a hash of
    /// the UUID, and the UUID itself rides in `code` so downstream cache
    /// layers can map back without collisions against UEX ids.
    pub fn map_commodities(&self, rows: Vec<CommodityRow>) -> Vec<Entity> {
        rows.into_iter()
            .filter(|r| !r.uuid.is_empty())
            .map(|r| Entity {
                id: dto::stable_id(&r.uuid),
                kind: EntityKind::Commodities,
                name: r.name,
                // The UUID rides in `code`: it is the resolution key for the
                // detail endpoint (the price path needs it; the slug is
                // display-only). The stable hash id keeps the row distinct
                // from UEX commodity ids.
                code: Some(r.uuid.clone()),
                subtitle: {
                    let groups = r.commodity_groups.unwrap_or_default().join(" > ");
                    if groups.is_empty() { r.slug } else { groups }
                },
            })
            .collect()
    }

    pub fn map_items(&self, rows: Vec<ItemRow>) -> Vec<Entity> {
        rows.into_iter()
            .filter(|r| !r.uuid.is_empty())
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

    /// Map the wiki location hierarchy onto seam [`Entity`] rows
    /// (`kind = Locations`). `subtitle` is the location chain in the SAME
    /// shape the UEX terminal rows use ("Stanton > Hurston > Aberdeen") —
    /// that chain is what makes a wiki location resolvable to the UEX
    /// terminals of the same place (Location→Terminal-Konsistenz).
    pub fn map_locations(&self, rows: Vec<LocationRow>) -> Vec<Entity> {
        rows.into_iter()
            .filter(|r| !r.uuid.is_empty())
            .map(|r| {
                let name = r.name.clone();
                let mut chain: Vec<String> = Vec::new();
                if let Some(system) = r.system.as_deref() {
                    let system = system.trim().trim_end_matches(" System");
                    if !system.is_empty() {
                        chain.push(system.to_string());
                    }
                }
                // Walk up the parent chain as far as the row carries it (the
                // list endpoint gives one parent level; deeper levels come
                // from the system/star fields).
                if let Some(parent) = &r.parent {
                    if !parent.name.is_empty() {
                        chain.push(parent.name.clone());
                    }
                }
                Entity {
                    id: dto::stable_id(&r.uuid),
                    kind: EntityKind::Locations,
                    name,
                    code: Some(r.slug),
                    subtitle: {
                        if !chain.iter().any(|c| c == &r.name) {
                            chain.push(r.name.clone());
                        }
                        normalize_chain(chain).join(" > ")
                    },
                }
            })
            .collect()
    }

    /// Map `uex_prices.purchase` entries onto seam price points. `location`
    /// is the terminal's display name (fallback: UEX code, then the numeric
    /// id) so the price table reads the same as UEX-sourced rows.
    pub fn map_price_entries(
        commodity_name: &str,
        commodity_id: u64,
        entries: Vec<UexPriceEntry>,
    ) -> Vec<PricePoint> {
        let mut out: Vec<PricePoint> = Vec::new();
        let mut seen: std::collections::HashSet<u64> = std::collections::HashSet::new();
        for e in entries {
            if e.terminal_id == 0 || !seen.insert(e.terminal_id) {
                continue;
            }
            let location = e
                .terminal_name
                .clone()
                .filter(|n| !n.is_empty())
                .or(e.terminal_code.clone().filter(|c| !c.is_empty()))
                .unwrap_or_else(|| format!("terminal {}", e.terminal_id));
            let chain = e
                .starmap_location
                .as_ref()
                .map(|s| {
                    [
                        s.star_system_name.clone().unwrap_or_default(),
                        s.parent_name.clone().unwrap_or_default(),
                        s.name.clone(),
                    ]
                    .into_iter()
                    .filter(|p| !p.is_empty())
                    .collect::<Vec<_>>()
                    .join(" > ")
                })
                .unwrap_or_default();
            out.push(PricePoint {
                commodity: commodity_name.to_string(),
                commodity_id,
                // The display column carries the terminal name; the chain is
                // appended in parentheses only when it adds information.
                location: if chain.is_empty() || chain == location {
                    location
                } else {
                    format!("{location} ({chain})")
                },
                terminal_id: e.terminal_id,
                // UEX price_buy = Spieler-Kaufpreis, price_sell = Spieler-
                // Verkaufspreis (Review t_15e598e7) — direkte Mappung.
                sell_price: e.price_sell,
                buy_price: e.price_buy,
                sell_stock: 0.0,
                buy_stock: 0.0,
                sell_status: if e.price_sell > 0.0 { 1 } else { 0 },
                buy_status: if e.price_buy > 0.0 { 1 } else { 0 },
                quality: 0,
                updated: e
                    .date_updated
                    .as_deref()
                    .and_then(parse_iso8601)
                    .unwrap_or(0),
            });
        }
        out
    }
}

/// Ketten-Normalisierung (t_925a577a): die Wiki-API doppelte Segmente in der
/// Hierarchie-Kette — z. B. meldet sie für Levski „Nyx > Nyx > Levski“
/// (System + Planet tragen denselben Namen). Aufeinanderfolgende Duplikate
/// (und leere Segmente) kollabieren: „Nyx > Nyx > Levski“ → „Nyx > Levski“.
/// Der Composite-Provider matcht die Kette zusätzlich tolerant je Segment
/// (`chain_has`), so dass auch gegen die UEX-Kette „Nyx > Delamar > Levski“
/// eine Auflösung gelingt.
fn normalize_chain(chain: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for seg in chain {
        let seg = seg.trim();
        if seg.is_empty() {
            continue;
        }
        if out.last().map(|last| last == seg).unwrap_or(false) {
            continue;
        }
        out.push(seg.to_string());
    }
    out
}

fn map_cache_err(e: CacheError) -> ProviderError {
    ProviderError::Cache(e.to_string())
}

/// Parse an ISO-8601 timestamp (`2026-10-20T15:09:33+00:00`) to unix seconds.
/// Hand-rolled (no chrono dependency in this crate): calendar math per
/// Howard Hinnant's `days_from_civil`; the timezone offset is applied
/// relative to UTC.
pub fn parse_iso8601(s: &str) -> Option<u64> {
    let bytes = s.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let num = |from: usize, to: usize| s.get(from..to)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, sec) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    // days_from_civil (Hinnant) — valid for the proleptic Gregorian calendar.
    let y = if mo <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let mut secs = days * 86_400 + h * 3_600 + mi * 60 + sec;
    // Timezone offset: `+HH:MM` / `-HH:MM` / `Z`.
    if let Some(rest) = s.get(19..) {
        let rest = rest.trim();
        if let Some(sign) = rest.chars().next() {
            if sign == '+' || sign == '-' {
                let digits: String = rest[1..].chars().filter(|c| c.is_ascii_digit()).collect();
                if digits.len() >= 4 {
                    let oh: i64 = digits[..2].parse().ok()?;
                    let om: i64 = digits[2..4].parse().ok()?;
                    let offset = oh * 3_600 + om * 60;
                    secs = if sign == '+' {
                        secs - offset
                    } else {
                        secs + offset
                    };
                }
            }
        }
    }
    if secs < 0 {
        return None;
    }
    Some(secs as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn provider() -> WikiProvider {
        let cache = Arc::new(Cache::open_in_memory().unwrap());
        WikiProvider::with_base_url("http://localhost:1", Duration::ZERO, cache).expect("provider")
    }

    #[test]
    fn map_commodities_uses_uuid_hash_as_id_and_slug_as_code() {
        let p = provider();
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
        assert_eq!(
            entities[0].code.as_deref(),
            Some("11111111-1111-1111-1111-111111111111"),
            "code carries the uuid (detail-endpoint resolution key)"
        );
        assert_eq!(entities[0].subtitle, "Raw");
        assert_ne!(entities[0].id, 0, "uuid hash must be non-zero");
    }

    #[test]
    fn map_items_carries_manufacturer_and_classification() {
        let p = provider();
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

    #[test]
    fn map_locations_builds_uex_shaped_chain() {
        // The chain shape ("Stanton > Hurston > Aberdeen") must match the UEX
        // terminal subtitle shape so the composite provider can resolve a
        // wiki location to the UEX terminals of the same place.
        let p = provider();
        let rows = vec![dto::LocationRow {
            uuid: "f8f07f5b-1c0e-47c9-aa50-46963065bf18".into(),
            name: "Aberdeen".into(),
            slug: "aberdeen".into(),
            system: Some("Stanton System".into()),
            parent: Some(dto::EntityNameRef {
                name: "Hurston".into(),
                ..Default::default()
            }),
            location_type: Some(dto::EntityTypeRef {
                name: Some("Moon".into()),
                classification: None,
            }),
            ..Default::default()
        }];
        let entities = p.map_locations(rows);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].kind, EntityKind::Locations);
        assert_eq!(entities[0].subtitle, "Stanton > Hurston > Aberdeen");
        assert_eq!(entities[0].name, "Aberdeen");
        assert_ne!(entities[0].id, 0);
    }

    /// Regression (t_925a577a): die Wiki-API doppelte Segmente in der Kette —
    /// Levski meldet sie als „Nyx > Nyx > Levski“ (System Nyx + Planet Nyx).
    /// Aufeinanderfolgende Duplikate müssen beim Import kollabieren, sonst
    /// scheitert die exakte Ketten-Matchung gegen die UEX-Terminal-Untertitel
    /// („Nyx > Delamar > Levski“) im Location-Detail.
    #[test]
    fn map_locations_collapses_consecutive_duplicate_segments() {
        let p = provider();
        let rows = vec![
            dto::LocationRow {
                uuid: "40470742-9359-3513-0000-000000000001".into(),
                name: "Levski".into(),
                slug: "levski".into(),
                system: Some("Nyx System".into()),
                parent: Some(dto::EntityNameRef {
                    name: "Nyx".into(),
                    ..Default::default()
                }),
                ..Default::default()
            },
            // Gebiet-Kette des Live-Befunds: „Stanton > Stanton > HUR-L3 …“
            dto::LocationRow {
                uuid: "40470742-9359-3513-0000-000000000002".into(),
                name: "HUR L3".into(),
                slug: "hur-l3".into(),
                system: Some("Stanton System".into()),
                parent: Some(dto::EntityNameRef {
                    name: "Stanton".into(),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ];
        let entities = p.map_locations(rows);
        assert_eq!(entities[0].subtitle, "Nyx > Levski");
        assert_eq!(entities[1].subtitle, "Stanton > HUR L3");
    }

    /// Live-Verifikation (t_925a577a): die echte Wiki-API meldet Levski mit
    /// System Nyx + Star „Nyx“ als Parent — die Kette muss normalisiert
    /// „Nyx > Levski“ liefern (opt-in: `cargo test -p wiki_client -- --ignored`).
    #[tokio::test]
    #[ignore = "hits the live Star Citizen Wiki API"]
    async fn live_locations_levski_chain_is_normalized() {
        let dir = std::env::temp_dir().join(format!(
            "wiki-live-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let cache = Arc::new(cache::Cache::open(&dir.join("live.sqlite3")).unwrap());
        let p = WikiProvider::new(cache).unwrap();
        let rows = p.fetch_locations().await.expect("live locations");
        let entities = p.map_locations(rows);
        let levski: Vec<_> = entities
            .iter()
            .filter(|e| e.name == "Levski")
            .map(|e| (e.name.clone(), e.subtitle.clone()))
            .collect();
        eprintln!("levski rows: {levski:?}");
        assert!(
            levski.iter().any(|(_, sub)| sub == "Nyx > Levski"),
            "Levski-Kette muss normalisiert sein, got {levski:?}"
        );
        // Keine Zeile darf aufeinanderfolgende Duplikate tragen.
        for (name, sub) in &levski {
            let segs: Vec<&str> = sub.split(" > ").map(str::trim).collect();
            for w in segs.windows(2) {
                assert!(
                    !w[0].eq_ignore_ascii_case(w[1]),
                    "row {name}: consecutive duplicate in {sub}"
                );
            }
        }
    }

    #[test]
    fn map_price_entries_maps_buy_sell_and_dedupes_terminals() {
        let entries = vec![
            dto::UexPriceEntry {
                price_buy: 8_379.0,
                price_sell: 0.0,
                terminal_id: 778,
                terminal_name: Some("Levski".into()),
                starmap_location: Some(dto::StarmapLocationRef {
                    name: "Levski".into(),
                    parent_name: Some("Delamar".into()),
                    star_system_name: Some("Nyx".into()),
                    ..Default::default()
                }),
                date_updated: Some("2026-10-20T15:09:33+00:00".into()),
                ..Default::default()
            },
            dto::UexPriceEntry {
                price_buy: 0.0,
                price_sell: 9_800.0,
                terminal_id: 802,
                terminal_code: Some("STANTG".into()),
                date_updated: Some("2026-10-19T22:36:18+00:00".into()),
                ..Default::default()
            },
            // duplicate terminal (first row wins)
            dto::UexPriceEntry {
                price_buy: 1.0,
                price_sell: 0.0,
                terminal_id: 778,
                ..Default::default()
            },
            // invalid: no terminal id
            dto::UexPriceEntry {
                price_buy: 5.0,
                terminal_id: 0,
                ..Default::default()
            },
        ];
        let points = WikiProvider::map_price_entries("Agricium", 42, entries);
        assert_eq!(points.len(), 2, "duplicates and id-less rows are dropped");
        assert_eq!(points[0].terminal_id, 778);
        // UEX price_buy = Spieler-Kaufpreis (Review t_15e598e7) → App
        // buy_price direkt; sell_price = 0.
        assert_eq!(points[0].buy_price, 8_379.0);
        assert_eq!(points[0].sell_price, 0.0);
        assert_eq!(points[0].commodity, "Agricium");
        assert_eq!(points[0].commodity_id, 42);
        assert_eq!(
            points[0].location, "Levski (Nyx > Delamar > Levski)",
            "the starmap chain enriches the terminal name"
        );
        assert!(
            points[0].updated > 1_700_000_000,
            "ISO date must parse to unix secs"
        );
        assert_eq!(points[1].terminal_id, 802);
        // UEX price_sell = Spieler-Verkaufspreis → App sell_price.
        assert_eq!(points[1].sell_price, 9_800.0);
        assert_eq!(points[1].location, "STANTG", "code fallback when no name");
    }

    #[test]
    fn parse_iso8601_handles_offset_and_z() {
        // 2026-10-20T15:09:33+00:00 == 2026-10-20T15:09:33Z
        let base = parse_iso8601("2026-10-20T15:09:33+00:00").unwrap();
        assert_eq!(base, parse_iso8601("2026-10-20T15:09:33Z").unwrap());
        // -02:00 offset → 2h later in UTC.
        assert_eq!(parse_iso8601("2026-10-20T13:09:33-02:00"), Some(base));
        // +01:00 offset → 1h earlier in UTC.
        assert_eq!(parse_iso8601("2026-10-20T16:09:33+01:00"), Some(base));
        // Epoch sanity: 1970-01-01T00:00:00Z == 0.
        assert_eq!(parse_iso8601("1970-01-01T00:00:00Z"), Some(0));
        // Garbage → None.
        assert_eq!(parse_iso8601("nope"), None);
        assert_eq!(parse_iso8601(""), None);
    }

    /// wiremock-driven: ensure_commodities fetches + stores under
    /// `source = wiki`, the second call is a TTL nop, and resolve_commodity
    /// finds a cached row by name.
    #[tokio::test]
    async fn ensure_commodities_caches_and_resolves() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/commodities"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    {"uuid": "u1", "name": "Agricium", "slug": "agricium"},
                    {"uuid": "u2", "name": "Titanium", "slug": "titanium"}
                ],
                "links": {"next": null},
                "meta": {"current_page": 1, "per_page": 200, "total": 2}
            })))
            .expect(1)
            .mount(&server)
            .await;
        let cache = Arc::new(Cache::open_in_memory().unwrap());
        let p = WikiProvider::with_base_url(&server.uri(), Duration::ZERO, cache.clone())
            .expect("provider");

        p.ensure_commodities().await.expect("fetch");
        assert_eq!(
            cache
                .count_entities_sourced(EntityKind::Commodities, SOURCE_WIKI)
                .unwrap(),
            2
        );
        // Second call: served from the fresh cache (mock expects exactly 1 hit).
        p.ensure_commodities().await.expect("cached");

        let (uuid, name) = p.resolve_commodity("agricium").await.expect("resolve");
        assert_eq!(name, "Agricium");
        assert_eq!(uuid, "u1");
        let err = p.resolve_commodity("Unknownium").await.unwrap_err();
        assert!(matches!(err, ProviderError::NotFound(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn prices_for_fetches_detail_and_caches() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/commodities"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"uuid": "u1", "name": "Agricium", "slug": "agricium"}],
                "links": {"next": null},
                "meta": {"total": 1}
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/commodities/u1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "uuid": "u1", "name": "Agricium", "slug": "agricium",
                    "uex_prices": {"purchase": [
                        {"price_buy": 8379, "price_sell": 0, "terminal_id": 778,
                         "terminal_name": "Levski", "date_updated": "2026-10-20T15:09:33+00:00"},
                        {"price_buy": 0, "price_sell": 9800, "terminal_id": 802,
                         "terminal_code": "STANTG", "date_updated": "2026-10-19T22:36:18+00:00"}
                    ]}
                }
            })))
            .mount(&server)
            .await;
        let cache = Arc::new(Cache::open_in_memory().unwrap());
        let p = WikiProvider::with_base_url(&server.uri(), Duration::ZERO, cache.clone())
            .expect("provider");

        let points = p.prices_for("Agricium").await.expect("prices");
        assert_eq!(points.len(), 2);
        // UEX price_buy = Spieler-Kaufpreis (Review t_15e598e7) → App
        // buy_price direkt.
        assert_eq!(points[0].buy_price, 8_379.0);
        assert_eq!(points[0].terminal_id, 778);

        // Cached under source=wiki with its own freshness marker.
        let id = dto::stable_id("u1");
        assert!(cache.prices_fresh_sourced(id, SOURCE_WIKI).unwrap());
        assert_eq!(
            cache
                .prices_for_sourced(id, Some(SOURCE_WIKI))
                .unwrap()
                .len(),
            2
        );
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Live smoke against the production API — opt-in via
    /// `cargo test -p wiki_client -- --ignored`.
    #[tokio::test]
    #[ignore = "hits the live api.star-citizen.wiki"]
    async fn live_commodity_detail_carries_terminal_prices() {
        let cache = Arc::new(Cache::open_in_memory().unwrap());
        let p = WikiProvider::new(cache).expect("provider");
        let points = p.prices_for("Agricium").await.expect("live prices");
        assert!(
            !points.is_empty(),
            "Agricium must carry terminal prices from the wiki detail endpoint"
        );
        let buy = points.iter().find(|p| p.buy_price > 0.0);
        let sell = points.iter().find(|p| p.sell_price > 0.0);
        assert!(buy.is_some(), "buy terminal present: {points:?}");
        assert!(sell.is_some(), "sell terminal present: {points:?}");
        eprintln!(
            "live prices: buy {:?} @ {}, sell {:?} @ {}",
            buy.map(|p| p.buy_price),
            buy.map(|p| p.location.clone()).unwrap_or_default(),
            sell.map(|p| p.sell_price),
            sell.map(|p| p.location.clone()).unwrap_or_default()
        );
    }
}
