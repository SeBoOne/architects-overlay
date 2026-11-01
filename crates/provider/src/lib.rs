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
pub mod routes;

pub use crate::model::{
    AdvertisePayload, AdvertiseResult, Category, Entity, EntityKind, ItemSuggestion, ListingType,
    LocationPrices, MarketOperation, MarketplaceFilter, Negotiation, NegotiationMessage, Offer,
    OfferSort, PricePoint, SearchEntities, SearchHit, TerminalPriceGroup, Trade, TradePayload,
    deserialize_id_flexible, deserialize_opt_id_flexible, serialize_id_as_string,
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
    /// Error with an i18n key + params instead of UI plaintext (T2 t_c5904bcb:
    /// params must NOT contain UI plaintext — the frontend translates via the
    /// fallback chain selected → en → raw key).
    Keyed {
        key: String,
        params: serde_json::Value,
    },
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
            ProviderError::Keyed { key, .. } => write!(f, "error: {key}"),
            ProviderError::Unsupported(msg) => write!(f, "unsupported: {msg}"),
            ProviderError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for ProviderError {}

/// Shortcut `Result<T>` = `Result<T, ProviderError>` used across the seam.
pub type Result<T> = std::result::Result<T, ProviderError>;

/// Client-side marketplace filter + sort, shared by every path that hands
/// offers to the UI (provider live/cache reads AND the offline-snapshot
/// fallback in `trade::get_marketplace`) so all views behave identically.
///
/// The UEX API exposes no text/sort/location parameters (only `id`, `slug`,
/// `username`, `id_item`, `operation`) — everything else is applied here.
///
/// `q` is the market's free-text search: every whitespace-separated term must
/// hit at least one offer field — Name (title), Typ (listing type +
/// operation), Terminal (location) or Verkäufer (seller); description
/// completes the haystack. Terms are AND-ed, fields OR-ed per term,
/// case-insensitive. Sort defaults to newest when unset.
pub fn apply_marketplace_filters(
    mut offers: Vec<Offer>,
    filters: &MarketplaceFilter,
) -> Vec<Offer> {
    let terms: Vec<String> = filters
        .q
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();
    let location = filters.location.to_lowercase();
    let region = filters.region.to_lowercase();
    offers.retain(|o| {
        let title = o.title.to_lowercase();
        let desc = o.description.to_lowercase();
        let seller = o.seller.to_lowercase();
        let loc = o.location.to_lowercase();
        let kind = o.listing_type.as_str().to_string();
        let op = o.operation.as_str().to_string();
        let hay = [&title, &desc, &seller, &loc, &kind, &op];
        (terms.is_empty()
            || terms
                .iter()
                .all(|term| hay.iter().any(|h| h.contains(term))))
            && (location.is_empty() || loc.contains(&location))
            && (region.is_empty() || o.region.to_lowercase().contains(&region))
    });
    match filters.sort.unwrap_or(OfferSort::Newest) {
        OfferSort::Newest => offers.sort_by(|a, b| b.updated.cmp(&a.updated).then(b.id.cmp(&a.id))),
        OfferSort::PriceAsc => offers.sort_by(|a, b| a.price.total_cmp(&b.price)),
        OfferSort::PriceDesc => offers.sort_by(|a, b| b.price.total_cmp(&a.price)),
    }
    offers
}

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

    /// PS (t_c84e545d): force one LIVE refresh of a single commodity's price
    /// table (detail-page F5 / settings "Jetzt aktualisieren"). Bypasses the
    /// freshness cache — exactly ONE API call. The default reports
    /// unsupported so sources without a force path stay honest.
    async fn refresh_prices(&self, commodity: &str) -> Result<Vec<PricePoint>> {
        let _ = commodity;
        Err(ProviderError::Unsupported(
            "refresh_prices: not supported by this source".into(),
        ))
    }

    /// PS (t_c84e545d): force one LIVE refresh of one item's price table
    /// (item-detail F5). Bypasses the (long) freshness cache — one API call.
    async fn refresh_item_prices(&self, item_id: u64) -> Result<Vec<PricePoint>> {
        let _ = item_id;
        Err(ProviderError::Unsupported(
            "refresh_item_prices: not supported by this source".into(),
        ))
    }

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

    /// The user's OWN marketplace offers, read via `user_trades`-scoped or
    /// `marketplace_listings?username=<self>` filtering. Requires the source's
    /// user credential. Default reports unsupported.
    async fn my_offers(&self, _username: String) -> Result<Vec<Offer>> {
        Err(ProviderError::Unsupported("own offers".to_string()))
    }

    /// The user's own trade history (last trades), source-scoped. Requires a
    /// user credential on the source. `force = true` BYPASSES the stale-first
    /// snapshot TTL and re-reads live (RA t_1bd5328b: the „Aktualisieren"
    /// button must actually reach UEX — deleted/added trades on the UEX side
    /// have to appear immediately). Default reports unsupported.
    async fn user_trades(&self, _force: bool) -> Result<Vec<Trade>> {
        Err(ProviderError::Unsupported("user trades".to_string()))
    }

    /// The user's MARKETPLACE deals (UEX `/marketplace_negotiations` —
    /// Advertisement-Verhandlungen/-Abschlüsse des Accounts, als Anbieter
    /// ODER Kunde). Das ist die korrekte Quelle für „Letzte 5 Trades" im
    /// Mein-Shop-Abschnitt (t_350c4300 B2) — `/user_trades` ist commodity-
    /// seitiger Cargo-Handel und gehört dort NICHT hin. User-scoped (secret
    /// key). Default reports unsupported.
    async fn marketplace_negotiations(&self) -> Result<Vec<Negotiation>> {
        Err(ProviderError::Unsupported(
            "marketplace negotiations".to_string(),
        ))
    }

    /// Chat-Verlauf einer MARKETPLACE-Verhandlung (M3b t_893761f1,
    /// `GET /marketplace_negotiations_messages`) — identifiziert über
    /// `hash` (bevorzugt) oder `id_negotiation`. Realtime-Endpoint ohne
    /// Cache-TTL (Research-Empfehlung: Polling max. 1/min im Idle).
    /// User-scoped (secret key). Default reports unsupported.
    async fn marketplace_negotiations_messages(
        &self,
        hash: &str,
        id_negotiation: u64,
    ) -> Result<Vec<NegotiationMessage>> {
        let _ = (hash, id_negotiation);
        Err(ProviderError::Unsupported(
            "marketplace negotiations messages".to_string(),
        ))
    }

    /// Chat-Nachricht senden (M3b, `POST /marketplace_negotiations_messages/`)
    /// mit Payload exakt nach Doku: `{id_negotiation, hash, message,
    /// is_production}`. KEIN Retry bei 429/403 — der Fehler steigt durch,
    /// die UI entscheidet Deep-Link. Liefert die neue Message-ID. Default
    /// reports unsupported.
    async fn send_negotiation_message(
        &self,
        id_negotiation: u64,
        hash: &str,
        message: &str,
    ) -> Result<u64> {
        let _ = (id_negotiation, hash, message);
        Err(ProviderError::Unsupported(
            "send negotiation message".to_string(),
        ))
    }

    /// Marketplace categories (for the advertise-form dropdown). UEX REQUIRES
    /// `id_category` for `type=item`, so the form lets the user pick a
    /// display-name category whose id goes into `AdvertisePayload.id_category`.
    /// Small reference list — the provider fetches/caches it like other
    /// reference data. Default reports unsupported.
    async fn categories(&self) -> Result<Vec<Category>> {
        Err(ProviderError::Unsupported("categories".to_string()))
    }

    /// Write-Layer (M2b): ein neues Marketplace-Listing anlegen via
    /// `POST /marketplace_advertise/`. Payload exakt nach Doku-Schema
    /// ([`AdvertisePayload`]); kein Edit — Bearbeiten = Abmelden + Neu anlegen.
    /// Der Provider validiert die Pflichtfelder und sendet KEINEN Retry bei
    /// 429/403 (Fehlerpfad: Meldung + Deep-Link, die UI entscheidet das
    /// Weiterleiten). Liefert die neue Listung als `id` + Permalink-`url`
    /// (UEX-Antwort ist NICHT die vollständige ListingRow — die erscheint erst
    /// nach Approval in `/marketplace_listings`). Default reports unsupported.
    async fn advertise_offer(&self, _payload: AdvertisePayload) -> Result<AdvertiseResult> {
        Err(ProviderError::Unsupported("advertise offer".to_string()))
    }

    /// Write-Layer (M2b): ein bestehendes Listing ABMELDEN via
    /// `DELETE /marketplace_listings/?id=…`. Nur der Secret-Key ist nötig
    /// (Research §1). Kein Retry-Spam bei 429/403. Default unsupported.
    async fn delete_offer(&self, _id: u64) -> Result<()> {
        Err(ProviderError::Unsupported("delete offer".to_string()))
    }

    /// Write-Layer (PT t_7fdc1317): einen Commodity-Trade ERFASSEN via
    /// `POST /user_trades_add/` (Doku §5a). Payload exakt nach Doku-Schema
    /// ([`TradePayload`], `is_production` setzt der Client konstant auf 1);
    /// KEIN Retry bei 429/403 — der Fehler steigt durch, die UI entscheidet
    /// Deep-Link. Liefert die neue Trade-ID (UEX-Antwort `{id_user_trade}`).
    /// Default reports unsupported.
    async fn add_trade(&self, _payload: TradePayload) -> Result<u64> {
        Err(ProviderError::Unsupported("add trade".to_string()))
    }

    /// Write-Layer (PT): einen Commodity-Trade BEARBEITEN via
    /// `POST /user_trades_edit/` (Doku §5c). Alle Pflichtfelder werden neu
    /// übergeben (die API kennt kein partielles Edit). Kein Retry bei
    /// 429/403. Liefert die bearbeitete Trade-ID. Default unsupported.
    async fn edit_trade(&self, _id: u64, _payload: TradePayload) -> Result<u64> {
        Err(ProviderError::Unsupported("edit trade".to_string()))
    }

    /// Write-Layer (PT): einen Commodity-Trade LÖSCHEN via
    /// `DELETE /user_trades_remove/?id=…` (Doku §5d). Nur der Secret-Key ist
    /// nötig. Kein Retry-Spam bei 429/403. Default unsupported.
    async fn remove_trade(&self, _id: u64) -> Result<()> {
        Err(ProviderError::Unsupported("remove trade".to_string()))
    }

    /// Handelsrouten (Karte t_128b7547): bester Einkauf → bester Verkauf je
    /// Ware, aus dem eigenen Preis-Datensatz. Der Default meldet "nicht
    /// unterstützt" — Implementierungen ohne Routen-Sweep fallen hier durch.
    async fn trade_routes(&self, filters: &routes::RouteFilters) -> Result<routes::RouteList> {
        let _ = filters;
        Err(ProviderError::Unsupported("trade routes".to_string()))
    }

    /// Alternativ-Tabelle (F2): alle Kauf-/Verkaufsorte einer Ware aus dem
    /// LOKALEN Routen-Datensatz (kein neuer UEX-Call — Datenbasis ist die
    /// bereits gesweepten Preiszeilen). Der Default meldet "nicht unterstützt";
    /// Implementierungen mit Routen-Sweep servieren die Tabelle aus den
    /// lokalen Zeilen.
    async fn route_locations(
        &self,
        commodity_id: u64,
    ) -> Result<Option<routes::RouteCommodityLocations>> {
        let _ = commodity_id;
        Err(ProviderError::Unsupported("route locations".to_string()))
    }

    /// Schiffs-SCU-Kapazitäten (name/name_full/slug → SCU, lowercase) für den
    /// Hangar-Filter des Routen-Views. Default: nicht unterstützt.
    async fn vehicles_scu(&self) -> Result<std::collections::HashMap<String, f64>> {
        Err(ProviderError::Unsupported("vehicle scu".to_string()))
    }

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
        // Locations search BOTH datasets: wiki locations first (richer
        // hierarchy), then the UEX terminal dataset (price aggregates open
        // through its terminals).
        assert_eq!(
            SearchEntities::Locations.kinds(),
            &[EntityKind::Locations, EntityKind::Terminals]
        );
        assert_eq!(SearchEntities::Terminals.kinds(), &[EntityKind::Terminals]);
        assert_eq!(
            SearchEntities::Commodities.kinds(),
            &[EntityKind::Commodities]
        );
        assert_eq!(SearchEntities::Items.kinds(), &[EntityKind::Items]);
    }

    #[test]
    fn apply_marketplace_filters_sorts_all_three_ways() {
        // P1 gate: the shared filter+sort helper must order price asc, price
        // desc and newest — on the SAME input set (all three filter modes
        // WTB/WTS/all run through this one function).
        let mk = |id: u64, price: f64, updated: u64, op: MarketOperation| Offer {
            id,
            title: format!("offer {id}"),
            description: String::new(),
            item_id: 0,
            category_id: 0,
            operation: op,
            listing_type: ListingType::Item,
            price,
            currency: "UEC".into(),
            unit: "unit".into(),
            quantity: 1,
            location: "Levski".into(),
            region: "Nyx".into(),
            seller: "s".into(),
            quality: 0,
            updated,
            expires: None,
            slug: "slug".into(),
            photos: Vec::new(),
            approved: true,
        };
        let offers = vec![
            mk(1, 300.0, 100, MarketOperation::Sell),
            mk(2, 100.0, 300, MarketOperation::Buy),
            mk(3, 200.0, 200, MarketOperation::Sell),
        ];
        let ids = |o: &[Offer]| -> Vec<u64> { o.iter().map(|o| o.id).collect() };

        let f = |q: &str, sort: Option<OfferSort>| MarketplaceFilter {
            q: q.into(),
            sort,
            ..Default::default()
        };

        // price asc / desc across the whole set
        assert_eq!(
            ids(&apply_marketplace_filters(
                offers.clone(),
                &f("", Some(OfferSort::PriceAsc))
            )),
            vec![2, 3, 1]
        );
        assert_eq!(
            ids(&apply_marketplace_filters(
                offers.clone(),
                &f("", Some(OfferSort::PriceDesc))
            )),
            vec![1, 3, 2]
        );
        // newest, tie-break by id
        assert_eq!(
            ids(&apply_marketplace_filters(
                offers.clone(),
                &f("", Some(OfferSort::Newest))
            )),
            vec![2, 3, 1]
        );
        // q terms AND-ed over title/operation; operation term narrows to buys
        assert_eq!(
            ids(&apply_marketplace_filters(offers.clone(), &f("buy", None))),
            vec![2]
        );
        // a term hitting nothing empties the list (AND semantics)
        assert!(apply_marketplace_filters(offers.clone(), &f("zzz", None)).is_empty());
        // region filter (system filter is UI-removed but the field stays honest)
        let region = MarketplaceFilter {
            region: "nyx".into(),
            ..Default::default()
        };
        assert_eq!(apply_marketplace_filters(offers, &region).len(), 3);
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
            force_refresh: false,
        };
        assert!(
            serde_json::from_str::<MarketplaceFilter>(&serde_json::to_string(&filter).unwrap())
                .is_ok()
        );
    }
}
