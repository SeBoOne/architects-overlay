//! M7-K1 DTO-Contract-Tests: Deserialisierung gegen die Spec-Examples der
//! Strata Public API (docs/MINING-MODE-PLAN.md, strata.celd.space/strata-api.md).
//! KEIN Live-API-Call — die Fixtures sind die Spec-Beispiele; Transportfälle
//! (Bearer-Header, 429+Retry-After) laufen gegen wiremock.

use std::collections::BTreeMap;

use strata_client::{
    LocationOresResponse, LocationRow, LocationsResponse, OreLocationsResponse, OresResponse,
    error_key, keystore,
};

// ---- Spec-Beispiel: GET /api/public/ores ----------------------------------

#[test]
fn ores_spec_example_parses() {
    // M7-F1: Live-Contract (Fixtures-live) — `tier` ist String-Rarity, NICHT
    // die i64-Zahl aus dem (stalen) Spec-Beispiel. instability/resistance
    // sind unbeschränkte f64 (350! / negativ kommen live vor).
    let json = r#"{
      "ores": [
        { "id": "beryl_raw", "name": "Beryl (Raw)", "slug": "beryl",
          "tier": "uncommon", "category": "ship", "scanSignature": 1350,
          "instability": 0.05, "resistance": 0.1 },
        { "id": "agricium_ore", "name": "Agricium (Ore)", "slug": "agricium",
          "tier": "uncommon", "category": "ship", "scanSignature": 3885,
          "instability": 350, "resistance": -0.4 }
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
    assert_eq!(
        ore.tier.as_deref(),
        Some("uncommon"),
        "tier = Rarity-String"
    );
    assert_eq!(ore.scan_signature, Some(1350));
    assert_eq!(ore.instability, Some(0.05));
    // Unbeschränkte Werte (live verifiziert) ohne Clamp durchgereicht:
    assert_eq!(
        r.ores[1].instability,
        Some(350.0),
        "350 ist live-Realität — kein Clamp"
    );
    assert_eq!(
        r.ores[1].resistance,
        Some(-0.4),
        "negativ ist live-Realität — kein Clamp"
    );
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

// ---- M7-F1: LIVE-Fixtures (echte celd.space-Responses, 2026-11-17) --------
//
// Die Live-Responses sind der Contract — die OpenAPI-Spec-Beispiele sind
// STALE (ores.tier ist live String-Rarity, instability/resistance
// unbeschränkt, system kann "None" sein). Alle 4 Endpunkte müssen gegen
// die echten Payloads deserialisieren.

#[test]
fn fixtures_live_ores_parse() {
    let raw = include_str!("fixtures-live/ores.live.json");
    let r: OresResponse = serde_json::from_str(raw).expect("ores.live.json muss deserialisieren");
    assert!(r.count > 0);
    assert_eq!(r.ores.len() as u64, r.count, "count = Anzahl der Erze");
    // Rarity-Strings statt Zahlen — der User-Bug („expected i64“) ist damit
    // historisch:
    let rarities: Vec<&str> = r.ores.iter().filter_map(|o| o.tier.as_deref()).collect();
    assert!(
        !rarities.is_empty(),
        "live mindestens ein Rarity-String vorhanden"
    );
    assert!(
        rarities
            .iter()
            .all(|t| matches!(*t, "common" | "uncommon" | "rare" | "epic" | "legendary")),
        "Rarity-Vokabular: {rarities:?}"
    );
    // Unbeschränkte Instability/Resistance bleiben unclamp't erhalten:
    assert!(
        r.ores
            .iter()
            .any(|o| o.instability.is_some_and(|v| v > 1.0)),
        "live existiert instability > 1.0 (Spec-Range war falsch)"
    );
    assert!(
        r.ores.iter().any(|o| o.resistance.is_some_and(|v| v < 0.0)),
        "live existiert resistance < 0.0"
    );
    assert!(!r.attribution.is_empty(), "Attribution Pflicht");
}

#[test]
fn fixtures_live_locations_parse() {
    let raw = include_str!("fixtures-live/locations.live.json");
    let r: LocationsResponse =
        serde_json::from_str(raw).expect("locations.live.json muss deserialisieren");
    assert!(r.count > 0);
    assert_eq!(r.locations.len() as u64, r.count);
    // system kann null sein (Dungeons ohne System-Zuordnung); type-Umfang
    // geht über die Spec-Beispiele hinaus — beides robust gehalten:
    assert!(
        r.locations.iter().any(|l| l.system.is_none()),
        "live existiert system=null (Dungeons)"
    );
    let types: Vec<&str> = r.locations.iter().map(|l| l.kind.as_str()).collect();
    for expected in [
        "asteroid_field",
        "asteroid_cluster",
        "station",
        "lagrange_point",
        "moon",
        "planet",
        "dungeon",
    ] {
        assert!(
            types.contains(&expected),
            "live-Typ {expected} muss vorkommen"
        );
    }
    assert!(!r.attribution.is_empty());
}

#[test]
fn fixtures_live_ore_locations_parse() {
    let raw = include_str!("fixtures-live/ore-locations.gold_ore.live.json");
    let r: OreLocationsResponse =
        serde_json::from_str(raw).expect("ore-locations.live.json muss deserialisieren");
    assert_eq!(r.ore.id, "gold_ore");
    assert_eq!(r.sort, "rank");
    // Spec-konform: Tier-Zahl 1–6 + tierName bleiben i64/String.
    let rows: Vec<&LocationRow> = [
        r.results.space.iter(),
        r.results.surface.iter(),
        r.results.cave.iter(),
        r.results.exploration.iter(),
    ]
    .into_iter()
    .flatten()
    .collect();
    assert!(
        rows.iter().all(|row| (1..=6).contains(&row.tier)),
        "ore-locations-Tier bleibt 1–6 (Spec korrekt)"
    );
    assert!(rows.iter().all(|row| !row.tier_name.is_empty()));
    assert!(!r.attribution.is_empty());
}

#[test]
fn fixtures_live_location_ores_parse() {
    let raw = include_str!("fixtures-live/location-ores.aaronhalo.live.json");
    let r: LocationOresResponse =
        serde_json::from_str(raw).expect("location-ores.live.json muss deserialisieren");
    assert_eq!(r.location.id, "aaronhalo");
    assert!(r.count > 0);
    assert_eq!(r.ores.len() as u64, r.count);
    assert!(
        r.ores.iter().all(|o| (1..=6).contains(&o.tier)),
        "location-ores-Tier bleibt 1–6 (Spec korrekt)"
    );
    assert!(!r.attribution.is_empty());
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

// ---- M8-K1 (t_a35b6344): Crafting-DTOs --------------------------------------
// Einheiten-Regel (dto.rs, verbindlich): SCU vs gems vs items — nie mischen;
// gems = Stück, CRAFT_GEMS_PER_SCU = 1000 gems = 1 SCU.

#[test]
fn craft_units_wire_vocabulary() {
    // Wire-Strings exakt: "SCU" | "gems" | "items" — kein anderes Vokabular.
    for (wire, expected) in [
        ("SCU", strata_client::CraftUnit::Scu),
        ("gems", strata_client::CraftUnit::Gems),
        ("items", strata_client::CraftUnit::Items),
    ] {
        let parsed: strata_client::CraftUnit =
            serde_json::from_value(serde_json::json!(wire)).expect("CraftUnit-Wire-String");
        assert_eq!(parsed, expected, "CraftUnit {wire} muss hin und her passen");
        assert_eq!(
            serde_json::to_value(parsed).unwrap(),
            serde_json::json!(wire),
            "Serialize bleibt beim Wire-String"
        );
    }
}

#[test]
fn craft_unit_volume_rule() {
    // Nur SCU ist Volumen; gems/items sind Stück-Mengen — die UI darf
    // Dezimalmengen nur über `is_volume()` als Volumen lesen (Einheiten-Regel).
    assert!(strata_client::CraftUnit::Scu.is_volume());
    assert!(!strata_client::CraftUnit::Gems.is_volume());
    assert!(!strata_client::CraftUnit::Items.is_volume());
    // 1000 gems = 1 SCU — Konstante, damit die Craft-UI nicht mit Magie rechnet.
    assert_eq!(strata_client::CRAFT_GEMS_PER_SCU, 1_000);
}

#[test]
fn items_spec_example_parses() {
    let json = r#"{
      "items": [
        { "id": "02a7f68a-5bdd-4887-a256-20c985a65bda",
          "name": "10-Series Greatsword Cannon",
          "slug": "10-series-greatsword-cannon",
          "category": "Ship Weapon" }
      ],
      "count": 1,
      "attribution": "Data from celd.space / Strata Mining Tools"
    }"#;
    let r: strata_client::ItemsResponse = serde_json::from_str(json).unwrap();
    assert_eq!(r.count, 1);
    let item = &r.items[0];
    assert_eq!(
        item.category.as_deref(),
        Some("Ship Weapon"),
        "category Option-haltig, aber gesetzt"
    );
    assert!(!r.attribution.is_empty());
}

#[test]
fn craft_cost_spec_example_parses() {
    // Live-Contract (Fixtures 2026-11-19): item-Header OHNE slug,
    // oreId null-fähig bei gems (Hadanite), minQuality Zahl (0 ist gültig).
    let json = r#"{
      "item": { "id": "6b1fb5e8-f9ec-49bc-befb-c81b7c08c5ea",
                "name": "A03 Sniper Rifle", "category": "FPS Weapon" },
      "craftTime": 180,
      "costs": [
        { "material": "Iron", "oreId": "iron_ore", "quantity": 0.03,
          "unit": "SCU", "minQuality": 0 },
        { "material": "Hadanite", "oreId": null, "quantity": 1,
          "unit": "gems", "minQuality": 0 },
        { "material": "Bewer", "oreId": "bewer_raw", "quantity": 2,
          "unit": "items", "minQuality": null }
      ],
      "attribution": "Data from celd.space / Strata Mining Tools"
    }"#;
    let r: strata_client::CraftCostResponse = serde_json::from_str(json).unwrap();
    assert_eq!(r.craft_time, 180);
    assert_eq!(r.costs.len(), 3);
    let iron = &r.costs[0];
    assert_eq!(iron.unit, strata_client::CraftUnit::Scu);
    assert_eq!(
        iron.min_quality,
        Some(0.0),
        "minQuality 0 ist gültige Daten (null-check, nie truthiness)"
    );
    let hadanite = &r.costs[1];
    assert_eq!(hadanite.unit, strata_client::CraftUnit::Gems);
    assert_eq!(
        hadanite.ore_id, None,
        "oreId null bei gems — null-fähig, nie truthiness"
    );
    let bewer = &r.costs[2];
    assert_eq!(bewer.unit, strata_client::CraftUnit::Items);
    assert_eq!(
        bewer.min_quality, None,
        "minQuality null = keine Anforderung"
    );
}

#[test]
fn fixtures_live_items_parse() {
    let raw = include_str!("fixtures-live/items.live.json");
    let r: strata_client::ItemsResponse =
        serde_json::from_str(raw).expect("items.live.json muss deserialisieren");
    assert!(r.count > 0);
    assert_eq!(r.items.len() as u64, r.count, "count = Anzahl der Items");
    // Kategorien sind freie Strings — die Liste darf kein hartkodiertes
    // Vokabular voraussetzen (M7-Präzedenz locations[].type).
    let cats: Vec<String> = r.items.iter().filter_map(|i| i.category.clone()).collect();
    assert!(cats.iter().any(|c| c == "Ship Weapon"));
    assert!(cats.iter().any(|c| c == "FPS Weapon"));
    // IDs sind UUIDs und Slug/Name immer gesetzt — id vor Slug als Key.
    let greatsword = r
        .items
        .iter()
        .find(|i| i.slug == "10-series-greatsword-cannon")
        .expect("Fixtures-Beispiel-Item");
    assert!(!greatsword.id.is_empty());
    assert!(!r.attribution.is_empty());
}

#[test]
fn fixtures_live_craft_costs_parse() {
    // Schiff-Item: ausschließlich SCU-Zeilen (Einheiten-Regel).
    let raw = include_str!("fixtures-live/craft-cost.10-series-greatsword-cannon.live.json");
    let r: strata_client::CraftCostResponse =
        serde_json::from_str(raw).expect("greatsword-craft-cost muss deserialisieren");
    assert!(r.craft_time > 0);
    assert!(!r.costs.is_empty());
    assert!(!r.attribution.is_empty());
    assert!(
        r.costs
            .iter()
            .all(|c| c.unit == strata_client::CraftUnit::Scu),
        "Schiff-Rezept: ausschließlich SCU (Einheiten-Regel)"
    );

    // FPS-Item: gems-Zeilen mit oreId null neben SCU-Zeilen.
    let raw = include_str!("fixtures-live/craft-cost.a03-sniper-rifle.live.json");
    let r: strata_client::CraftCostResponse =
        serde_json::from_str(raw).expect("a03-craft-cost muss deserialisieren");
    assert!(r.craft_time > 0);
    assert!(!r.costs.is_empty());
    assert!(
        r.costs
            .iter()
            .any(|c| c.unit == strata_client::CraftUnit::Gems),
        "FPS-Rezept: gems-Zeilen kommen live vor"
    );
    assert!(
        r.costs
            .iter()
            .any(|c| c.unit == strata_client::CraftUnit::Gems && c.ore_id.is_none()),
        "gems mit oreId null (live: Hadanite)"
    );
    // minQuality 0 ist gültige Daten — bleibt als Some(0.0) erhalten.
    assert!(
        r.costs.iter().any(|c| c.min_quality == Some(0.0)),
        "minQuality 0 wird als Some(0.0) durchgereicht, nie truthiness"
    );
}
