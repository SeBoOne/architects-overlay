//! # provider
//!
//! **Architecture seam of the app.** `DataProvider` is the single interface the
//! UI talks to for trade data (search, prices, marketplace). UEX is the *first*
//! implementation (`uex_client::UexProvider`); later sources (other SC trade
//! APIs) plug in behind the same trait without touching the UI layer. DTOs live
//! in this crate, so no implementation type ever leaks through the seam.
//!
//! The trait is `async` (native async-fn-in-trait, Rust ≥ 1.75) and requires
//! `Send + Sync` so implementations can back Tauri commands on a tokio runtime
//! and can be held behind `Arc<dyn DataProvider>`.

mod model;

pub use crate::model::{
    Entity, EntityKind, ListingType, LocationPrices, MarketOperation, MarketplaceFilter, Offer,
    OfferSort, PricePoint, SearchEntities, SearchHit, TerminalPriceGroup,
};

/// Domain error produced by a provider implementation.
#[derive(Debug, Clone, serde::Serialize)]
#[non_exhaustive]
pub enum ProviderError {
    /// Transport-level HTTP failure with the status code and a snippet of the
    /// response body.
    Http { status: u16, body: String },
    /// The source reported its request limit is reached (`requests_limit_reached`
    /// on UEX). `retry_after` seconds if the source told us.
    RateLimit { retry_after: Option<u64> },
    /// The request exceeded the client timeout.
    Timeout,
    /// Response could not be parsed (serde/json error message attached).
    Json(String),
    /// The requested entity/commodity is not known to the source.
    NotFound(String),
    /// Local cache failure (SQLite open/query/…).
    Cache(String),
    /// Configuration problem (missing key file, unreadable config, …).
    Config(String),
    /// The source does not support the requested operation/entity kind.
    Unsupported(String),
    /// Catch-all for anything else.
    Other(String),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderError::Http { status, body } => write!(
                f,
                "http error {status}: {}",
                body.chars().take(200).collect::<String>()
            ),
            ProviderError::RateLimit { retry_after } => match retry_after {
                Some(secs) => write!(f, "rate limit reached, retry in {secs}s"),
                None => write!(f, "rate limit reached"),
            },
            ProviderError::Timeout => write!(f, "request timed out"),
            ProviderError::Json(msg) => write!(f, "invalid response: {msg}"),
            ProviderError::NotFound(what) => write!(f, "not found: {what}"),
            ProviderError::Cache(msg) => write!(f, "cache error: {msg}"),
            ProviderError::Config(msg) => write!(f, "configuration error: {msg}"),
            ProviderError::Unsupported(msg) => write!(f, "unsupported: {msg}"),
            ProviderError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for ProviderError {}

/// Shortcut `Result<T>` = `Result<T, ProviderError>` used across the seam.
pub type Result<T> = std::result::Result<T, ProviderError>;

/// Implementations of a trade-data source (UEX today, more later).
///
/// Async via `#[async_trait]` (boxed `Send` futures): dyn-compatible AND
/// spawn-safe on the tokio runtime / Tauri commands. Implementations must be
/// `Send + Sync` so they can live behind `Arc<dyn DataProvider>` in app state.
#[async_trait::async_trait]
pub trait DataProvider: Send + Sync {
    /// Search entities of the given kind across locally cached reference data.
    /// Runs against the local cache (no network), so the UI gets instant
    /// results; a search implementation may refresh stale reference data
    /// on demand or rely on the background sync task.
    async fn search(&self, entities: SearchEntities, query: &str) -> Result<Vec<SearchHit>>;

    /// Current price points for one commodity (matched by name/code/slug).
    /// Prices are volatile and short-lived; the provider re-fetches when the
    /// local copy is older than the price TTL.
    async fn prices(&self, commodity: &str) -> Result<Vec<PricePoint>>;

    /// Price aggregate of one location (= the trade terminal the user opened):
    /// every terminal sharing the location chain, each with its commodity
    /// prices (buy/sell per terminal, location header). Implementations fetch
    /// per-terminal and cache; the default reports the source as unsupported.
    async fn location_prices(&self, terminal_id: u64) -> Result<LocationPrices> {
        let _ = terminal_id;
        Err(ProviderError::Unsupported("location prices".to_string()))
    }

    /// The price table of ONE terminal (buy/sell per commodity, plus item
    /// prices for non-commodity shops like Guns/Medical). Cached 30 min;
    /// an empty table is a valid answer (fuel/refinery docks trade nothing).
    /// Default reports the source as unsupported.
    async fn terminal_prices(&self, terminal_id: u64) -> Result<Vec<PricePoint>> {
        let _ = terminal_id;
        Err(ProviderError::Unsupported("terminal prices".to_string()))
    }

    /// Price points for ONE item across all terminals that sell it
    /// (the item's buy/sell table in the item detail view). Cached 30 min;
    /// an empty table is valid (item not sold anywhere). Default reports the
    /// source as unsupported.
    async fn item_prices(&self, item_id: u64) -> Result<Vec<PricePoint>> {
        let _ = item_id;
        Err(ProviderError::Unsupported("item prices".to_string()))
    }

    /// Live marketplace offers, filtered. Marketplace data is never presented
    /// stale — a fresh fetch happens on every call; the last snapshot is kept
    /// locally for offline display only.
    async fn marketplace_offers(&self, filters: MarketplaceFilter) -> Result<Vec<Offer>>;

    /// Download the full reference dataset of one `EntityKind` (used by the
    /// background sync task). Providers return source-agnostic [`Entity`] rows.
    async fn fetch_entities(&self, kind: EntityKind) -> Result<Vec<Entity>>;

    /// Short source id, e.g. "uex".
    fn name(&self) -> String;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_error_display_is_not_empty() {
        // Regression: the scaffold left a Display impl returning an empty string.
        let errs = [
            ProviderError::Http {
                status: 500,
                body: "boom".into(),
            },
            ProviderError::RateLimit {
                retry_after: Some(60),
            },
            ProviderError::Timeout,
            ProviderError::Json("expected array".into()),
            ProviderError::NotFound("Agricium".into()),
            ProviderError::Cache("disk full".into()),
            ProviderError::Config("missing key".into()),
            ProviderError::Unsupported("routes".into()),
            ProviderError::Other("whatever".into()),
        ];
        for err in errs {
            assert!(!err.to_string().is_empty(), "empty Display for {err:?}");
        }
    }

    #[test]
    fn offer_sort_wire_format_matches_frontend_contract() {
        // The Svelte UI declares OfferSort = 'newest' | 'price_asc' | 'price_desc'
        // and sends `sort` verbatim in the get_marketplace filter — regression for
        // the IPC mismatch (sent price_asc, backend expected priceasc).
        for (variant, wire) in [
            (OfferSort::Newest, "newest"),
            (OfferSort::PriceAsc, "price_asc"),
            (OfferSort::PriceDesc, "price_desc"),
        ] {
            assert_eq!(
                serde_json::to_string(&variant).unwrap(),
                format!("\"{wire}\"")
            );
            let back: OfferSort = serde_json::from_str(&format!("\"{wire}\"")).unwrap();
            assert_eq!(back, variant);
        }
    }

    #[test]
    fn location_prices_serde_roundtrip() {
        // The aggregate crosses the Tauri IPC boundary as JSON — the wire
        // shape must be stable for the frontend TS mirror.
        use crate::{LocationPrices, TerminalPriceGroup};
        let agg = LocationPrices {
            location: "Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station".into(),
            terminal: "Commodity Shop - Admin - Checkmate".into(),
            groups: vec![TerminalPriceGroup {
                terminal_id: 436,
                terminal_name: "Commodity Shop - Admin - Checkmate".into(),
                prices: vec![PricePoint {
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
        };
        let json = serde_json::to_string(&agg).unwrap();
        assert!(
            json.contains(
                "\"location\":\"Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station\""
            ),
            "location header must be the chain: {json}"
        );
        assert!(json.contains("\"groups\""));
        assert!(json.contains("\"terminal_name\":\"Commodity Shop - Admin - Checkmate\""));
        let back: LocationPrices = serde_json::from_str(&json).unwrap();
        assert_eq!(back.groups.len(), 1);
        assert_eq!(back.groups[0].prices[0].buy_price, 1108.0);
    }

    #[test]
    fn search_entities_maps_to_data_kinds() {
        // Locations are served from the terminal dataset (trade locations).
        assert_eq!(SearchEntities::Locations.kinds(), &[EntityKind::Terminals]);
        assert_eq!(SearchEntities::Terminals.kinds(), &[EntityKind::Terminals]);
        assert_eq!(
            SearchEntities::Commodities.kinds(),
            &[EntityKind::Commodities]
        );
        assert_eq!(SearchEntities::Items.kinds(), &[EntityKind::Items]);
    }

    #[test]
    fn dto_serde_roundtrip() {
        let pp = PricePoint {
            commodity: "Agricium".into(),
            commodity_id: 1,
            location: "Area 18".into(),
            terminal_id: 12,
            sell_price: 9_725.0,
            buy_price: 8_631.0,
            sell_stock: 301.0,
            buy_stock: 0.0,
            sell_status: 1,
            buy_status: 0,
            quality: 0,
            updated: 1_792_341_485,
        };
        assert_eq!(
            serde_json::to_string(&pp).unwrap(),
            r#"{"commodity":"Agricium","commodity_id":1,"location":"Area 18","terminal_id":12,"sell_price":9725.0,"buy_price":8631.0,"sell_stock":301.0,"buy_stock":0.0,"sell_status":1,"buy_status":0,"quality":0,"updated":1792341485}"#
        );
        let back: PricePoint = serde_json::from_str(&serde_json::to_string(&pp).unwrap()).unwrap();
        assert_eq!(back.commodity, "Agricium");
        assert_eq!(back.sell_price, 9_725.0);

        let filter = MarketplaceFilter {
            location: "Levski".into(),
            q: "compensator".into(),
            item_id: Some(5332),
            operation: Some(MarketOperation::Sell),
            region: "Nyx".into(),
            sort: Some(OfferSort::PriceAsc),
            limit: 100,
        };
        assert!(
            serde_json::from_str::<MarketplaceFilter>(&serde_json::to_string(&filter).unwrap())
                .is_ok()
        );
    }
}
