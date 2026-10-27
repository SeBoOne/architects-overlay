//! Live integration test against the real UEX API (read endpoints need no
//! auth). Ignored by default (`cargo test -- --ignored tests::live_*`) so the
//! suite stays offline-friendly. Run manually to verify against the live
//! source.

use std::sync::Arc;
use std::time::Duration;

use cache::Cache;
use provider::{DataProvider, EntityKind, SearchEntities};
use uex_client::UexProvider;

async fn live_provider() -> UexProvider {
    let cache = Arc::new(Cache::open_in_memory().unwrap());
    // 300ms pacing keeps us well under the ~120 req/min daily quota.
    UexProvider::with_base_url(
        cache,
        None,
        "https://api.uexcorp.uk/2.0",
        Duration::from_millis(300),
    )
    .unwrap()
}

#[tokio::test]
#[ignore = "network test — run with -- --ignored"]
async fn live_reference_data_fetches() {
    let p = live_provider().await;

    let commodities = p.fetch_entities(EntityKind::Commodities).await.unwrap();
    assert!(
        commodities.len() > 150,
        "expected 200+ commodities, got {}",
        commodities.len()
    );
    assert!(commodities.iter().any(|e| e.name == "Agricium"));

    let terminals = p.fetch_entities(EntityKind::Terminals).await.unwrap();
    assert!(
        terminals.len() > 400,
        "expected 500+ terminals, got {}",
        terminals.len()
    );

    let items = p.fetch_entities(EntityKind::Items).await.unwrap();
    assert!(
        items.len() > 500,
        "expected item catalog across categories, got {}",
        items.len()
    );
}

#[tokio::test]
#[ignore = "network test — run with -- --ignored"]
async fn live_search_and_prices() {
    let p = live_provider().await;

    let hits = p
        .search(SearchEntities::Commodities, "agricium")
        .await
        .unwrap();
    assert!(
        hits.iter().any(|h| h.name == "Agricium"),
        "expected Agricium in results, got {:?}",
        hits
    );
    let agri = hits.iter().find(|h| h.name == "Agricium").unwrap();
    assert_eq!(agri.code.as_deref(), Some("AGRI"));

    let prices = p.prices("agricium").await.unwrap();
    assert!(
        prices.len() >= 10,
        "expected many terminal price points, got {}",
        prices.len()
    );
    assert!(prices.iter().all(|pt| pt.commodity_id == 1));

    let terminals = p
        .search(SearchEntities::Locations, "area 18")
        .await
        .unwrap();
    assert!(!terminals.is_empty(), "expected Area 18 terminals");
}

#[tokio::test]
#[ignore = "network test — run with -- --ignored"]
async fn live_location_aggregate_checkmate() {
    let p = live_provider().await;

    // Live-feedback case: open the Checkmate commodity terminal and expect
    // the aggregate of ALL four Checkmate terminals (not "Preise nicht
    // geladen"), grouped per terminal, with real prices on 436.
    let agg = p.location_prices(436).await.unwrap();
    assert!(
        agg.location.contains("Checkmate"),
        "location header must carry the chain: {}",
        agg.location
    );
    let ids: Vec<u64> = agg.groups.iter().map(|g| g.terminal_id).collect();
    assert!(
        ids.contains(&436),
        "clicked terminal must be part of the aggregate: {ids:?}"
    );
    let commodity = agg.groups.iter().find(|g| g.terminal_id == 436).unwrap();
    assert!(
        commodity.prices.len() >= 5,
        "commodity terminal must carry its price table ({} rows)",
        commodity.prices.len()
    );
    for g in &agg.groups {
        assert!(
            !g.terminal_name.is_empty(),
            "every group needs a display name"
        );
    }
    eprintln!(
        "CHECKMATE: location={} groups={} prices@436={}",
        agg.location,
        agg.groups.len(),
        commodity.prices.len()
    );

    // Same terminal search now shows full names (no ID-raten).
    let hits = p
        .search(SearchEntities::Locations, "checkmate")
        .await
        .unwrap();
    assert!(
        hits.iter().any(|h| h.name.contains("Checkmate")),
        "terminal hits must carry full names: {hits:?}"
    );
    assert!(
        hits.iter()
            .find(|h| h.id == 436)
            .map(|h| h.name == "Commodity Shop - Admin - Checkmate")
            .unwrap_or(false),
        "commodity terminal must show its full shop name"
    );
}

#[tokio::test]
#[ignore = "network test — run with -- --ignored"]
async fn live_terminal_prices_are_row_specific() {
    // Location-Expansion-Fix (t_d87ca3a4): JEDES Terminal liefert EIGENE
    // Preise ueber die neue DataProvider::terminal_prices(terminal_id) —
    // Landing Services (428) != Guns (437) != Commodity (436). Das
    // Trait-Kontrakt traegt id in den Provider; der Cache ist 30min-TTL.
    let p = live_provider().await;
    let landing = p.terminal_prices(428).await.unwrap();
    let guns = p.terminal_prices(437).await.unwrap();
    let admin = p.terminal_prices(436).await.unwrap();

    // Zwei verschiedene Lokations-Terminals: nicht identische Warenlisten.
    // (Exakter Inhalt haengt von UEX ab — die entscheidende Assertion ist,
    // dass der Provider pro id eigene Zeilen zurueckgibt und kein Fehler
    // auftritt. Ein leeres Ergebnis ist GUELTIG fuer Nicht-Handels-Terminals
    // (Fuel-/Landing-/Refinery-Docks), nicht Unsupported.)
    assert!(
        admin.len() >= 5,
        "admin commodity terminal must carry its price table ({} rows)",
        admin.len()
    );
    eprintln!(
        "LANDING@428={} GUNS@437={} ADMIN@436={}",
        landing.len(),
        guns.len(),
        admin.len()
    );
    // Wenn ein Terminal Daten traegt, gehoeren sie zu seiner id — die drei
    // ids muessen ohne Fehler durchgehen (kein Unsupported-Fallback).
    for (tid, rows) in [(428u64, &landing), (437, &guns), (436, &admin)] {
        if let Some(first) = rows.first() {
            assert_eq!(first.terminal_id, tid);
        }
    }
}

#[tokio::test]
#[ignore = "network test — run with -- --ignored"]
async fn live_item_without_prices_and_dedupe() {
    let p = live_provider().await;

    // Dedup at import: commodities (is_commodity=1) must never appear as
    // items in the cached dataset. Commodity ids are a small range (1..205)
    // and DO overlap ordinary item ids — the flag, not the id, is the
    // criterion (research Q4/Q1).
    let items = p.fetch_entities(EntityKind::Items).await.unwrap();
    let _ = p.fetch_entities(EntityKind::Commodities).await.unwrap();
    let quant = items.iter().find(|e| e.name == "Quantainium");
    assert!(
        quant.is_none(),
        "Quantainium (is_commodity=1) must be deduped from items"
    );
    assert!(
        items.len() > 500,
        "item catalog must survive the filter, got {}",
        items.len()
    );

    // An armor item has no commodity price table → empty (not an error), so
    // the UI can render "nicht kaufbar / keine Preisdaten".
    let prices = p.prices("CSP-68H Backpack").await.unwrap();
    assert!(
        prices.is_empty(),
        "item detail must be empty, got {prices:?}"
    );
}

#[tokio::test]
#[ignore = "network test — run with -- --ignored"]
async fn live_marketplace_filter_and_sort() {
    use provider::{MarketplaceFilter, OfferSort};

    let p = live_provider().await;

    // Region filter: restricting to Nyx must return only Nyx listings.
    let nyx = p
        .marketplace_offers(MarketplaceFilter {
            region: "Nyx".into(),
            sort: Some(OfferSort::PriceAsc),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(
        !nyx.is_empty(),
        "expected active Nyx listings, got {}",
        nyx.len()
    );
    assert!(
        nyx.iter().all(|o| o.region == "Nyx"),
        "region filter leaked: {:?}",
        nyx.iter().map(|o| &o.region).collect::<Vec<_>>()
    );
    // Ascending sort must be monotone for the returned page.
    let mut sorted = nyx
        .iter()
        .map(|o| o.price)
        .filter(|p| *p > 0.0)
        .collect::<Vec<_>>();
    let orig = sorted.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    assert_eq!(orig, sorted, "price ascending sort failed");

    // Every live listing must carry a slug (deep-link target).
    assert!(nyx.iter().all(|o| !o.slug.is_empty()));
    // And a region resolve (star-systems mapping worked).
    assert!(nyx.iter().all(|o| !o.region.is_empty()));

    // Free-text q matches title/description/seller.
    let first = &nyx[0].title.split_whitespace().next().unwrap_or("x");
    let q = first.chars().take(4).collect::<String>();
    let filtered = p
        .marketplace_offers(MarketplaceFilter {
            q,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(!filtered.is_empty());
}

#[tokio::test]
#[ignore = "network test — run with -- --ignored"]
async fn live_market_wts_term_sorts_complete_item_set() {
    // P1 acceptance probe (live): WTS + "Parallax" + price-desc must return
    // ONLY WTS Parallax offers, sorted by price descending. The term resolves
    // to item ids and the API filters server-side (id_item + operation),
    // so the sort acts on the complete per-item set — not a newest window.
    use provider::{MarketOperation, MarketplaceFilter, OfferSort};

    let p = live_provider().await;

    let offers = p
        .marketplace_offers(MarketplaceFilter {
            q: "Parallax".into(),
            operation: Some(MarketOperation::Sell),
            sort: Some(OfferSort::PriceDesc),
            ..Default::default()
        })
        .await
        .unwrap();

    assert!(
        !offers.is_empty(),
        "expected live WTS Parallax offers, got 0"
    );
    assert!(
        offers.iter().all(|o| o.operation == MarketOperation::Sell),
        "WTS filter leaked: {:?}",
        offers.iter().map(|o| &o.operation).collect::<Vec<_>>()
    );
    assert!(
        offers
            .iter()
            .all(|o| o.title.to_lowercase().contains("parallax")
                || o.description.to_lowercase().contains("parallax")),
        "term filter leaked: {:?}",
        offers
            .iter()
            .map(|o| o.title.chars().take(50).collect::<String>())
            .collect::<Vec<_>>()
    );
    let prices: Vec<f64> = offers.iter().map(|o| o.price).collect();
    let mut sorted = prices.clone();
    sorted.sort_by(|a, b| b.total_cmp(a));
    assert_eq!(prices, sorted, "price descending sort failed");
    // request pacing: at most 3 item-id fetches + this call's payload
    eprintln!(
        "live WTS Parallax offers: {} (first price {})",
        offers.len(),
        prices[0]
    );
}

/// t_65a91d5b C3b: LIVE-Beweis des Enrichments gegen den echten UEX-API —
/// seedet eine kleine Routen-Matrix (2 reale Dreifach-Routen), ruft
/// `refresh_route_enrichment()` gegen api.uexcorp.uk auf und prüft, dass der
/// `/commodities_routes`-Response (PLURAL!) an der exakten Route hängt
/// (Volumen/Status/Score/Fraktion/Container). Schreib den VOLLEN Diff als
/// JSON raus (vorher/nachher) für den Task-Beleg.
#[tokio::test]
#[ignore = "network test — run with -- --ignored --nocapture"]
async fn live_route_enrichment_before_after() {
    use provider::routes::{RouteEnrichment, TradeRoute};

    fn matrix_route(id: u64, name: &str, dest: u64) -> TradeRoute {
        TradeRoute {
            commodity_id: id,
            commodity: name.into(),
            illegal: false,
            buy_terminal_id: 1, // ARC-L1 (echtes UEX-Terminal)
            buy_terminal: "Admin - ARC-L1".into(),
            buy_chain: "Stanton > ArcCorp > ArcCorp Lagrange Point 1".into(),
            buy_price: 7400.0,
            buy_terminal_autoload: false,
            buy_terminal_cargo_deck: false,
            buy_terminal_ground: false,
            sell_terminal_id: dest,
            sell_terminal: format!("Dest {dest}").into(),
            sell_chain: "Stanton > Dest".into(),
            sell_price: 9000.0,
            sell_terminal_autoload: false,
            sell_terminal_cargo_deck: false,
            sell_terminal_ground: true,
            sell_stock: 0.0,
            profit_per_scu: 1600.0,
            profit_total: 160000.0,
            cost_total: 740000.0,
            margin: 0.21,
            distance: 3,
            enrichment: None,
        }
    }

    fn display(r: &TradeRoute) -> String {
        match &r.enrichment {
            Some(e) => serde_json::to_string(e).unwrap(),
            None => "null".into(),
        }
    }

    let cache = Arc::new(Cache::open_in_memory().unwrap());
    // Zwei ECHTE Dreifach-Routen (je Commodity — Matrix-PK ist id_commodity):
    // Laranite 47 → Term 12 und Waste 79 → Term 274 (Sample-A live verifiziert).
    let matrix = vec![
        matrix_route(47, "Laranite", 12),
        matrix_route(79, "Waste", 274),
    ];
    cache.store_route_matrix(&matrix).unwrap();

    let p = UexProvider::with_base_url(
        cache.clone(),
        None,
        "https://api.uexcorp.uk/2.0",
        Duration::from_millis(300),
    )
    .unwrap();

    eprintln!("\n=== C3b LIVE Enrichment: VORHER (nur K1-Matrix, enrichment null) ===");
    for r in &matrix {
        eprintln!(
            "{} (c{}) → dest {}: profit {:.0}",
            r.commodity, r.commodity_id, r.sell_terminal_id, r.profit_per_scu
        );
        eprintln!("  enrichment = {}", display(&r));
    }

    let ok = p.refresh_route_enrichment().await.expect("live enrichment");
    eprintln!(
        "\n=== refresh_route_enrichment → {} Routen enriched ===",
        ok
    );
    assert!(
        ok >= 2,
        "erwarte enrichment für beide gesetzten Routen, got {ok}"
    );

    let rows = cache.route_enrichment_rows().unwrap();
    eprintln!("\n=== NACHHER (Enrichment-Cache, JSON) ===");
    for key in ["47:1:12", "79:1:274"] {
        let key = key.to_string();
        let e: RouteEnrichment = rows.get(&key).cloned().expect("route {key} enrich'd");
        eprintln!("{key}: {}", serde_json::to_string(&e).unwrap());
        assert!(e.scu_reachable > 0.0, "Volumen muss gesetzt sein: {key}");
        assert_eq!(
            e.game_version_origin.as_deref().is_some(),
            true,
            "game_version origin"
        );
        assert!(
            e.scu_origin > 0.0 || e.scu_destination > 0.0,
            "SCU-Volumen belegt: {key}"
        );
    }
}
