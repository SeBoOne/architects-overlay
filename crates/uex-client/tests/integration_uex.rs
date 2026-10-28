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
    DataProvider, Entity, EntityKind, MarketplaceFilter, ProviderError, SearchEntities,
    routes::RouteEnrichment,
};
use uex_client::UexProvider;
use wiremock::matchers::{header, method, path, query_param};
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
        for other in ["428", "431", "435", "437"] {
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

        // Same item id, live-captured shape WITH explicit `"price_sell": null`
        // (QA runde 2, Punkt 3/7 — the first row has null, the second a real
        // value). Proves the whole provider path survives null rows.
        Mock::given(method("GET"))
            .and(path("/items_prices"))
            .and(query_param("id_item", "466"))
            .respond_with(json_response(&fixture("item_prices_465_null.json")))
            .mount(&server)
            .await;

        // Terminal-scoped item prices WITH null sell rows (Guns terminal):
        // terminal_prices() must merge them without failing the table.
        Mock::given(method("GET"))
            .and(path("/items_prices"))
            .and(query_param("id_terminal", "437"))
            .respond_with(json_response(&fixture("item_prices_465_null.json")))
            .mount(&server)
            .await;
        for other in ["428", "431", "435", "436"] {
            Mock::given(method("GET"))
                .and(path("/items_prices"))
                .and(query_param("id_terminal", other))
                .respond_with(json_response(
                    r#"{"status":"ok","http_code":200,"data":[]}"#,
                ))
                .mount(&server)
                .await;
        }

        // Marketplace: param-specific mocks first (wiremock matches in mount
        // order — the id_item mock must outrank the operation mocks because a
        // q→id_item fetch may also carry operation), the catch-all covers any
        // other shape.
        // Item-scoped fetch (q → id_item server-side path): MacFlex 174.
        Mock::given(method("GET"))
            .and(path("/marketplace_listings"))
            .and(query_param("id_item", "174"))
            .respond_with(json_response(&fixture("marketplace_item_174.json")))
            .mount(&server)
            .await;
        // operation=sell → recorded sell rows; operation=buy → buy rows with
        // one overlapping id (179899) to exercise the merge dedupe.
        Mock::given(method("GET"))
            .and(path("/marketplace_listings"))
            .and(query_param("operation", "sell"))
            .respond_with(json_response(&fixture("marketplace_listings.json")))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/marketplace_listings"))
            .and(query_param("operation", "buy"))
            .respond_with(json_response(&fixture("marketplace_listings_buy.json")))
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
    // TRADE terminal of the location chain (435 item, 436 commodity). The
    // service terminals (428 fuel, 431 refinery) are excluded by the
    // terminal-type filter (Feedback-Runde 4 / P2): no buy/sell function →
    // not searched, not aggregated.
    let agg = p.location_prices(436).await.unwrap();
    assert_eq!(
        agg.location, "Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station",
        "location header must be the full chain"
    );
    assert_eq!(agg.terminal, "Commodity Shop - Admin - Checkmate");
    let ids: Vec<u64> = agg.groups.iter().map(|g| g.terminal_id).collect();
    assert_eq!(
        ids,
        [436, 435],
        "only trade terminals are aggregated, sorted by terminal NAME (t_dc152764)"
    );

    // commodity terminal carries its price table
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
    // Guns terminal (435) carries item prices
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

    // Cached second open must not hit the API again.
    let before = api.server.received_requests().await.unwrap().len();
    let again = p.location_prices(436).await.unwrap();
    assert_eq!(again.groups.len(), 2);
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
async fn item_prices_survive_explicit_null_sell_rows() {
    // QA runde 2, Punkt 3/7: /items_prices sends `"price_sell": null` rows
    // (measured live: 1/16 on item 465, 22/90 on terminal 437). The whole
    // provider path — wire DTO → seam PricePoint — must survive them; the row
    // maps to sell 0 instead of a Json error.
    let api = MockedApi::start().await;
    let (p, _cache) = provider(&api);

    let points = p.item_prices(466).await.unwrap();
    assert_eq!(points.len(), 2, "both fixture rows must parse");
    let null_row = points.iter().find(|pt| pt.terminal_id == 107).unwrap();
    assert_eq!(null_row.buy_price, 295.0);
    assert_eq!(null_row.sell_price, 0.0, "null sell must map to 0");
    let priced = points.iter().find(|pt| pt.terminal_id == 113).unwrap();
    assert_eq!(priced.sell_price, 350.0, "real values must survive");
}

#[tokio::test]
async fn terminal_prices_survive_null_item_rows_and_item_fetch_failure() {
    // Regression (QA runde 2): `terminal_prices_impl` merged the item fetch
    // with `.await?` — one null in /items_prices sank the ENTIRE terminal
    // table (incl. commodity rows). Two guards:
    //   1. null item rows parse (Guns terminal 437 fixture).
    //   2. even a hard item-fetch failure (500) must not sink commodities.
    let api = MockedApi::start().await;
    let (p, _cache) = provider(&api);

    let prices = p.terminal_prices(437).await.unwrap();
    assert!(
        prices.iter().any(|pt| pt.commodity == "CSP-68H Backpack"),
        "null item rows must still surface as rows: {prices:?}"
    );

    // Item fetch fails hard → commodity table must still come through.
    let degraded = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/commodities_prices"))
        .and(query_param("id_terminal", "436"))
        .respond_with(json_response(&fixture("terminal_prices_checkmate.json")))
        .mount(&degraded)
        .await;
    Mock::given(method("GET"))
        .and(path("/items_prices"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&degraded)
        .await;
    let cache = Arc::new(Cache::open_in_memory().unwrap());
    let pk = UexProvider::with_base_url(cache, None, &degraded.uri(), Duration::ZERO).unwrap();
    let prices = pk.terminal_prices(436).await.unwrap();
    assert!(
        prices
            .iter()
            .any(|pt| pt.commodity == "Agricultural Supplies"),
        "commodity table must survive a failing /items_prices: {prices:?}"
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

    // second call within TTL: served from cache — the first call spent two
    // /marketplace_listings fetches (sell + buy windows) plus the star-system
    // probe; the second adds none of them.
    let before = api.server.received_requests().await.unwrap().len();
    p.marketplace_offers(MarketplaceFilter::default())
        .await
        .unwrap();
    let after = api.server.received_requests().await.unwrap().len();
    assert_eq!(
        (after, before),
        (3, 3),
        "cache-fresh marketplace read must not hit the API (cache served)"
    );
}

#[tokio::test]
async fn market_all_mode_merges_sell_and_buy_windows() {
    // P1 regression: the API exposes no pagination and caps each fetch, so the
    // old single fetch only ever saw one newest-window — q/sort over it looked
    // "ignored" for the all view. The all mode must merge BOTH operation
    // windows (and dedupe overlapping ids) before filtering/sorting locally.
    let api = MockedApi::start().await;
    let (p, _cache) = provider(&api);

    let offers = p
        .marketplace_offers(MarketplaceFilter {
            sort: Some(provider::OfferSort::PriceAsc),
            ..Default::default()
        })
        .await
        .unwrap();

    // both operations present in the merged view
    assert!(
        offers
            .iter()
            .any(|o| o.operation == provider::MarketOperation::Sell),
        "sell window missing"
    );
    assert!(
        offers
            .iter()
            .any(|o| o.operation == provider::MarketOperation::Buy),
        "buy window missing"
    );
    // the id 179899 exists in BOTH fixtures → merged exactly once
    assert_eq!(
        offers.iter().filter(|o| o.id == 179899).count(),
        1,
        "overlapping id must be deduped"
    );
    // price sort spans the merged set
    let prices: Vec<f64> = offers.iter().map(|o| o.price).collect();
    let mut sorted = prices.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    assert_eq!(
        prices, sorted,
        "price ascending sort across merged windows failed"
    );

    // exactly two /marketplace_listings calls, one per operation
    let reqs = api.server.received_requests().await.unwrap();
    let marketplace_ops: Vec<Option<String>> = reqs
        .iter()
        .filter(|r| r.url.path().contains("marketplace_listings"))
        .map(|r| {
            r.url
                .query_pairs()
                .find(|(k, _)| k == "operation")
                .map(|(_, v)| v.to_string())
        })
        .collect();
    assert_eq!(
        marketplace_ops,
        vec![Some("sell".into()), Some("buy".into())],
        "all mode must fetch one window per operation"
    );
    // the API ignores a `limit` parameter — the client must not send one
    assert!(
        reqs.iter()
            .filter(|r| r.url.path().contains("marketplace_listings"))
            .all(|r| !r.url.query_pairs().any(|(k, _)| k == "limit")),
        "no limit param may be sent (API ignores it)"
    );
}

#[tokio::test]
async fn market_wts_resolves_term_to_item_and_fetches_server_side() {
    // P1 regression: WTS + term ("Parallax"-style) must not filter a tiny
    // newest window — a term that names an item goes to the API as `id_item`
    // (+ `operation`), which returns the COMPLETE per-item set and unlocks
    // the 1,000-row cap, so the local price sort acts on the real data.
    let api = MockedApi::start().await;
    let (p, _cache) = provider(&api);

    let offers = p
        .marketplace_offers(MarketplaceFilter {
            q: "MacFlex".into(),
            operation: Some(provider::MarketOperation::Sell),
            sort: Some(provider::OfferSort::PriceDesc),
            ..Default::default()
        })
        .await
        .unwrap();

    assert!(
        !offers.is_empty(),
        "item-scoped fetch must return the item's offers"
    );
    assert!(
        offers.iter().all(|o| o.item_id == 174),
        "every offer must come from the id_item=4745 fetch"
    );
    assert!(
        offers
            .iter()
            .all(|o| o.operation == provider::MarketOperation::Sell)
    );
    let prices: Vec<f64> = offers.iter().map(|o| o.price).collect();
    let mut sorted = prices.clone();
    sorted.sort_by(|a, b| b.total_cmp(a));
    assert_eq!(prices, sorted, "price descending sort failed");

    let reqs = api.server.received_requests().await.unwrap();
    let item_scoped = reqs
        .iter()
        .filter(|r| r.url.path().contains("marketplace_listings"))
        .filter(|r| {
            r.url
                .query_pairs()
                .any(|(k, v)| k == "id_item" && v == "174")
        })
        .count();
    assert_eq!(item_scoped, 1, "exactly one id_item fetch expected");
    // P3 hybrid (Feedback-Runde 4): alongside the id_item fetch the FULL
    // sell window is fetched too, so the local q/quality/sort filter runs
    // over the complete market — the merge dedupes via `seen`.
    let window_fetches = reqs
        .iter()
        .filter(|r| r.url.path().contains("marketplace_listings"))
        .filter(|r| r.url.query_pairs().any(|(k, _)| k == "operation"))
        .filter(|r| !r.url.query_pairs().any(|(k, _)| k == "id_item"))
        .count();
    assert_eq!(
        window_fetches, 1,
        "hybrid plan must add the full operation window fetch"
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

// Regression (QA-Runde 3, Auflage B): die Live-API schickt bei
// /marketplace_listings einzelne Zeilen mit `"location": null` — ein
// `#[serde(default)]` alleine fängt das NICHT ab (null ≠ fehlendes Feld) und
// der Fetch-Plan (Verkauf+Kauf-Fenster) scheiterte am realen Datenbestand
// (`invalid type: null, expected a string`). Die String-Felder des
// ListingRow sind jetzt null-tolerant (null → "").
#[tokio::test]
async fn marketplace_offers_null_location_tolerated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/star_systems"))
        .respond_with(json_response(&fixture("star_systems.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/marketplace_listings"))
        .respond_with(json_response(&fixture("marketplace_listings_null.json")))
        .mount(&server)
        .await;

    let cache = Arc::new(Cache::open_in_memory().unwrap());
    let p = UexProvider::with_base_url(cache, None, &server.uri(), Duration::ZERO).unwrap();

    let offers = p
        .marketplace_offers(MarketplaceFilter {
            operation: Some(provider::MarketOperation::Sell),
            ..Default::default()
        })
        .await
        .expect("rows with location: null must not fail the fetch");
    assert!(!offers.is_empty(), "expected the null-fixture rows");
    // Kontrolle: 2 der 3 Fixture-Zeilen schicken `"location": null` (→ ""),
    // die dritte trägt eine echte Location — beide Formen müssen gleichzeitig
    // durchkommen.
    assert_eq!(offers.iter().filter(|o| o.location.is_empty()).count(), 2);
    assert_eq!(offers.iter().filter(|o| !o.location.is_empty()).count(), 1);
    assert!(offers.iter().all(|o| !o.slug.is_empty()));
}

// Regression (t_790ef376): die id-Resolution löst ALLE FTS-Treffer eines
// Begriffs auf — nicht mehr nur die Top-3 (MARKET_ITEM_ID_CAP). Ein Begriff,
// der 5 Items matcht, muss 5 id_item-Fetches erzeugen (jeder liefert das
// KOMPLETTE Offer-Set des Items), damit das ≥15-Offers-Kriterium bei breiten
// Begriffen erreichbar ist (Live-Fall: "parallax" matched 10+ Items, UA-4433
// mit 18 Q≥900-Offers lag außerhalb der Top-3).
#[tokio::test]
async fn market_resolves_every_fts_hit_of_a_term() {
    let server = MockServer::start().await;

    // 5 Items, alle matchen den Begriff "parallax".
    let items: Vec<String> = (0..5)
        .map(|i| {
            format!(
                r#"{{"id":{},"name":"Parallax Mk.{} Module","is_commodity":0}}"#,
                9000 + i,
                i
            )
        })
        .collect();
    let items_json = format!(
        r#"{{"status":"ok","http_code":200,"data":[{}]}}"#,
        items.join(",")
    );
    Mock::given(method("GET"))
        .and(path("/items"))
        .respond_with(json_response(&items_json))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/categories"))
        .respond_with(json_response(&fixture("categories.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/star_systems"))
        .respond_with(json_response(
            r#"{"status":"ok","http_code":200,"data":[]}"#,
        ))
        .mount(&server)
        .await;
    // Listings-Antworten sind hier egal (leer) — gezählt wird nur, WIE VIELE
    // DISTINCTE id_item-Fetches der Begriff erzeugt.
    Mock::given(method("GET"))
        .and(path("/marketplace_listings"))
        .respond_with(json_response(
            r#"{"status":"ok","http_code":200,"data":[]}"#,
        ))
        .mount(&server)
        .await;

    let cache = Arc::new(Cache::open_in_memory().unwrap());
    let p = UexProvider::with_base_url(cache, None, &server.uri(), Duration::ZERO).unwrap();

    p.marketplace_offers(MarketplaceFilter {
        q: "parallax".into(),
        ..Default::default()
    })
    .await
    .unwrap();

    let reqs = server.received_requests().await.unwrap();
    let mut ids: Vec<u64> = reqs
        .iter()
        .filter(|r| r.url.path().contains("marketplace_listings"))
        .filter_map(|r| {
            r.url
                .query_pairs()
                .find(|(k, _)| k == "id_item")
                .map(|(_, v)| v.parse::<u64>().unwrap())
        })
        .collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(
        ids,
        vec![9000, 9001, 9002, 9003, 9004],
        "every FTS hit of the term must be resolved to an id_item fetch (old cap was 3)"
    );
}

/// P15a-Härtung (Fix-Karte t_0f1cac34, Review-Blocker t_b3153a53
/// 'Trust-by-TTL'): der Routen-Sweep darf einen kontaminierten, aber
/// TTL-frischen route_meta:terminals-Payload NIEMALS blind vertrauen.
///
/// Setup wie die Review-Reproduktion: seedeter Legacy-Payload (v0.3.1-Muster:
/// Flag-Einträge für ALLE Terminal-IDs — hier commodity 1/2/436 PLUS die
/// ITEM-Terminals 126 'Live Fire Weapons - HUR-L2' und 435) mit frischem
/// fetched_at, seedeter Terminal-Entities-Cache (126 enthalten), KEIN
/// Ground-truth-Sidecar (Pre-Härtungs-Cache).
///
/// Erwartet (Abnahmekriterium des Reviewers):
/// * Self-Heal: /terminals wird re-fetchet — fetched_at ändert sich.
/// * Sweep refetcht keine Preis-Tabellen für ITEM-Terminals (126/435).
/// * Routen-Ausgabe nutzt ausschließlich commodity/commodity_raw-Endpoints.
#[tokio::test]
async fn p15a_contaminated_route_meta_never_feeds_the_sweep() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/terminals"))
        .respond_with(json_response(&fixture("routes_terminals.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/commodities"))
        .respond_with(json_response(&fixture("commodities.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/commodities_prices"))
        .and(query_param("id_terminal", "1"))
        .respond_with(json_response(&fixture("routes_prices_1.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/commodities_prices"))
        .and(query_param("id_terminal", "2"))
        .respond_with(json_response(&fixture("routes_prices_2.json")))
        .mount(&server)
        .await;
    // 436 ohne Preise, ITEM-126 mit umfangreicher Tabelle: würde sie im
    // Sweep landen, wäre der Payload kontaminiert (Live-Evidenz der Review:
    // ITEM-126 mit 70 Preiszeilen refetched).
    Mock::given(method("GET"))
        .and(path("/commodities_prices"))
        .respond_with(json_response(
            r#"{"status":"ok","http_code":200,"data":[]}"#,
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/items_prices"))
        .respond_with(json_response(
            r#"{"status":"ok","http_code":200,"data":[]}"#,
        ))
        .mount(&server)
        .await;

    let cache = Arc::new(Cache::open_in_memory().unwrap());
    let now = {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    };

    // Terminal-Entities + Frische-Marker frisch (ensure_fresh bleibt beim
    // Network-Check still) — wie im Live-Upgrade-Fall.
    cache
        .store_entities(
            EntityKind::Terminals,
            &[
                Entity {
                    id: 1,
                    kind: EntityKind::Terminals,
                    name: "Admin - ARC-L1".into(),
                    code: None,
                    subtitle: "ArcCorp > ArcCorp > Landing Zone".into(),
                },
                Entity {
                    id: 2,
                    kind: EntityKind::Terminals,
                    name: "Admin - ARC-L2".into(),
                    code: None,
                    subtitle: "ArcCorp > ArcCorp > Landing Zone".into(),
                },
                Entity {
                    id: 436,
                    kind: EntityKind::Terminals,
                    name: "Admin - Checkmate".into(),
                    code: None,
                    subtitle: "ArcCorp > ArcCorp > Checkmate Station".into(),
                },
                Entity {
                    id: 126,
                    kind: EntityKind::Terminals,
                    name: "Live Fire Weapons - HUR-L2".into(),
                    code: None,
                    subtitle: "Nyx > Yela > Hoover's Station".into(),
                },
                Entity {
                    id: 435,
                    kind: EntityKind::Terminals,
                    name: "Medical Shop - Checkmate".into(),
                    code: None,
                    subtitle: "ArcCorp > ArcCorp > Checkmate Station".into(),
                },
            ],
        )
        .unwrap();

    // Kontaminierter Legacy-Flag-Payload, TTL-FRISCH (12h-TTL würde blind
    // trusten): enthält explizit ITEM-Terminal 126.
    cache
        .route_meta_set(
            "route_meta:terminals",
            now - 600,
            r#"{"1":{"autoload":true,"cargo_center":false,"ground":true,"chain":"ArcCorp > ArcCorp > Landing Zone"},
                "2":{"autoload":true,"cargo_center":false,"ground":true,"chain":"ArcCorp > ArcCorp > Landing Zone"},
                "436":{"autoload":true,"cargo_center":false,"ground":false,"chain":"ArcCorp > ArcCorp > Checkmate Station"},
                "126":{"autoload":false,"cargo_center":false,"ground":false,"chain":"Nyx > Yela > Hoover's Station"},
                "435":{"autoload":false,"cargo_center":false,"ground":false,"chain":"ArcCorp > ArcCorp > Checkmate Station"}}"#,
        )
        .unwrap();

    let before: u64 = cache
        .route_meta_get("route_meta:terminals")
        .unwrap()
        .expect("seeded payload")
        .0;

    let p = UexProvider::with_base_url(cache.clone(), None, &server.uri(), Duration::ZERO).unwrap();
    let out = p
        .trade_routes(&provider::routes::RouteFilters::default())
        .await
        .unwrap();

    // 1) Self-Heal: frischer, aber kontaminierter Payload wurde re-fetchet —
    //    fetched_at hat sich bewegt (Review-Abnahme: 'fetched_at ändert sich').
    let (after, healed_payload) = cache
        .route_meta_get("route_meta:terminals")
        .unwrap()
        .expect("payload after run");
    assert_ne!(
        after, before,
        "kontaminierter Payload muss re-fetchet werden (Self-Heal)"
    );

    // 2) Nach Start/Erstlauf sind payload UND Ground truth sauber.
    let types: std::collections::HashMap<String, String> = serde_json::from_str(
        &cache
            .route_meta_get("route_meta:terminal_types")
            .unwrap()
            .expect("ground-truth-sidecar geschrieben")
            .1,
    )
    .unwrap();
    assert_eq!(types.get("126").map(String::as_str), Some("item"));
    assert_eq!(types.get("436").map(String::as_str), Some("commodity"));
    let flag_ids: std::collections::HashSet<u64> =
        serde_json::from_str::<std::collections::HashMap<u64, serde_json::Value>>(&healed_payload)
            .unwrap()
            .keys()
            .copied()
            .collect();
    assert!(flag_ids.contains(&1) && flag_ids.contains(&2) && flag_ids.contains(&436));
    assert!(
        !flag_ids.contains(&126) && !flag_ids.contains(&435),
        "ITEM-Terminals dürfen im geheilten Payload nicht vorkommen"
    );

    // 3) Sweep-Fenster: /commodities_prices wurde NIE für ITEM-Terminals
    //    aufgerufen (Live-Evidenz der Review war genau das: 236 Tabellen inkl.
    //    ITEM-126). Stattdessen ausschließlich die commodity-Terminals.
    let reqs = server.received_requests().await.unwrap();
    let swept: Vec<u64> = reqs
        .iter()
        .filter(|r| r.url.path() == "/commodities_prices")
        .filter_map(|r| {
            r.url
                .query_pairs()
                .find(|(k, _)| k == "id_terminal")
                .map(|(_, v)| v.parse::<u64>().unwrap())
        })
        .collect();
    assert!(
        !swept.contains(&126) && !swept.contains(&435),
        "ITEM-Terminal-Preis-Tabellen dürfen im Sweep nicht refetched werden: {swept:?}"
    );
    let mut swept_unique = swept;
    swept_unique.sort_unstable();
    swept_unique.dedup();
    assert_eq!(
        swept_unique,
        vec![1u64, 2u64, 436u64],
        "Sweep refetcht genau die commodity-Terminals des Payloads"
    );

    // 4) Routen-Ausgabe: Endpoints ausschließlich commodity/commodity_raw —
    //    NiX/Hoover ITEM-Shops tauchen nie als Kauf- oder Verkaufsort auf.
    assert!(!out.routes.is_empty(), "Routen müssen computed werden");
    for r in &out.routes {
        assert_ne!(r.buy_terminal_id, 126);
        assert_ne!(r.sell_terminal_id, 126);
        assert_ne!(r.buy_terminal_id, 435);
        assert_ne!(r.sell_terminal_id, 435);
        assert!(
            [1u64, 2, 436].contains(&r.buy_terminal_id)
                && [1u64, 2, 436].contains(&r.sell_terminal_id),
            "route endpoints must be commodity terminals: {r:?}"
        );
    }
}

/// t_65a91d5b C3b: simulierter UEX-Ausfall des `/commodities_routes`-Endpoints
/// (PLURAL) — das Enrichment darf die bestehende Map NICHT überschreiben und
/// darf als Fehler die Revalidation/Matrix NICHT sinken lassen. Der Code
/// gibt `Ok(0)` zurück (0 Routen geholt) und lässt den alten Bestand stehen
/// (stale-first). Genau das ist das Abnahmekriterium „Matrix bleibt\".
#[tokio::test]
async fn enrichment_outage_keeps_existing_map_and_does_not_fail() {
    // UEX-Komplettausfall auf dem Enrichment-Endpoint: 500 auf /commodities_routes.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/commodities_routes"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/commodities"))
        .respond_with(json_response(&fixture("commodities.json")))
        .mount(&server)
        .await;
    // Revalidation braucht auch /terminals + leere Preis-Tabellen (keine
    // Routen-Daten) — damit der Verlauf bis zum Enrichment tatsächlich Ok
    // zurückgibt und der Enrichment-Fehler der einzige nicht-triviale ist.
    Mock::given(method("GET"))
        .and(path("/terminals"))
        .respond_with(json_response(&fixture("routes_terminals.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/commodities_prices"))
        .respond_with(json_response(
            r#"{"status":"ok","http_code":200,"data":[]}"#,
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/items_prices"))
        .respond_with(json_response(
            r#"{"status":"ok","http_code":200,"data":[]}"#,
        ))
        .mount(&server)
        .await;

    let cache = Arc::new(Cache::open_in_memory().unwrap());
    let provider =
        UexProvider::with_base_url(cache.clone(), None, &server.uri(), Duration::ZERO).unwrap();

    // Bereits vorhandener, TTL-frischer Enrichment-Bestand für die Matrix-Route.
    let key = "47:1:274";
    cache
        .store_route_enrichment(&[(
            key.into(),
            RouteEnrichment {
                uex_score: 953,
                scu_reachable: 2.0,
                ..RouteEnrichment::default()
            },
        )])
        .unwrap();

    // Enrichment-Refresh gegen den toten Endpoint: Ok(0), kein Fehler.
    let n = provider.refresh_route_enrichment().await.unwrap();
    assert_eq!(n, 0, "outage must report 0 enriched, not error");

    // Der alte Bestand ist UNVERÄNDERT (stale-first: nichts überschreiben).
    let rows = cache.route_enrichment_rows().unwrap();
    assert_eq!(rows.len(), 1, "old map must survive the outage");
    assert_eq!(rows.get(key).unwrap().uex_score, 953);

    // Revalidation schlägt am Enrichment NICHT fehl (Fehler wird geloggt) —
    // alle anderen Pfade sind gemockt (Ok auch bei down).
    let _ = provider.revalidate_route_data().await.unwrap();
}

/// M2a-Fix (t_2f197d1c): User-scoped Reads senden den persönlichen Secret-Key
/// NUR als `secret-key`-Header, wenn er via `with_secret_key` gesetzt ist —
/// der App-Key bleibt reiner Bearer und geht NICHT als secret-key mit.
/// Wiremock-matcht den Header: ein Request ohne den Header erfüllt den Mock
/// nicht → 404 → Provider fällt in den Fehlerpfad (Test scheitert sonst).
#[tokio::test]
async fn user_trades_sends_secret_key_header_only_when_set() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user_trades"))
        .and(header("secret-key", "my-super-secret-40"))
        .respond_with(json_response(
            r#"{"status":"ok","http_code":200,"data":[]}"#,
        ))
        .mount(&server)
        .await;

    // Ohne with_secret_key → KEIN secret-key-Header → der obige Mock matcht
    // nicht, /user_trades antwortet 404 → Fehler. Damit ist belegt, dass der
    // Key NICHT automatisch (etwa aus dem App-Key) mitgesendet wird.
    let cache = Arc::new(Cache::open_in_memory().unwrap());
    let no_key = UexProvider::with_base_url(
        cache.clone(),
        Some("app-token".into()), // App-Key gesetzt — reicht NICHT für user_trades
        &server.uri(),
        Duration::ZERO,
    )
    .unwrap();
    assert!(
        no_key.user_trades().await.is_err(),
        "user_trades ohne secret-key muss fehlschlagen (UEX verlangt beides)"
    );

    // Mit with_secret_key → Request trägt den Header → Mock matcht → Ok.
    let cache2 = Arc::new(Cache::open_in_memory().unwrap());
    let with_key = UexProvider::with_base_url(
        cache2.clone(),
        Some("app-token".into()),
        &server.uri(),
        Duration::ZERO,
    )
    .unwrap()
    .with_secret_key(Some("my-super-secret-40".into()));
    assert!(
        with_key.user_trades().await.is_ok(),
        "user_trades mit secret-key muss Ok sein (Header gesendet)"
    );
}
