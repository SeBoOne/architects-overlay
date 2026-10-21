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
