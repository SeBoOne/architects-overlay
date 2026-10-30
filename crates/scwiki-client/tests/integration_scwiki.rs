//! Integrationstest gegen einen gemockten SC-Wiki-Endpunkt (wiremock) —
//! gleiche Prämisse wie `integration_uex.rs`: deterministisch, keine
//! Live-API im CI-Pfad.

use scwiki_client::{ScWikiClient, category_label};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn vehicles_page(page: u32, last: u32, names: &[&str]) -> String {
    let items: Vec<String> = names
        .iter()
        .map(|n| {
            format!(
                r#"{{"name":"{n}","ports":[{{"name":"hardpoint_module","compatibleTypes":[{{"type":"Module","subTypes":[]}}],"sizes":{{"min":1,"max":1}}}}]}}"#
            )
        })
        .collect();
    format!(
        r#"{{"data":[{}],"meta":{{"last_page":{last},"current_page":{page}}}}}"#,
        items.join(",")
    )
}

#[tokio::test]
async fn fetches_version_vehicles_paginated_and_items() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/game-versions/default"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"data":{"code":"4.10.2-LIVE.12881860","channel":"live","released_at":"2026-10-09T00:00:00+00:00"}}"#,
        ))
        .mount(&server)
        .await;

    // Zwei Fahrzeug-Seiten: Seite 1 Aurora Mk II, Seite 2 Hull E (ohne Ports).
    Mock::given(method("GET"))
        .and(path("/vehicles"))
        .and(query_param("page[number]", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(vehicles_page(
            1,
            2,
            &["Aurora Mk II", "C2 Hercules"],
        )))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/vehicles"))
        .and(query_param("page[number]", "2"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(
                r#"{"data":[{"name":"Hull E","ports":[]}],"meta":{"last_page":2}}"#,
            ),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/items"))
        .and(query_param("filter[type]", "Module"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"data":[
                {"name":"Aurora Mk II DM Module","type":"Module","subType":"UNDEFINED","size":1},
                {"name":"Aurora Mk II TS Module","type":"Module","subType":"UNDEFINED","size":1}
            ],"meta":{"last_page":1}}"#,
        ))
        .mount(&server)
        .await;

    let client = ScWikiClient::new(&server.uri())
        .unwrap()
        .with_min_interval(std::time::Duration::from_millis(1));

    let version = client.game_version_default().await.unwrap();
    assert_eq!(version.code, "4.10.2-LIVE.12881860");

    let vehicles = client.vehicles().await.unwrap();
    // Hull E (leere Ports) kommt mit — der Caller trennt mit/ohne Ports.
    assert_eq!(
        vehicles.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(),
        vec!["Aurora Mk II", "C2 Hercules", "Hull E"]
    );

    let items = client.items_by_types(&["Module"]).await.unwrap();
    assert_eq!(items.len(), 2);

    let with_ports: Vec<_> = vehicles
        .into_iter()
        .filter(|v| !v.ports.is_empty())
        .collect();
    let rows = scwiki_client::build_compat(&with_ports, &items);
    assert_eq!(rows.len(), 2);
    let dm = rows.iter().find(|r| r.module_name.contains("DM")).unwrap();
    assert_eq!(dm.category, category_label("Module"));
    assert_eq!(
        dm.ships,
        vec!["Aurora Mk II".to_string(), "C2 Hercules".to_string()]
    );
}

#[tokio::test]
async fn http_error_maps_to_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/game-versions/default"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let client = ScWikiClient::new(&server.uri())
        .unwrap()
        .with_min_interval(std::time::Duration::from_millis(1));
    match client.game_version_default().await {
        Err(scwiki_client::ScWikiError::Status(503)) => {}
        other => panic!("expected Status(503), got {other:?}"),
    }
}

/// SC-Wiki-Quirk (live belegt): Felder kommen teils EXPLIZIT als `null` —
/// `#[serde(default)]` deckt nur FEHLENDE Felder ab (Serde-Regel). Regression
/// für `compatible_types: null` / `subTypes: null` / `subType: null`.
#[tokio::test]
async fn null_fields_do_not_break_parsing() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/vehicles"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"data":[{"name":"Odd Ship","ports":[
                {"name":"p1","compatibleTypes":null,"sizes":{"min":1,"max":2}},
                {"name":"p2","compatibleTypes":[{"type":"Shield","subTypes":null}],"sizes":null}
            ]}],"meta":{"last_page":1}}"#,
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/items"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"data":[{"name":"Null Item","type":"Shield","subType":null,"size":1}],"meta":{"last_page":1}}"#,
        ))
        .mount(&server)
        .await;

    let client = ScWikiClient::new(&server.uri())
        .unwrap()
        .with_min_interval(std::time::Duration::from_millis(1));
    let vehicles = client.vehicles().await.unwrap();
    assert_eq!(vehicles.len(), 1);
    // Port ohne compatible_types (null → leer) wird als dekorativer Slot
    // übersprungen — nur p2 (Shield) bleibt.
    assert_eq!(vehicles[0].ports.len(), 1);
    assert_eq!(vehicles[0].ports[0].compatible[0].0, "Shield");
    let items = client.items_by_types(&["Shield"]).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].sub_type, "");
}
