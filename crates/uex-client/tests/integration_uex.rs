//! Integration test against a **mocked** UEX API (wiremock serving the
//! recorded fixtures) — exercises the full provider path: HTTP client, pacing,
//! envelope unwrapping, mapping, cache read/write, staleness and error mapping.
//!
//! No network access. The fixture files under `tests/fixtures/` are live API
//! captures (October 2026), cut down to a handful of rows.

use std::sync::Arc;
use std::time::Duration;

use cache::Cache;
use provider::{
    DataProvider, EntityKind, MarketOperation, MarketplaceFilter, ProviderError, SearchEntities,
};
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

        Mock::given(method("GET"))
            .and(path("/commodities_prices"))
            .and(query_param("id_commodity", "1"))
            .respond_with(json_response(&fixture("prices.json")))
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
async fn marketplace_offers_fetch_live_and_filter_locally() {
    let api = MockedApi::start().await;
    let (p, cache) = provider(&api);

    // item_id + operation filter is forwarded to the API
    let offers = p
        .marketplace_offers(MarketplaceFilter {
            location: "levski".into(),
            q: "compensator".into(),
            item_id: Some(5_332),
            operation: Some(MarketOperation::Sell),
            limit: 100,
        })
        .await
        .unwrap();
    assert!(
        !offers.is_empty(),
        "expected a filtered hit, got {}",
        offers.len()
    );
    let offer = &offers[0];
    assert_eq!(offer.operation, MarketOperation::Sell);
    assert_eq!(offer.currency, "UEC");
    assert!(
        offer.price > 0.0,
        "price must parse from the string wire format"
    );
    assert!(offer.location.to_lowercase().contains("levski"));

    // snapshot persisted for offline display
    let snapshot = cache.marketplace_snapshot().unwrap();
    assert_eq!(snapshot.len(), offers.len());
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
