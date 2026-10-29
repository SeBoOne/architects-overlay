//! Live proof for PS (t_c84e545d): the synthesized Locations dataset + the
//! forced price refresh, against the real UEX API. Opt-in (-- --ignored).

use std::time::Duration;

use cache::Cache;
use provider::{DataProvider, EntityKind};
use uex_client::UexProvider;

#[tokio::test]
#[ignore = "network test — run with -- --ignored"]
async fn live_locations_synth_size_and_names() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::open(&dir.path().join("live.sqlite3")).unwrap();
    let p = UexProvider::new(Arc::new(cache), None).unwrap();
    let locs = p.fetch_entities(EntityKind::Locations).await.unwrap();
    let lev = locs.iter().find(|e| e.name.eq_ignore_ascii_case("Levski"));
    println!("locations synthesized: {}", locs.len());
    println!("levski: {lev:?}");
    assert!(locs.len() > 100, " Locations dataset must be non-trivial");
    assert!(
        lev.is_some(),
        "Levski must exist in the synthesized dataset"
    );
}

#[tokio::test]
#[ignore = "network test — run with -- --ignored"]
async fn live_forced_commodity_refresh_renews_timestamp() {
    let dir = tempfile::tempdir().unwrap();
    let cache = std::sync::Arc::new(Cache::open(&dir.path().join("live.sqlite3")).unwrap());
    let p = UexProvider::new(cache.clone(), None).unwrap();
    let first = p.refresh_prices("laranite").await.unwrap();
    assert!(!first.is_empty());
    let ages1 = cache.price_sync_ages().unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    let second = p.refresh_prices("laranite").await.unwrap();
    assert!(!second.is_empty());
    let ages2 = cache.price_sync_ages().unwrap();
    println!(
        "commodity_age before={:?} after={:?}",
        ages1.commodity_age_secs, ages2.commodity_age_secs
    );
    // The second forced refresh RE-MARKS the table: the age resets.
    assert!(ages2.commodity_age_secs.unwrap() < ages1.commodity_age_secs.unwrap() + 1);
}

use std::sync::Arc;
