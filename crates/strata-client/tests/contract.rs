//! M7-K1 DTO-Contract-Tests: Deserialisierung gegen die Spec-Examples der
//! Strata Public API (docs/MINING-MODE-PLAN.md, strata.celd.space/strata-api.md).
//! KEIN Live-API-Call — die Fixtures sind die Spec-Beispiele; Transportfälle
//! (Bearer-Header, 429+Retry-After) laufen gegen wiremock.

use std::collections::BTreeMap;

use strata_client::{
    LocationOresResponse, LocationsResponse, OreLocationsResponse, OresResponse, error_key,
    keystore,
};

// ---- Spec-Beispiel: GET /api/public/ores ----------------------------------

#[test]
fn ores_spec_example_parses() {
    let json = r#"{
      "ores": [
        { "id": "beryl_raw", "name": "Beryl (Raw)", "slug": "beryl",
          "tier": 3, "category": "ship", "scanSignature": 1350,
          "instability": 0.05, "resistance": 0.1 }
      ],
      "count": 38,
      "attribution": "Data from celd.space / Strata Mining Tools"
    }"#;
    let r: OresResponse = serde_json::from_str(json).unwrap();
    assert_eq!(r.count, 38);
    assert_eq!(
        r.attribution, "Data from celd.space / Strata Mining Tools",
        "Attribution wird durchgereicht (Pflichtanzeige)"
    );
    let ore = &r.ores[0];
    assert_eq!(ore.id, "beryl_raw");
    assert_eq!(ore.tier, Some(3));
    assert_eq!(ore.scan_signature, Some(1350));
    assert_eq!(ore.instability, Some(0.05));
}

/// Null-Disziplin: null-Felder → Option(None), und 0.0-Werte sind GÜLTIGE
/// Daten (Spec: „use a null-check, not a truthiness check").
#[test]
fn ores_null_fields_are_option_and_zero_is_valid() {
    let json = r#"{
      "ores": [
        { "id": "minableelement_fps_aphorite", "name": "Aphorite (Raw)",
          "slug": "aphorite", "tier": null, "category": "fps",
          "scanSignature": 3000, "instability": 0, "resistance": 0 }
      ],
      "count": 1,
      "attribution": "Data from celd.space / Strata Mining Tools"
    }"#;
    let r: OresResponse = serde_json::from_str(json).unwrap();
    let ore = &r.ores[0];
    assert!(ore.tier.is_none(), "tier=null → None");
    assert_eq!(
        ore.instability,
        Some(0.0),
        "0.0 ist gültige Daten — KEIN falsches None"
    );
    assert_eq!(ore.resistance, Some(0.0));
}

// ---- Spec-Beispiel: GET /api/public/locations ------------------------------

#[test]
fn locations_spec_example_parses() {
    let json = r#"{
      "locations": [
        { "id": "aaron_halo", "name": "Aaron Halo", "slug": "aaron-halo",
          "system": "Stanton", "type": "exploration_region" },
        { "id": "ita_01", "name": "Ita Belt Alpha", "slug": "ita-belt-alpha",
          "system": "Pyro", "type": "belt" }
      ],
      "count": 282,
      "attribution": "Data from celd.space / Strata Mining Tools"
    }"#;
    let r: LocationsResponse = serde_json::from_str(json).unwrap();
    assert_eq!(r.count, 282);
    assert_eq!(r.locations[0].kind, "exploration_region");
    assert_eq!(r.locations[1].id, "ita_01");
}

// ---- Spec-Beispiel: GET /api/public/ore-locations/{ore} --------------------

#[test]
fn ore_locations_spec_example_parses_grouped() {
    let json = r#"{
      "ore": { "id": "beryl_raw", "name": "Beryl (Raw)" },
      "sort": "concentration",
      "tierLegend": { "0": "None", "1": "Negligible", "2": "Trace",
        "3": "Minor", "4": "Notable", "5": "Major", "6": "Pure" },
      "results": {
        "space": [
          { "locationId": "ita_01", "location": "Ita Belt Alpha", "system": "Pyro",
            "rankPct": 100, "rank": 1, "n": 47, "concentration": 0.31,
            "tier": 6, "tierName": "Pure" }
        ],
        "surface": [],
        "cave": [],
        "exploration": [
          { "locationId": "aaron_halo", "location": "Aaron Halo", "system": "Stanton",
            "rankPct": 100, "rank": 1, "n": 3, "concentration": 0.08,
            "tier": 4, "tierName": "Notable" }
        ]
      },
      "attribution": "Data from celd.space / Strata Mining Tools"
    }"#;
    let r: OreLocationsResponse = serde_json::from_str(json).unwrap();
    assert_eq!(r.ore.id, "beryl_raw");
    assert_eq!(r.sort, "concentration");
    let legend: BTreeMap<String, String> = r.tier_legend.clone().unwrap();
    assert_eq!(legend.get("6").map(String::as_str), Some("Pure"));
    assert_eq!(r.results.space.len(), 1);
    assert_eq!(r.results.space[0].rank_pct, 100);
    assert_eq!(r.results.space[0].tier_name, "Pure");
    assert_eq!(r.results.exploration[0].tier, 4);
}

/// Fehlende Gruppe → leeres Vec (serde default), keine Deserialisierungsfehler.
#[test]
fn ore_locations_missing_groups_default_empty() {
    let json = r#"{
      "ore": { "id": "ice_raw", "name": "Ice (Raw)" },
      "sort": "rank",
      "results": { "surface": [
        { "locationId": "stanton1b", "location": "Aberdeen", "system": "Stanton",
          "rankPct": 90, "rank": 1, "n": 10, "concentration": 0.2,
          "tier": 5, "tierName": "Major" }
      ] },
      "attribution": "Data from celd.space / Strata Mining Tools"
    }"#;
    let r: OreLocationsResponse = serde_json::from_str(json).unwrap();
    assert_eq!(r.results.surface.len(), 1);
    assert!(r.results.space.is_empty());
    assert!(r.results.cave.is_empty());
    assert!(r.results.exploration.is_empty());
    assert!(
        r.tier_legend.is_none(),
        "fehlende Legende → None (UI-Fallback)"
    );
}

// ---- Spec-Beispiel: GET /api/public/location-ores/{location} ---------------

#[test]
fn location_ores_spec_example_parses() {
    let json = r#"{
      "location": { "id": "ita_01", "name": "Ita Belt Alpha", "system": "Pyro", "type": "belt" },
      "ores": [
        { "oreId": "beryl_raw", "name": "Beryl (Raw)",
          "concentration": 0.3511, "tier": 6, "tierName": "Pure",
          "scanSignature": 1350 },
        { "oreId": "iron_ore", "name": "Iron (Ore)",
          "concentration": 0.0, "tier": 1, "tierName": "Negligible",
          "scanSignature": null }
      ],
      "count": 8,
      "attribution": "Data from celd.space / Strata Mining Tools"
    }"#;
    let r: LocationOresResponse = serde_json::from_str(json).unwrap();
    assert_eq!(r.location.kind, "belt");
    assert_eq!(r.count, 8, "count ist ein eigenes Feld, nicht ores.len()");
    assert_eq!(r.ores[0].concentration, 0.3511);
    // Konzentration 0.0 + scanSignature null: BEIDES muss sauber halten.
    assert_eq!(r.ores[1].concentration, 0.0);
    assert!(r.ores[1].scan_signature.is_none());
}

// ---- Fehler-i18n-Keys -------------------------------------------------------

#[test]
fn provider_errors_map_to_strata_error_keys() {
    let rl = error_key(&provider::ProviderError::RateLimit {
        retry_after: Some(7),
    });
    let v: serde_json::Value = serde_json::from_str(&rl).unwrap();
    assert_eq!(v["error_key"], "error.strata.rate_limit");
    assert_eq!(v["params"]["retry_after"], 7);

    let http = error_key(&provider::ProviderError::Http {
        status: 401,
        body: "unauthorized".into(),
    });
    let v: serde_json::Value = serde_json::from_str(&http).unwrap();
    assert_eq!(v["error_key"], "error.strata.http");
    assert_eq!(v["params"]["status"], 401);

    // Keyed bleibt unverändert stehen (kein doppeltes Wrappen).
    let keyed = provider::ProviderError::Keyed {
        key: "error.strata.no_key".into(),
        params: serde_json::json!({}),
    };
    let s = error_key(&keyed);
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert_eq!(v["error_key"], "error.strata.no_key");
}

// ---- Keystore (connector_keystore-Muster) -----------------------------------

#[test]
fn keystore_stores_keys_by_ref() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("strata-key.json");
    keystore::set_key(&path, "default", Some("celd_test_1"));
    assert_eq!(
        keystore::resolve_key(&path, "default").as_deref(),
        Some("celd_test_1")
    );
    // Persistiert über Neuladen (Datei lesen).
    assert_eq!(
        keystore::resolve_key(&path, "default").as_deref(),
        Some("celd_test_1")
    );
    assert!(keystore::resolve_key(&path, "unbekannt").is_none());
    // Keystore-Datei NICHT in der Config (key_ref trennt Secret von Config).
    assert!(path.file_name().unwrap() == "strata-key.json");
}

// ---- Transport: Bearer-Pflicht + 429/Retry-After (wiremock, kein Live-API) --

#[tokio::test]
async fn http_sends_bearer_and_maps_429_retry_after() {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;

    // 429 mit Retry-After: korrekt gemappt, Header respektiert.
    Mock::given(method("GET"))
        .and(path("/api/public/ores"))
        .and(header("authorization", "Bearer celd_unit"))
        .respond_with(
            ResponseTemplate::new(429)
                .set_body_string(r#"{ "error": "Rate limit exceeded (120/60s)" }"#)
                .insert_header("Retry-After", "42"),
        )
        .mount(&server)
        .await;

    let client = strata_client::StrataClient::for_base_url(&server.uri(), "celd_unit").unwrap();
    let err = client.ores().await.expect_err("429 → error");
    match &err {
        provider::ProviderError::RateLimit { retry_after } => {
            assert_eq!(
                *retry_after,
                Some(42),
                "Retry-After-Header wird respektiert"
            );
        }
        other => panic!("expected RateLimit, got {other:?}"),
    }
    assert!(error_key(&err).contains("error.strata.rate_limit"));

    // Fehlender/leerer Key → Config-Fehler VOR dem ersten Request (Bearer ist
    // Pflicht für jeden Endpoint).
    assert!(strata_client::StrataClient::for_base_url(&server.uri(), "  ").is_err());
    assert!(strata_client::StrataClient::for_base_url(&server.uri(), "").is_err());
}
