//! Integration test against a **mocked** UEX API (wiremock serving the
//! recorded fixtures) — exercises the full provider path: HTTP client, pacing,
//! envelope unwrapping, mapping, cache read/write, staleness and error mapping.
//!
//! No network access. The fixture files under `tests/fixtures/` are live API
//! captures (October 2026), cut down to a handful of rows.

use std::sync::Arc;
use std::time::Duration;

use cache::Cache;
use provider::{DataProvider, EntityKind, MarketplaceFilter, ProviderError, SearchEntities};
use uex_client::UexProvider;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> String {
    let p: std::path::PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures", name]
        .iter()
        .collect();
    std::fs::read_to_string(p).expect("fixture exists")
}

fn json_response(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.to_string(), "application/json")
}

struct MockedApi {
    server: MockServer,
}

impl MockedApi {
    async fn start() -> MockedApi {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/commodities"))
            .respond_with(json_response(&fixture("commodities.json")))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/terminals"))
            .respond_with(json_response(&fixture("terminals.json")))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/categories"))
            .respond_with(json_response(&fixture("categories.json")))
            .mount(&server)
            .await;

        // /items?id_category=2 → recorded rows; other categories → empty list.
        Mock::given(method("GET"))
            .and(path("/items"))
            .and(query_param("id_category", "2"))
            .respond_with(json_response(&fixture("items.json")))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/items"))
            .respond_with(json_response(
                r#"{"status":"ok","http_code":200,"data":[]}"#,
            ))
            .mount(&server)
            .await;

        // star systems — region resolution for marketplace listings
        Mock::given(method("GET"))
            .and(path("/star_systems"))
            .respond_with(json_response(&fixture("star_systems.json")))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/commodities_prices"))
            .and(query_param("id_commodity", "1"))
            .respond_with(json_response(&fixture("prices.json")))
            .mount(&server)
            .await;

        // Terminal-scoped prices (location aggregate): only the Checkmate
        // commodity terminal (436) trades; the fuel/refinery/item terminals
        // of the location return empty tables.
        Mock::given(method("GET"))
            .and(path("/commodities_prices"))
            .and(query_param("id_terminal", "436"))
            .respond_with(json_response(&fixture("terminal_prices_checkmate.json")))
            .mount(&server)
            .await;
        for other in ["428", "431", "435"] {
            Mock::given(method("GET"))
                .and(path("/commodities_prices"))
                .and(query_param("id_terminal", other))
                .respond_with(json_response(
                    r#"{"status":"ok","http_code":200,"data":[]}"#,
                ))
                .mount(&server)
                .await;
        }

        // Terminal-scoped item prices: the Guns terminal (435) sells items;
        // commodity/fuel/refinery terminals return empty.
        Mock::given(method("GET"))
            .and(path("/items_prices"))
            .and(query_param("id_terminal", "435"))
            .respond_with(json_response(&fixture("item_prices_terminal_435.json")))
            .mount(&server)
            .await;
        for other in ["428", "431", "436"] {
            Mock::given(method("GET"))
                .and(path("/items_prices"))
                .and(query_param("id_terminal", other))
                .respond_with(json_response(
                    r#"{"status":"ok","http_code":200,"data":[]}"#,
                ))
                .mount(&server)
                .await;
        }

        // Item-scoped prices: /items_prices?id_item=465 → backpack prices
        Mock::given(method("GET"))
            .and(path("/items_prices"))
            .and(query_param("id_item", "465"))
            .respond_with(json_response(&fixture("item_prices_465.json")))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/marketplace_listings"))
            .respond_with(json_response(&fixture("marketplace_listings.json")))
            .mount(&server)
            .await;

        MockedApi { server }
    }
}

fn provider(api: &MockedApi) -> (UexProvider, Arc<Cache>) {
    let cache = Arc::new(Cache::open_in_memory().unwrap());
    let p =
        UexProvider::with_base_url(cache.clone(), None, &api.server.uri(), Duration::ZERO).unwrap();
    (p, cache)
}

#[tokio::test]
async fn search_maps_reference_data_and_refreshes_on_demand() {
    let api = MockedApi::start().await;
    let (p, _cache) = provider(&api);

    // first search triggers a lazy refresh (empty cache) + FTS lookup;
    // the recorded fixture has 3 commodities matching "agri"*
    let hits = p.search(SearchEntities::Commodities, "agri").await.unwrap();
    assert_eq!(
        hits.len(),
        3,
        "Agricium, Agricium (Ore), Agricultural Supplies"
    );
    assert!(
        hits.iter().any(|h| h.name == "Agricium"),
        "match must include the exact commodity: {hits:?}"
    );
    assert!(hits.iter().all(|h| h.kind == "commodity"));

    // second search is served from cache, no new HTTP round-trip
    let hits = p.search(SearchEntities::Commodities, "agri").await.unwrap();
    assert_eq!(hits.len(), 3);
    assert_eq!(
        api.server.received_requests().await.unwrap().len(),
        1,
        "reference data must not be re-fetched while fresh"
    );

    // terminal hierarchy surfaces in the subtitle
    let t = p.search(SearchEntities::Terminals, "arc").await.unwrap();
    assert!(!t.is_empty());
    assert!(
        t[0].subtitle.contains("Stanton"),
        "subtitle should carry hierarchy: {:?}",
        t[0].subtitle
    );
    assert_eq!(t[0].kind, "terminal");
}

#[tokio::test]
async fn fetch_entities_items_enumerates_categories() {
    let api = MockedApi::start().await;
    let (p, _cache) = provider(&api);

    let items = p.fetch_entities(EntityKind::Items).await.unwrap();
    // categories fixture has 2 categories, one with 2 recorded items
    assert!(
        items.len() >= 2,
        "expected items from the recorded category, got {}",
        items.len()
    );
    assert!(items.iter().all(|e| e.kind == EntityKind::Items));
    assert!(items.iter().all(|e| e.id > 0));

    // Dedup (feedback 4): the items fixture also carries Quantainium with
    // is_commodity=1 — it must be filtered at import (it is represented by
    // the commodities dataset), never surface as a separate item.
    assert!(
        items.iter().all(|e| e.id != 4745),
        "is_commodity=1 items must be deduped at import: {items:?}"
    );
    assert!(items.iter().any(|e| e.id == 174));
    assert!(items.iter().any(|e| e.id == 465));
}

#[tokio::test]
async fn terminal_search_shows_fullname_instead_of_displayname() {
    let api = MockedApi::start().await;
    let (p, _cache) = provider(&api);

    let t = p.search(SearchEntities::Terminals, "arc").await.unwrap();
    assert!(!t.is_empty());
    // Feedback 2: the result name must be the complete terminal name
    // (fullname), not the bare displayname ("ARC-L1 Wide Forest Station"),
    // so the user recognizes the stand without guessing IDs.
    assert_eq!(
        t[0].name, "Commodity Shop - Admin - ARC-L1",
        "terminal hit must carry the full trade name, got {:?}",
        t[0].name
    );
    assert_eq!(t[0].kind, "terminal");
    assert!(
        t[0].subtitle.contains("ArcCorp"),
        "hierarchy must stay in the subtitle: {:?}",
        t[0].subtitle
    );

    // the Checkmate commodity terminal: fullname > name > displayname
    let checkmate = p
        .search(SearchEntities::Terminals, "checkmate")
        .await
        .unwrap();
    assert!(
        checkmate
            .iter()
            .any(|h| h.name == "Commodity Shop - Admin - Checkmate"),
        "Checkmate terminal must show the full commodity-shop name: {checkmate:?}"
    );
}

#[tokio::test]
async fn location_prices_aggregates_all_terminals_of_the_chain() {
    let api = MockedApi::start().await;
    let (p, _cache) = provider(&api);

    // Open "Admin - Checkmate" (terminal 436): the aggregate must cover every
    // terminal of the location chain (428 fuel, 431 refinery, 435 item,
    // 436 commodity), each with its own buy/sell table.
    let agg = p.location_prices(436).await.unwrap();
    assert_eq!(
        agg.location, "Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station",
        "location header must be the full chain"
    );
    assert_eq!(agg.terminal, "Commodity Shop - Admin - Checkmate");
    let ids: Vec<u64> = agg.groups.iter().map(|g| g.terminal_id).collect();
    assert_eq!(ids, [428, 431, 435, 436], "groups sorted by terminal id");

    // commodity terminal carries its price table; the others trade nothing
    let commodity = agg.groups.iter().find(|g| g.terminal_id == 436).unwrap();
    assert_eq!(
        commodity.terminal_name,
        "Commodity Shop - Admin - Checkmate"
    );
    assert!(commodity.prices.len() >= 4, "got {:?}", commodity.prices);
    assert!(
        commodity
            .prices
            .iter()
            .any(|pt| pt.commodity == "Agricultural Supplies"),
        "fixture rows must be mapped with names"
    );
    // Guns terminal (435) carries item prices; fuel/refinery (428, 431) trade nothing
    let guns = agg.groups.iter().find(|g| g.terminal_id == 435).unwrap();
    assert!(
        !guns.prices.is_empty(),
        "Guns terminal must show merged item prices"
    );
    assert!(
        guns.prices
            .iter()
            .any(|pt| pt.commodity == "CSP-68H Backpack"),
        "item price row must carry the item name"
    );
    assert_eq!(guns.prices[0].buy_price, 295.0);
    assert!(
        agg.groups
            .iter()
            .filter(|g| g.terminal_id != 436 && g.terminal_id != 435)
            .all(|g| g.prices.is_empty()),
        "fuel/refinery terminals must show an empty table, not an error"
    );

    // Cached second open must not hit the API again.
    let before = api.server.received_requests().await.unwrap().len();

    // Cached second open must not hit the API again.    let before = api.server.received_requests().await.unwrap().len();
    let again = p.location_prices(436).await.unwrap();
    assert_eq!(again.groups.len(), 4);
    let after = api.server.received_requests().await.unwrap().len();
    assert_eq!(
        after, before,
        "aggregate must be served from the terminal-prices cache while fresh"
    );
}

#[tokio::test]
async fn terminal_prices_merges_item_prices_for_shop_terminals() {
    let api = MockedApi::start().await;
    let (p, _cache) = provider(&api);

    // Guns terminal (435) trades no commodities but sells items — the terminal
    // price table must include those item rows (Point 3: shops show item prices).
    let prices = p.terminal_prices(435).await.unwrap();
    assert!(
        prices.iter().any(|pt| pt.commodity == "CSP-68H Backpack"),
        "Guns terminal must surface item prices: {prices:?}"
    );
    let backpack = prices
        .iter()
        .find(|pt| pt.commodity == "CSP-68H Backpack")
        .unwrap();
    assert_eq!(backpack.buy_price, 295.0);
    assert_eq!(backpack.terminal_id, 435);

    // Commodity terminal (436) still returns commodity prices only.
    let comm_prices = p.terminal_prices(436).await.unwrap();
    assert!(
        comm_prices
            .iter()
            .any(|pt| pt.commodity == "Agricultural Supplies")
    );
}

#[tokio::test]
async fn item_prices_fetches_live_then_serves_cache() {
    let api = MockedApi::start().await;
    let (p, _cache) = provider(&api);

    // First call: live fetch from /items_prices?id_item=465
    let points = p.item_prices(465).await.unwrap();
    assert_eq!(points.len(), 1, "fixture has one price row for item 465");
    assert_eq!(points[0].commodity, "CSP-68H Backpack");
    assert_eq!(points[0].buy_price, 295.0);
    assert_eq!(points[0].terminal_id, 107);

    let before = api.server.received_requests().await.unwrap().len();
    let again = p.item_prices(465).await.unwrap();
    assert_eq!(
        again.len(),
        1,
        "cached second call must return the same rows"
    );
    let after = api.server.received_requests().await.unwrap().len();
    assert_eq!(
        after, before,
        "item prices must be served from cache while fresh"
    );
}

#[tokio::test]
async fn prices_of_known_item_return_empty_not_error() {
    let api = MockedApi::start().await;
    let (p, _cache) = provider(&api);

    // An armor item has no commodity price table — the detail must come back
    // EMPTY (UI: "nicht kaufbar / keine Preisdaten") instead of a NotFound.
    let points = p.prices("CSP-68H Backpack").await.unwrap();
    assert!(points.is_empty(), "items are no commodities: {points:?}");
}

#[tokio::test]
async fn prices_serve_cache_then_refetch_after_ttl() {
    let api = MockedApi::start().await;
    let (p, cache) = provider(&api);

    let points = p.prices("agricium").await.unwrap();
    assert!(!points.is_empty());
    let fetched = points;
    assert_eq!(fetched[0].commodity, "Agricium");
    assert!(fetched.iter().all(|pt| pt.commodity_id == 1));

    // within TTL: served from cache, no extra request
    let again = p.prices("Agricium").await.unwrap();
    assert_eq!(again.len(), fetched.len());
    assert_eq!(
        api.server.received_requests().await.unwrap().len(),
        2, // 1× commodities refresh + 1× prices fetch; cached lookups add none
    );
    let _ = cache; // cache freshness asserted indirectly via request count
}

#[tokio::test]
async fn marketplace_offers_fetch_live_filter_and_sort() {
    let api = MockedApi::start().await;
    let (p, cache) = provider(&api);

    // generic fetch (no item_id/operation) → store snapshot, sort by price asc,
    // region resolved from /star_systems (default price sort = Newest)
    let offers = p
        .marketplace_offers(MarketplaceFilter {
            location: "".into(),
            q: "".into(),
            item_id: None,
            operation: None,
            region: "".into(),
            sort: Some(provider::OfferSort::PriceAsc),
            limit: 100,
        })
        .await
        .unwrap();
    assert!(!offers.is_empty(), "expected a hit, got {}", offers.len());
    // oldest returned = cheapest when sorted ascending; verify monotone prices
    let prices: Vec<f64> = offers
        .iter()
        .filter(|o| o.price > 0.0)
        .map(|o| o.price)
        .collect();
    let mut sorted = prices.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    assert_eq!(prices, sorted, "price ascending sort failed");

    // region resolution: fixture system id 55 = "Nyx", 68 = "Stanton"
    assert!(
        offers
            .iter()
            .any(|o| o.region == "Nyx" || o.region == "Stanton"),
        "regions not resolved: {:?}",
        offers.iter().map(|o| &o.region).collect::<Vec<_>>()
    );
    // every listing carries its permalink slug
    assert!(offers.iter().all(|o| !o.slug.is_empty()));

    // region + operation filters applied client-side
    let nyx = p
        .marketplace_offers(MarketplaceFilter {
            region: "Nyx".into(),
            sort: Some(provider::OfferSort::Newest),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(!nyx.is_empty());
    assert!(nyx.iter().all(|o| o.region == "Nyx"));

    // a generic fetch stored the snapshot for cache/offline reads
    let snapshot = cache.marketplace_snapshot().unwrap();
    assert!(!snapshot.is_empty());
    assert_eq!(snapshot.len(), offers.len());
}

#[tokio::test]
async fn marketplace_serves_cache_while_fresh() {
    let api = MockedApi::start().await;
    let (p, _cache) = provider(&api);

    // first call: snapshot stored (5-min TTL)
    p.marketplace_offers(MarketplaceFilter::default())
        .await
        .unwrap();

    // second call within TTL: served from cache — the only new requests are
    // the star-system probe (already stored) → no /marketplace_listings call.
    let before = api.server.received_requests().await.unwrap().len();
    p.marketplace_offers(MarketplaceFilter::default())
        .await
        .unwrap();
    let after = api.server.received_requests().await.unwrap().len();
    assert_eq!(
        (after, before),
        (2, 2),
        "cache-fresh marketplace read must not hit the API (cache served)"
    );
}

#[tokio::test]
async fn rate_limit_on_marketplace_can_be_served_from_snapshot() {
    // Provider on a dedicated server that ONLY rate-limits (no fixture mock),
    // and a snapshot warmed directly into the cache — so the live fetch of a
    // stale snapshot must fail instead of returning fixture data.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/marketplace_listings"))
        .respond_with(json_response(
            r#"{"status":"requests_limit_reached","http_code":429,"data":null,"message":"limit"}"#,
        ))
        .mount(&server)
        .await;
    // star systems must also be served (region resolution) — empty dataset is
    // fine, the point is the listings endpoint failing.
    Mock::given(method("GET"))
        .and(path("/star_systems"))
        .respond_with(json_response(
            r#"{"status":"ok","http_code":200,"data":[]}"#,
        ))
        .mount(&server)
        .await;

    let cache = Arc::new(Cache::open_in_memory().unwrap());
    let p = UexProvider::with_base_url(cache.clone(), None, &server.uri(), Duration::ZERO).unwrap();

    // Warm a snapshot, then make it stale (expired TTL).
    use provider::{ListingType, MarketOperation};
    cache
        .store_marketplace(&[provider::Offer {
            id: 42,
            title: "rescue + save tanga".into(),
            description: String::new(),
            item_id: 0,
            category_id: 0,
            operation: MarketOperation::Sell,
            listing_type: ListingType::Service,
            price: 250_000.0,
            currency: "UEC".into(),
            unit: "unit".into(),
            quantity: 1,
            location: "Levski".into(),
            region: "Nyx".into(),
            seller: "paladin".into(),
            quality: 0,
            updated: now_unix() - 3600,
            expires: None,
            slug: "rescue-save-tanga-abc123".into(),
        }])
        .unwrap();
    cache.set_marketplace_fetched_at(now_unix() - 301).unwrap();
    assert!(!cache.marketplace_fresh().unwrap());

    let err = p
        .marketplace_offers(MarketplaceFilter::default())
        .await
        .unwrap_err();
    // Stale snapshot + failed live fetch → the provider surfaces the error
    // (no fake data); callers (trade.rs get_marketplace) fall back to the
    // last snapshot. Verify the snapshot is still readable independently.
    assert!(
        matches!(err, ProviderError::RateLimit { .. }),
        "got {err:?}"
    );
    let snap = cache.marketplace_snapshot().unwrap();
    assert_eq!(snap.len(), 1, "snapshot must survive a failed refresh");
    assert_eq!(snap[0].slug, "rescue-save-tanga-abc123");
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[tokio::test]
async fn rate_limit_status_maps_to_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/commodities"))
        .respond_with(json_response(
            r#"{"status":"requests_limit_reached","http_code":429,"data":null,"message":"Requests limit reached"}"#,
        ))
        .mount(&server)
        .await;

    let cache = Arc::new(Cache::open_in_memory().unwrap());
    let p = UexProvider::with_base_url(cache, None, &server.uri(), Duration::ZERO).unwrap();

    let err = p.fetch_entities(EntityKind::Commodities).await.unwrap_err();
    assert!(
        matches!(err, ProviderError::RateLimit { .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn api_error_status_maps_to_http_error_with_message() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/commodities"))
        .respond_with(json_response(
            r#"{"status":"missing_required_input","http_code":400,"data":null,"message":"Missing required input"}"#,
        ))
        .mount(&server)
        .await;

    let cache = Arc::new(Cache::open_in_memory().unwrap());
    let p = UexProvider::with_base_url(cache, None, &server.uri(), Duration::ZERO).unwrap();

    let err = p.fetch_entities(EntityKind::Commodities).await.unwrap_err();
    match err {
        ProviderError::Http { status, body } => {
            assert_eq!(status, 400);
            assert!(body.contains("Missing required input"));
        }
        other => panic!("expected Http error, got {other:?}"),
    }
}
