//! # Composite data provider
//!
//! PS (t_c84e545d): the composite is UEX-ONLY. The wiki source is gone —
//! it never had prices of its own (it served UEX data itself), so the
//! UEX/WIKI choice was meaningless and has been removed together with the
//! `SourceSwitch` / Settings A/B and the wiki sync task.
//!
//! Every operation delegates to the UEX provider behind the same
//! [`DataProvider`] seam; Locations search uses the SYNTHESIZED UEX
//! location dataset (one entity per unique terminal chain — see
//! `uex_client::location_entities_from_rows`), so no functionality is lost
//! for search or location details.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use cache::Cache;
use provider::{
    DataProvider, Entity, EntityKind, LocationPrices, MarketplaceFilter, PricePoint, ProviderError,
    Result, SearchEntities, SearchHit, routes::RouteList,
};
use uex_client::UexProvider;

/// Search result page size served from the cache (mirrors the UEX provider).
const SEARCH_LIMIT: u32 = 25;

/// Composite provider: one source (UEX), one dataset per kind.
pub struct CompositeProvider {
    // Arc'd: the Stale-While-Revalidate background revalidation (K1
    // Cache-Architektur) captures this handle to schedule the Sweep
    // outside the request path.
    uex: Arc<UexProvider>,
    cache: Arc<Cache>,
    /// Guard against stacking Sweeps: set while a background
    /// revalidation is running (at most one per TTL window).
    route_refresh_in_flight: Arc<AtomicBool>,
}

impl CompositeProvider {
    pub fn new(uex: UexProvider, cache: Arc<Cache>) -> Self {
        CompositeProvider {
            uex: Arc::new(uex),
            cache,
            route_refresh_in_flight: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The wrapped UEX provider (background price-sync sweeps, t_c84e545d).
    pub fn uex(&self) -> &Arc<UexProvider> {
        &self.uex
    }
}

#[async_trait::async_trait]
impl DataProvider for CompositeProvider {
    async fn search(&self, entities: SearchEntities, query: &str) -> Result<Vec<SearchHit>> {
        // Locations searches BOTH datasets (synthesized UEX locations + UEX
        // terminals). UEX terminals are ensured via the uex provider's own
        // search path — queried through the cache directly so BOTH kinds'
        // rows come back (the uex provider filters to its own rows by
        // design).
        if entities == SearchEntities::Locations {
            // Ensure the UEX terminal dataset is fresh (reuse the provider's
            // ensure logic via a throwaway search; the result is discarded —
            // it also seeds the synthesized location dataset on import).
            let _ = self.uex.search(SearchEntities::Terminals, "").await?;
            let hits = self
                .cache
                .search_sourced(
                    &[EntityKind::Locations, EntityKind::Terminals],
                    &[],
                    query,
                    // P1: Location-ENTITY nur per Name (Subtitle matcht nicht).
                    &[EntityKind::Locations],
                    SEARCH_LIMIT,
                )
                .map_err(|e| ProviderError::Cache(e.to_string()))?;
            return Ok(hits);
        }
        self.uex.search(entities, query).await
    }

    /// Commodity price points for one commodity (cached ≤ 1 h, renewed by
    /// the hourly background price sync — PS t_c84e545d).
    async fn prices(&self, commodity: &str) -> Result<Vec<PricePoint>> {
        self.uex.prices(commodity).await
    }

    /// PS (t_c84e545d): force ONE live refresh of this commodity's price
    /// table (detail F5 / settings "Jetzt aktualisieren").
    async fn refresh_prices(&self, commodity: &str) -> Result<Vec<PricePoint>> {
        self.uex.refresh_prices(commodity).await
    }

    /// PS (t_c84e545d): force ONE live refresh of one item's price table
    /// (item-detail F5).
    async fn refresh_item_prices(&self, item_id: u64) -> Result<Vec<PricePoint>> {
        self.uex.refresh_item_prices(item_id).await
    }

    async fn location_prices(&self, terminal_id: u64) -> Result<LocationPrices> {
        // Location ids are UEX terminal ids now (synthesized dataset) — the
        // aggregate opens directly, no id resolution anymore.
        self.uex.location_prices(terminal_id).await
    }

    async fn terminal_prices(&self, terminal_id: u64) -> Result<Vec<PricePoint>> {
        self.uex.terminal_prices(terminal_id).await
    }

    async fn item_prices(&self, item_id: u64) -> Result<Vec<PricePoint>> {
        // Item prices carry the LONG patch-driven TTL (7 days) — renewal only
        // through the explicit refresh paths (PS t_c84e545d).
        self.uex.item_prices(item_id).await
    }

    async fn marketplace_offers(&self, filters: MarketplaceFilter) -> Result<Vec<provider::Offer>> {
        self.uex.marketplace_offers(filters).await
    }

    async fn my_offers(&self, username: String) -> Result<Vec<provider::Offer>> {
        // Eigene Offers sind UEX-user-scoped (secret-key).
        self.uex.my_offers(username).await
    }

    async fn user_trades(&self) -> Result<Vec<provider::Trade>> {
        // Rede-Verlauf ist UEX-user-scoped (secret-key).
        self.uex.user_trades().await
    }

    async fn marketplace_negotiations(&self) -> Result<Vec<provider::Negotiation>> {
        // Marktplatz-Deals (Mein Shop, „Letzte 5 Trades“) sind UEX-user-scoped
        // (secret-key). Delegation MUSS explizit sein: der Trait-Default
        // meldet „nicht unterstützt" und bricht den Mein-Shop-Abschnitt
        // (t_1af18a08).
        self.uex.marketplace_negotiations().await
    }

    async fn marketplace_negotiations_messages(
        &self,
        hash: &str,
        id_negotiation: u64,
    ) -> Result<Vec<provider::NegotiationMessage>> {
        // Deal-Chat (M3b t_893761f1) — dieselbe Delegierungs-Regel wie
        // `marketplace_negotiations`.
        self.uex
            .marketplace_negotiations_messages(hash, id_negotiation)
            .await
    }

    async fn send_negotiation_message(
        &self,
        id_negotiation: u64,
        hash: &str,
        message: &str,
    ) -> Result<u64> {
        self.uex
            .send_negotiation_message(id_negotiation, hash, message)
            .await
    }

    async fn categories(&self) -> Result<Vec<provider::Category>> {
        // Marketplace-Kategorien (Anlegen-Form) sind UEX-Referenzdaten.
        self.uex.categories().await
    }

    async fn advertise_offer(
        &self,
        payload: provider::AdvertisePayload,
    ) -> Result<provider::AdvertiseResult> {
        // Write-Layer (M2b): Anlegen ist UEX-user-scoped (secret-key).
        self.uex.advertise_offer(payload).await
    }

    async fn delete_offer(&self, id: u64) -> Result<()> {
        // Write-Layer (M2b): Abmelden ist UEX-user-scoped (secret-key).
        self.uex.delete_offer(id).await
    }

    async fn add_trade(&self, payload: provider::TradePayload) -> Result<u64> {
        // Trade-Log-Write-Layer (PT t_7fdc1317, QY t_07edb03d): Erfassen ist
        // UEX-user-scoped (secret-key). Delegation MUSS explizit sein — der
        // Trait-Default meldet „Nicht unterstützt: add trade" (Bug-Cluster
        // t_1af18a08-Klasse).
        self.uex.add_trade(payload).await
    }

    async fn edit_trade(&self, id: u64, payload: provider::TradePayload) -> Result<u64> {
        // dito: Bearbeiten (/user_trades_edit/).
        self.uex.edit_trade(id, payload).await
    }

    async fn remove_trade(&self, id: u64) -> Result<()> {
        // dito: Löschen (/user_trades_remove/).
        self.uex.remove_trade(id).await
    }

    async fn trade_routes(&self, filters: &provider::routes::RouteFilters) -> Result<RouteList> {
        // Handelsrouten bleiben bewusst UEX-EXKLUSIV (Karte t_128b7547).
        //
        // Stale-While-Revalidate (K1 Cache-Architektur): if the Sweep marker
        // expired but the cache can serve old data, the Sweep is scheduled
        // HERE — on a background task — and the view opens immediately from
        // the persisted Profit-Matrix. The API load is UNCHANGED (one Sweep
        // per TTL window, marker-guarded); only the FOREGROUND never waits
        // for a Sweep again.
        let schedule = self.uex.route_sweep_stale().unwrap_or(false)
            && self.uex.route_inputs_present().unwrap_or(false);
        if schedule && !self.route_refresh_in_flight.swap(true, Ordering::SeqCst) {
            let uex = self.uex.clone();
            let flag = self.route_refresh_in_flight.clone();
            _ = tokio::spawn(async move {
                match uex.revalidate_route_data().await {
                    Ok(n) => log::info!("routes revalidate: sweeped {n} terminals"),
                    Err(e) => log::warn!("routes revalidate failed: {e}"),
                }
                flag.store(false, Ordering::SeqCst);
            });
        }
        self.uex.trade_routes(filters).await
    }

    async fn route_locations(
        &self,
        commodity_id: u64,
    ) -> Result<Option<provider::routes::RouteCommodityLocations>> {
        self.uex.route_locations(commodity_id).await
    }

    async fn vehicles_scu(&self) -> Result<std::collections::HashMap<String, f64>> {
        self.uex.vehicles_scu().await
    }

    async fn fetch_entities(&self, kind: EntityKind) -> Result<Vec<Entity>> {
        // All datasets are UEX now — including Locations, which the uex
        // client synthesizes from the terminal rows.
        self.uex.fetch_entities(kind).await
    }

    fn name(&self) -> String {
        String::from("uex")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn wired() -> (Arc<Cache>, CompositeProvider) {
        let cache = Arc::new(Cache::open_in_memory().unwrap());
        let uex =
            UexProvider::with_base_url(cache.clone(), None, "http://127.0.0.1:1", Duration::ZERO)
                .unwrap();
        (cache.clone(), CompositeProvider::new(uex, cache))
    }

    #[test]
    fn composite_is_uex_only() {
        let (_, provider) = wired();
        assert_eq!(provider.name(), "uex");
    }

    /// Regression t_1af18a08: `CompositeProvider` MUSS
    /// `marketplace_negotiations` explizit an den UexProvider delegieren —
    /// der Trait-Default meldet „nicht unterstützt" und bricht den
    /// Mein-Shop-Abschnitt. Offline/ohne Secret-Key muss der Fehler ein
    /// Netz-/Config-Fehler sein, NIEMALS `Unsupported`.
    #[tokio::test]
    async fn composite_delegates_marketplace_negotiations_to_uex() {
        let (_, provider) = wired();
        let err = provider.marketplace_negotiations().await.unwrap_err();
        assert!(
            !matches!(err, ProviderError::Unsupported(_)),
            "Composite fällt in den Trait-Default durch: {err:?}"
        );
    }

    /// QY (t_07edb03d): dieselbe Delegierungs-Regel für den Trade-Log-Write-
    /// Layer (add/edit/remove via /user_trades_add|edit|remove). Ohne
    /// explizite Delegation fällt der Composite in den Trait-Default und das
    /// Formular meldet „Nicht unterstützt: add trade".
    #[tokio::test]
    async fn composite_delegates_trade_writes_to_uex() {
        let (_, provider) = wired();
        let payload = provider::TradePayload {
            id_terminal: 1,
            id_commodity: 1,
            operation: "buy".to_string(),
            scu: 1,
            price: 1.0,
        };
        let err = provider.add_trade(payload.clone()).await.unwrap_err();
        assert!(
            !matches!(err, ProviderError::Unsupported(_)),
            "Composite fällt in den Trait-Default durch: {err:?}"
        );
        let err2 = provider.edit_trade(1, payload).await.unwrap_err();
        assert!(
            !matches!(err2, ProviderError::Unsupported(_)),
            "Composite fällt in den Trait-Default durch: {err2:?}"
        );
        let err3 = provider.remove_trade(1).await.unwrap_err();
        assert!(
            !matches!(err3, ProviderError::Unsupported(_)),
            "Composite fällt in den Trait-Default durch: {err3:?}"
        );
    }

    /// M3b (t_893761f1): dieselbe Delegierungs-Regel für den Deal-Chat.
    #[tokio::test]
    async fn composite_delegates_negotiation_messages_to_uex() {
        let (_, provider) = wired();
        let err = provider
            .marketplace_negotiations_messages("h", 1)
            .await
            .unwrap_err();
        assert!(
            !matches!(err, ProviderError::Unsupported(_)),
            "Composite fällt in den Trait-Default durch: {err:?}"
        );
        let err2 = provider
            .send_negotiation_message(1, "h", "x")
            .await
            .unwrap_err();
        assert!(
            !matches!(err2, ProviderError::Unsupported(_)),
            "Composite fällt in den Trait-Default durch: {err2:?}"
        );
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use std::time::Duration;

    /// LIVE-Smoke (Feedback-Runde 4, t_7a1add66) — echte API, echter Cache.
    /// Läuft nur mit `ARCH_SMOKE_CACHE=<pfad>` gegen eine KOPIE der App-Cache-DB
    /// (`cargo test -p architects-overlay --lib live_smoke -- --ignored
    /// --nocapture`), damit die laufende App unberührt bleibt.
    #[tokio::test]
    #[ignore = "live: needs ARCH_SMOKE_CACHE=<copy of app cache.db> + network"]
    async fn live_smoke_search_semantics() {
        let Some(path) = std::env::var_os("ARCH_SMOKE_CACHE") else {
            panic!("ARCH_SMOKE_CACHE not set — this live test is opt-in");
        };
        let cache = Arc::new(Cache::open(std::path::Path::new(&path)).unwrap());
        let uex = UexProvider::with_base_url(
            cache.clone(),
            None,
            uex_client::private::DEFAULT_BASE_URL,
            Duration::ZERO,
        )
        .unwrap();
        let provider = CompositeProvider::new(uex, cache.clone());
        println!(
            "terminals in cache: {}",
            cache.count_entities(EntityKind::Terminals).unwrap()
        );

        for q in ["levski", "checkmate"] {
            let hits = provider.search(SearchEntities::Locations, q).await.unwrap();
            println!("\n:l {q} → {} Treffer", hits.len());
            for h in &hits {
                println!("  [{}] {} · {}", h.kind, h.name, h.subtitle);
            }
            let mercy = hits.iter().any(|h| h.name.contains("Mercy"));
            let services = hits.iter().any(|h| {
                h.kind == "terminal"
                    && (h.name.contains("Landing Services")
                        || h.name.contains("Refinement Center")
                        || h.name.contains("Rentals"))
            });
            assert!(
                !mercy,
                "P1: Mercy Hospital darf bei '{q}' nicht mehr auftauchen"
            );
            assert!(
                !services,
                "P2: Service-Terminals (Landing/Refinement/Rentals) sind gefiltert"
            );
            let shops = hits.iter().filter(|h| h.kind == "terminal").count();
            println!("  Terminal-Zeilen (Station-Shops): {shops}");
            assert!(shops > 0, "P1.2: Station-Shops müssen gelistet sein");
        }

        // P1.3: Levski-Aggregat (Location-Detail = alle Kaufterminals).
        let hits = provider
            .search(SearchEntities::Locations, "levski")
            .await
            .unwrap();
        let loc = hits
            .iter()
            .find(|h| h.kind == "location" && h.name.eq_ignore_ascii_case("Levski"))
            .expect("Levski location hit")
            .clone();
        let agg = provider.location_prices(loc.id).await.unwrap();
        let rows: usize = agg.groups.iter().map(|g| g.prices.len()).sum();
        println!(
            "Levski-Aggregat: {} Terminal-Gruppen, {rows} Preiszeilen (erwarte 8+ Gruppen)",
            agg.groups.len()
        );
        assert!(
            agg.groups.len() >= 8,
            "Levski-Referenzfall: 8+ Kaufterminals"
        );

        // `:c laranite` — Baseline 26 Zeilen.
        let rows = provider.prices("laranite").await.unwrap();
        println!("\n:c laranite → {} Preiszeilen (Baseline: 26)", rows.len());
        assert_eq!(rows.len(), 26, "Laranite-Baseline unverändert");

        use provider::OfferSort;
        // P3: Market hybrid — komplettes Offer-Fenster für 'parallax'.
        let offers = provider
            .marketplace_offers(MarketplaceFilter {
                q: "parallax".into(),
                sort: Some(OfferSort::PriceAsc),
                limit: 1000,
                ..Default::default()
            })
            .await
            .unwrap();
        let q900: Vec<&provider::Offer> = offers.iter().filter(|o| o.quality >= 900).collect();
        println!(
            "\nMarket 'parallax': {} Offers, {} mit Q>=900 (erwarte >=15), price_asc={:?}",
            offers.len(),
            q900.len(),
            q900.iter().take(5).map(|o| o.price).collect::<Vec<_>>()
        );
        assert!(
            q900.len() >= 15,
            "P3: 'Parallax + Min-Q 900' braucht >=15 Offers (got {})",
            q900.len()
        );
        assert!(
            offers.windows(2).all(|w| w[0].price <= w[1].price),
            "price_asc sortiert"
        );
    }
}
