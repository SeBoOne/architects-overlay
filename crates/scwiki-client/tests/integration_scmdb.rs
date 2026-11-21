//! SCMDB-Mining-Integration (M8-R2, t_7a7844cb) — `data/latest.json` →
//! `mining_data-<version>.json` gegen KrovaxCode/SCMDB_DATA-Form-Belege
//! (Fixtures nach LIVE-Struktur 2026-11-20: Taranite (Raw) /
//! [310,525,646,718,853,925,957,1000] · Carinite-Dup · Aphorite ohne
//! materialName).

use scwiki_client::{ScmdbClient, normalize_material};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture vorhanden")
}

#[tokio::test]
async fn mining_index_fetches_latest_then_mining_data() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/data/latest.json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("scmdb_latest.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/data/mining_data-4.10.3-live.13220821.json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("scmdb_mining_data.json")))
        .mount(&server)
        .await;

    let idx = ScmdbClient::new(&server.uri())
        .unwrap()
        .mining_index()
        .await
        .expect("parse ok");

    // LIVE-Beleg Omar 2026-11-20: Taranite (Raw) Bänder exakt.
    assert_eq!(
        idx.bands_for("Taranite (Raw)"),
        vec![310, 525, 646, 718, 853, 925, 957, 1000]
    );
    assert_eq!(
        idx.bands_for("Taranite"),
        vec![310, 525, 646, 718, 853, 925, 957, 1000]
    );
    assert_eq!(
        idx.bands_for("Borase (Ore)"),
        vec![274, 526, 665, 762, 867, 916, 959, 1000]
    );
    // Kein materialName? name-Form greift trotzdem.
    assert_eq!(
        idx.bands_for("Aphorite"),
        vec![280, 520, 640, 720, 850, 920, 960, 1000]
    );
    // Carinite-Dup-Case: identische Bänder kollabieren zur Union.
    assert_eq!(
        idx.bands_for("Carinite"),
        vec![273, 554, 647, 716, 880, 909, 959, 1000]
    );
    // Kein Match → leer (UI rastet auf 500-only).
    assert!(idx.bands_for("Unobtainium").is_empty());
}

#[tokio::test]
async fn mining_index_missing_latest_is_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/data/latest.json"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let err = ScmdbClient::new(&server.uri())
        .unwrap()
        .mining_index()
        .await
        .expect_err("404 muss als Fehler ankommen");
    assert!(matches!(err, scwiki_client::ScWikiError::Status(404)));
}

/// Normalize-Matrix: Suffixe strip, Case/Space egal — Match gegen beide
/// Namensformen.
#[test]
fn normalize_matches_raw_ore_pure_and_bare() {
    assert_eq!(normalize_material("Taranite (Raw)"), "taranite");
    assert_eq!(normalize_material("taranite (RAW)"), "taranite");
    assert_eq!(normalize_material(" Borase (Ore) "), "borase");
    assert_eq!(normalize_material("Carinite (Pure)"), "carinite");
    assert_eq!(normalize_material("Aphorite"), "aphorite");
    assert_eq!(normalize_material("  HADANITE  "), "hadanite");
    // Kein Suffix-Anhnahme-Raten: unbekannte Klammer bleibt.
    assert_eq!(normalize_material("Foo (Bar)"), "foo (bar)");
}
