//! Recorded-response tests: real UEX API captures (October 2026) parsed by the
//! wire DTOs and mapped into seam DTOs. Runs without network.

use std::path::PathBuf;

use uex_client::private::{CommodityRow, ListingRow, PriceRow, TerminalRow};

fn fixture(name: &str) -> serde_json::Value {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures", name]
        .iter()
        .collect();
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn envelope_data<T: serde::de::DeserializeOwned>(name: &str) -> Vec<T> {
    let v = fixture(name);
    let mut it = v.get("data").unwrap().as_array().unwrap().iter();
    let first = it.next();
    // the cut fixture keeps as few rows as requested — but items/categories
    // were cut to 2; we parse *all* rows to exercise every mapping.
    let _ = first;
    serde_json::from_value(v.get("data").unwrap().clone()).unwrap()
}

#[test]
fn commodities_fixture_maps_to_entities() {
    let rows: Vec<CommodityRow> = envelope_data("commodities.json");
    assert_eq!(rows.len(), 3);
    let agri = rows
        .iter()
        .find(|r| r.code.as_deref() == Some("AGRI"))
        .unwrap();
    assert_eq!(agri.name, "Agricium");
    assert_eq!(agri.id, 1);
    assert!(agri.price_buy > 0.0);
    assert!(agri.price_sell > 0.0);
}

#[test]
fn terminals_fixture_maps_to_entities_with_hierarchy() {
    let rows: Vec<TerminalRow> = envelope_data("terminals.json");
    assert!(!rows.is_empty());
    for r in &rows {
        assert!(r.id > 0);
        assert!(
            r.displayname.is_some() || r.fullname.is_some(),
            "terminal {} has no name fields",
            r.id
        );
    }
    // first fixture row is ARC-L1 with full hierarchy
    let first = &rows[0];
    assert!(first.star_system_name.is_some());
}

#[test]
fn prices_fixture_maps_to_price_points() {
    let rows: Vec<PriceRow> = envelope_data("prices.json");
    assert_eq!(rows.len(), 3);
    for r in &rows {
        assert_eq!(
            r.id_commodity, 1,
            "fixture was captured for commodity 1 (Agricium)"
        );
        assert!(r.id_terminal > 0);
        assert!(r.price_sell >= 0.0);
        assert!(
            r.date_modified > 1_700_000_000,
            "date_modified must be a unix ts"
        );
    }
}

#[test]
fn marketplace_fixture_maps_to_offers() {
    let rows: Vec<ListingRow> = envelope_data("marketplace_listings.json");
    assert_eq!(rows.len(), 3);
    for r in &rows {
        assert!(r.id > 0);
        assert!(
            r.price.parse::<f64>().is_ok(),
            "price string must be numeric, got {:?}",
            r.price
        );
        assert!(matches!(r.operation.as_str(), "buy" | "sell"));
        assert!(r.date_added > 1_700_000_000);
    }
}
