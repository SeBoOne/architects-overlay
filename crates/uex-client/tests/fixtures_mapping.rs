//! Recorded-response tests: real UEX API captures (October 2026) parsed by the
//! wire DTOs and mapped into seam DTOs. Runs without network.

use std::path::PathBuf;

use uex_client::private::{CommodityRow, ItemPriceRow, ListingRow, PriceRow, TerminalRow};

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
            r.displayname.is_some() || r.fullname.is_some() || r.name.is_some(),
            "terminal {} has no name fields",
            r.id
        );
    }
    // first fixture row is ARC-L1 with full hierarchy
    let first = &rows[0];
    assert!(first.star_system_name.is_some());
}

#[test]
fn terminals_carry_all_name_fields() {
    let rows: Vec<TerminalRow> = envelope_data("terminals.json");
    // the Checkmate commodity terminal: name ("Admin - Checkmate") AND
    // fullname ("Commodity Shop - Admin - Checkmate") are both present — the
    // display mapping must prefer fullname, never the bare displayname.
    let commodity = rows.iter().find(|r| r.id == 436).unwrap();
    assert_eq!(commodity.name.as_deref(), Some("Admin - Checkmate"));
    assert_eq!(
        commodity.fullname.as_deref(),
        Some("Commodity Shop - Admin - Checkmate")
    );
    assert_eq!(commodity.displayname.as_deref(), Some("Checkmate"));
}

#[test]
fn items_fixture_carries_is_commodity_flag() {
    let rows: Vec<uex_client::private::ItemRow> = envelope_data("items.json");
    let quant = rows.iter().find(|r| r.id == 4745).unwrap();
    assert_eq!(quant.name, "Quantainium");
    assert_eq!(quant.is_commodity, 1, "commodity items must be flagged");
    let backpack = rows.iter().find(|r| r.id == 174).unwrap();
    assert_eq!(backpack.is_commodity, 0);
    // missing flag → default 0 (legacy/partial rows must not fail)
    let parsed: uex_client::private::ItemRow =
        serde_json::from_str(r#"{"id": 1, "name": "x"}"#).unwrap();
    assert_eq!(parsed.is_commodity, 0);
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
fn item_prices_fixture_with_null_sell_parses() {
    // QA runde 2 (Punkt 3/7): the live API sends `"price_sell": null` on
    // rows where shops don't buy back — measured on id_item=465 (1/16 rows)
    // and id_terminal=437 (22/90 rows). This is a REGRESSION guard: without
    // null-tolerant fields, parsing this fixture fails with
    // `invalid type: null, expected f64` and takes the whole table down.
    let rows: Vec<ItemPriceRow> = envelope_data("item_prices_465_null.json");
    assert_eq!(rows.len(), 2);
    let null_row = rows.iter().find(|r| r.id == 1).unwrap();
    assert_eq!(null_row.price_buy, 295.0);
    assert_eq!(null_row.price_sell, 0.0, "explicit null must map to 0");
    let priced_row = rows.iter().find(|r| r.id == 2).unwrap();
    assert_eq!(priced_row.price_sell, 350.0, "real values must survive");
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
