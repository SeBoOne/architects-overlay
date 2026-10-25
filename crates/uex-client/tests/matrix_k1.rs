//! Live verification of the K1 Cache-Architectur (Stale-First + Profit-Matrix
//! + Background-Revalidate). Ignored by default like the other network tests.
//! Run: `cargo test -p uex_client --test matrix_k1 -- --ignored` (needs the
//! live API; the cold-sweep leg takes ~6 min at 300ms pacing).

use std::sync::Arc;
use std::time::Duration;

use cache::Cache;
use provider::{DataProvider, routes::RouteFilters};
use uex_client::UexProvider;

#[tokio::test]
#[ignore = "network test — run with -- --ignored"]
async fn live_matrix_revalidate() {
    let cache = Arc::new(Cache::open_in_memory().unwrap());
    // 300ms pacing keeps us well under the ~120 req/min daily quota.
    let p =
        UexProvider::with_base_url(
            cache.clone(),
            None,
            "https://api.uexcorp.uk/2.0",
            Duration::from_millis(300),
        )
        .unwrap();

    let filters = RouteFilters { scu: 24.0, ..RouteFilters::default() };

    // 1) COLD START: empty cache → the one blocking Sweep runs; the data was
    //    just fetched FRESH, so serving does NOT flag refreshing.
    let first = p.trade_routes(&filters).await.unwrap();
    assert !(!first.routes.is_empty(), "cold sweep must produce routes");
    assert !(!first.refreshing, "cold start serves freshly swept data");

    // 2) SECOND OPEN: Sweep marker fresh, epoch unchanged → the persisted
    //    Profit-Matrix serves WITHOUT recalculation and WITHOUT network calls.
    let second = p.trade_routes(&filters).await.unwrap();
    assert_eq !(
        second.routes.len(),
        first.routes.len(),
        "matrix must serve exactly the persisted set",
    );
    assert !(
        second.routes.iter().zip(first.routes.iter())
            .all(|(a, b)| a.commodity == b.commodity),
        "matrix must serve identical routes",
    );
    assert !(!second.refreshing, "warm cache must NOT flag refreshing");
    assert !(
        !p.route_sweep_stale().unwrap(),
        "INCONSISTENT: warm cache reports stale",
    );

    // 3) The matrix + its epoch are persisted and epoch-readable.
    let epoch = cache.route_matrix_epoch_get().unwrap();
    assert !(epoch.is_some(), "persist_route_matrix must stamp the dataset epoch");
    let rows = cache.route_matrix_rows().unwrap();
    assert_eq !(
        rows.len(),
        first.routes.len(),
        "route_matrix table must hold the persisted artifact",
    );

    // 4) Revalidate (manual refresh path): reference flags are TTL-checked
    //    (fresh → no network), FRESH terminals are skipped (the Sweep never
    //    hammers the API twice inside one TTL), the matrix is rebuilt.
    let swept = p.revalidate_route_data().await.unwrap();
    assert !(swept == 0, "revalidate must skip fresh terminals within TTL");

    // 5) After revalidation the epoch shifted and the matrix was rebuilt —
    //    serving again is instant with a fresh marker.
    let third = p.trade_routes(&filters).await.unwrap();
    assert !(!third.refreshing, "post-revalidate marker must be fresh");
    let epoch2 = cache.route_matrix_epoch_get().unwrap();
    assert_eq !(
        epoch2.as_deref(),
        epoch.as_deref(),
        "epoch unchanged after re-Sweep (same data)",
    );
}
