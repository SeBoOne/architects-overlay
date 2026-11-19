//! M8-K4 (t_72b27d99): Craft-Endpoints gegen ECHTE API-Responses
//! (Fixtures in `tests/fixtures/`, Live-Zug 2026-11-19). Wiremock wie
//! `integration_scwiki.rs` — deterministisch, kein Live-API im CI-Pfad.

use scwiki_client::{ATTRIBUTION_SCWIKI, Blueprint, CraftableItem, ScWikiClient, ScWikiError};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture vorhanden")
}

/// Fixture als Single-Page-Response (meta.last_page=1) — die echte Seite
/// bleibt Inhalt-für-Inhalt identisch, nur die Pagination wird auf eine
/// Seite gekappt, damit der Mock nicht endlos dieselbe Seite serviert.
fn single_page(name: &str) -> String {
    let mut v: serde_json::Value =
        serde_json::from_str(&fixture(name)).expect("fixture valid json");
    v["meta"]["last_page"] = serde_json::json!(1);
    v.to_string()
}

/// Gleiche Seite mit beliebiger last_page — der Mock serviert bei jedem
/// page[number] denselben Inhalt; der Client loopt bis page >= last_page
/// (oder bis der MAX_PAGES-Guard zuschlägt).
fn paginated_fixture(last_page: u32) -> String {
    let mut v: serde_json::Value =
        serde_json::from_str(&fixture("items_craftable_page1.json")).expect("fixture valid json");
    v["meta"]["last_page"] = serde_json::json!(last_page);
    v.to_string()
}

fn client(url: &str) -> ScWikiClient {
    ScWikiClient::new(url)
        .unwrap()
        // Tests mit minimalem Pacing — sonst warten 2 Requests 700 ms.
        .with_min_interval(std::time::Duration::from_millis(1))
}

#[tokio::test]
async fn craftable_items_parses_real_page() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/items"))
        .and(query_param("filter[is_craftable]", "1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(single_page("items_craftable_page1.json")),
        )
        .mount(&server)
        .await;

    let items: Vec<CraftableItem> = client(&server.uri())
        .craftable_items(None)
        .await
        .expect("parse ok");

    assert!(!items.is_empty());
    let gs = items
        .iter()
        .find(|i| i.name == "10-Series Greatsword Cannon")
        .expect("Greatsword in echter Seite");
    assert_eq!(gs.item_type, "WeaponGun");
    assert_eq!(gs.size, Some(2));
    assert_eq!(gs.grade.as_deref(), Some("A"));
    assert_eq!(gs.mass, Some(192.0));
    // description.de_DE live belegt:
    assert!(
        gs.description_de
            .as_deref()
            .unwrap_or_default()
            .contains("ballistische")
    );
    // Blueprint-UUID für die Detail-Auflösung:
    assert_eq!(
        gs.blueprint_uuids,
        vec!["b04aff42-d892-4ca9-991e-101c869a7caa".to_string()]
    );
    // vehicle_weapon{capacity,range,damage_per_shot,rpm,damages[]}:
    let vw = gs.vehicle_weapon.as_ref().expect("vehicle_weapon");
    assert_eq!(vw.capacity, Some(492.0));
    assert_eq!(vw.range, Some(2703.0));
    assert_eq!(vw.damage_per_shot, Some(73.0));
    assert_eq!(vw.rpm, Some(250.0));
    assert!(
        vw.damages
            .iter()
            .any(|d| d.damage_type == "impact" && d.damage == Some(72.99))
    );
    // ammunition live belegt (Größe/Munitions-Kapazität):
    let ammo = gs.ammunition.as_ref().expect("ammunition");
    assert!(ammo.capacity.is_some());
    assert!(ammo.speed.is_some());
}

#[tokio::test]
async fn craftable_items_name_cont_filter_is_sent() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/items"))
        .and(query_param("filter[is_craftable]", "1"))
        .and(query_param("filter[name_cont]", "Greatsword"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(single_page("items_name_cont.json")),
        )
        .mount(&server)
        .await;

    let items = client(&server.uri())
        .craftable_items(Some("Greatsword"))
        .await
        .expect("parse ok");
    assert!(
        items
            .iter()
            .any(|i| i.name == "10-Series Greatsword Cannon")
    );
    // Kein zweites Mock: andere Query-Kombis wären 404 → Fehler. Test
    // bestätigt, dass GENAU die erwartete Query geschickt wurde.
}

#[tokio::test]
async fn blueprint_parses_real_detail() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/blueprints/b04aff42-d892-4ca9-991e-101c869a7caa"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(single_page("blueprint_greatsword.json")),
        )
        .mount(&server)
        .await;

    let bp: Blueprint = client(&server.uri())
        .blueprint("b04aff42-d892-4ca9-991e-101c869a7caa")
        .await
        .expect("parse ok");

    assert_eq!(bp.output_name, "10-Series Greatsword Cannon");
    assert_eq!(bp.craft_time_seconds, Some(960));
    assert_eq!(bp.craft_time_label.as_deref(), Some("16 minutes"));
    assert!(!bp.is_available_by_default);
    assert_eq!(bp.game_version.as_deref(), Some("4.10.3-LIVE.13165946"));
    assert_eq!(
        bp.web_url.as_deref(),
        Some("https://api.star-citizen.wiki/blueprints/10-series-greatsword-cannon")
    );

    // ingredients[{name,kind,quantity_scu}]:
    assert_eq!(bp.ingredients.len(), 3);
    let iron = bp.ingredients.iter().find(|i| i.name == "Iron").unwrap();
    assert_eq!(iron.kind, "resource");
    assert_eq!(iron.quantity_scu, Some(0.64));

    // dismantle_returns:
    assert!(
        bp.dismantle_returns
            .iter()
            .any(|d| d.name == "Iron" && d.quantity_scu == Some(0.32))
    );

    // tiers[].requirements: nested groups FRAME/CYCLER/BARREL → resources
    // mit min_quality + modifiers[] mit quality/modifier-Range.
    assert_eq!(bp.tiers.len(), 1);
    let groups = &bp.tiers[0].requirements.children;
    for (key, resource) in [
        ("FRAME", "Iron"),
        ("CYCLER", "Riccite"),
        ("BARREL", "Titanium"),
    ] {
        let g = groups
            .iter()
            .find(|c| c.group.as_ref().map(|g| g.key.as_deref()) == Some(Some(key)))
            .unwrap_or_else(|| panic!("Gruppe {key} fehlt"));
        let g = g.group.as_ref().unwrap();
        assert_eq!(g.kind.as_deref(), Some("group"));
        let child = &g.children[0];
        assert_eq!(child.name, resource);
        assert_eq!(child.min_quality, Some(1.0));
        let m = &g.modifiers[0];
        assert!(!m.property_key.is_empty());
        assert_eq!(m.quality_min, Some(0.0));
        assert_eq!(m.quality_max, Some(1000.0));
        assert!(m.at_min_quality.is_some());
        assert!(m.at_max_quality.is_some());
        assert_eq!(m.value_range_type.as_deref(), Some("linear"));
    }
    // Live-Beleg: FRAME-Modifier ist Integrity (health_maxhealth, 0.9→1.1).
    let frame = groups[0].group.as_ref().unwrap();
    assert_eq!(frame.modifiers[0].property_key, "health_maxhealth");
    assert_eq!(frame.modifiers[0].label, "Integrity");
    assert_eq!(frame.modifiers[0].at_min_quality, Some(0.9));
    assert_eq!(frame.modifiers[0].at_max_quality, Some(1.1));

    // summary_properties + unlocking_missions_grouped:
    assert!(
        bp.summary_properties
            .iter()
            .any(|p| p.property_key == "weapon_damage"
                && p.label == "Impact Force"
                && p.better_when.as_deref() == Some("higher"))
    );
    assert!(!bp.unlocking_missions_grouped.is_empty());
    let guaranteed = bp
        .unlocking_missions_grouped
        .iter()
        .find(|g| g.label == "Guaranteed")
        .expect("Guaranteed-Gruppe");
    assert!(!guaranteed.missions.is_empty());
    let m = &guaranteed.missions[0];
    assert!(!m.title.is_empty());
    assert!(m.reward_scope.is_some());
    assert!(
        m.web_url
            .as_deref()
            .unwrap_or_default()
            .starts_with("https://")
    );
}

/// M8-K5 (t_03eafbfb): `aspects`-Envelope → flache Aspekt-Liste, und das
/// Serialize-JSON exakt im K6-Contract (`src/lib/quality-calc.ts`):
/// snake_case (`property_key`, `min_quality`, `quantity_scu`,
/// `value_range_type`, `at_min_quality`/`at_max_quality`), nested
/// `{min, max}` / `{at_min_quality, at_max_quality}`.
#[tokio::test]
async fn aspects_parse_real_fixture_and_serialize_k6_shape() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/blueprints/b04aff42-d892-4ca9-991e-101c869a7caa"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(single_page("blueprint_greatsword.json")),
        )
        .mount(&server)
        .await;

    let bp: Blueprint = client(&server.uri())
        .blueprint("b04aff42-d892-4ca9-991e-101c869a7caa")
        .await
        .expect("parse ok");

    assert_eq!(bp.aspects.len(), 3, "FRAME/CYCLER/BARREL");
    let frame = bp.aspects.iter().find(|a| a.key == "FRAME").unwrap();
    assert_eq!(frame.name, "Frame");
    assert_eq!(frame.required_count, Some(1));
    let input = frame.input.as_ref().expect("input");
    assert_eq!(input.name, "Iron");
    assert_eq!(input.min_quality, Some(1.0));
    assert_eq!(input.quantity_scu, Some(0.64));
    let m = &frame.modifiers[0];
    assert_eq!(m.property_key, "health_maxhealth");
    assert_eq!(m.label, "Integrity");
    assert_eq!(m.better_when.as_deref(), Some("higher"));
    assert_eq!(m.value_range_type.as_deref(), Some("linear"));
    assert_eq!(
        m.quality_range.as_ref().unwrap(),
        &scwiki_client::QualityRange {
            min: Some(0.0),
            max: Some(1000.0)
        }
    );
    assert_eq!(
        m.modifier_range.as_ref().unwrap(),
        &scwiki_client::ModifierRange {
            at_min_quality: Some(0.9),
            at_max_quality: Some(1.1)
        }
    );

    // Serialize-Contract: JSON-Keys exakt wie das K6-Frontend sie liest.
    let json = serde_json::to_value(&bp.aspects).unwrap();
    let frame_json = &json[0];
    for key in ["key", "name", "required_count", "input", "modifiers"] {
        assert!(frame_json.get(key).is_some(), "{key} fehlt: {frame_json}");
    }
    let mod_json = &frame_json["modifiers"][0];
    for key in [
        "property_key",
        "label",
        "better_when",
        "quality_range",
        "modifier_range",
        "value_range_type",
    ] {
        assert!(mod_json.get(key).is_some(), "{key} fehlt: {mod_json}");
    }
    assert!(mod_json["quality_range"]["min"].is_number());
    assert!(mod_json["modifier_range"]["at_min_quality"].is_number());
    assert!(frame_json["input"]["min_quality"].is_number());
    assert!(frame_json["input"]["quantity_scu"].is_number());
    // Roundtrip (Cache-Readback-Pfad): Deserialize liefert dieselben Daten.
    let rt: Vec<scwiki_client::BlueprintAspect> =
        serde_json::from_value(json.clone()).expect("roundtrip");
    assert_eq!(rt, bp.aspects);
}

#[test]
fn attribution_const_is_present() {
    assert_eq!(ATTRIBUTION_SCWIKI, "Data from star-citizen.wiki");
}

/// M8-K5-Review-Blocker-Regression (Live-Beleg 2026-11-19): Die echte API
/// meldet meta.last_page=33 bei 1630 craftable Items (per_page=50). Ein
/// Mock mit last_page=33 muss VOLL durchlaufen (kein TooManyPages) — der
/// alte MAX_PAGES=30-Kappung hat hier hart fehlgeschlagen.
#[tokio::test]
async fn craftable_items_33_pages_no_longer_capped() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/items"))
        .and(query_param("filter[is_craftable]", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(paginated_fixture(33)))
        .mount(&server)
        .await;

    let items: Vec<CraftableItem> = client(&server.uri())
        .craftable_items(None)
        .await
        .expect("33 Seiten müssen durchlaufen (Live-Realität)");

    assert!(!items.is_empty());
}

/// Guard-Regression: Ein hart-bounded Limit bleibt — last_page=60 (weit
/// über MAX_PAGES=50) schlägt mit TooManyPages(50) fehl statt endlos zu
/// loopen.
#[tokio::test]
async fn craftable_items_too_many_pages_still_bounded() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/items"))
        .and(query_param("filter[is_craftable]", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(paginated_fixture(60)))
        .mount(&server)
        .await;

    let err = client(&server.uri())
        .craftable_items(None)
        .await
        .expect_err("über Limit muss TooManyPages liefern");
    match err {
        ScWikiError::TooManyPages(50) => {}
        other => panic!("TooManyPages(50) erwartet, got: {other:?}"),
    }
}
