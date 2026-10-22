//! Wire DTOs for the UEX API v2.0 (see research doc / api.uexcorp.uk).
//!
//! These structs mirror the *actual* JSON shapes captured from the live API —
//! every field type was verified against real responses (October 2026), not
//! guessed. Notable quirks handled here:
//!
//! * responses always carry `{"status": …, "data": …}`; status `"ok"` OR the
//!   API's string-coded error names (`requests_limit_reached`, …);
//! * marketplace `price` arrives as a **string** (`"400000"`), parsed to f64;
//! * `/items` requires `id_category` (or `id_company`/`uuid`) — no bulk list;
//! * optional fields default via `#[serde(default)]`; a missing price means
//!   "not traded" (0), identical to UEX's explicit `0`.

use serde::Deserialize;

/// Deserialize a JSON number that may be `null` (or absent) into `f64`,
/// mapping null/absent to `0.0`. UEX sends explicit `null` ("not traded",
/// "price unknown") in price rows — `#[serde(default)]` alone does NOT cover
/// an explicit null, only a *missing* field.
pub(crate) fn de_f64_null_to_zero<'de, D>(d: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<f64>::deserialize(d)?.unwrap_or(0.0))
}

/// Same for `u8` (inventory status).
pub(crate) fn de_u8_null_to_zero<'de, D>(d: D) -> Result<u8, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<u8>::deserialize(d)?.unwrap_or(0))
}

/// Same for `u16` (quality).
pub(crate) fn de_u16_null_to_zero<'de, D>(d: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<u16>::deserialize(d)?.unwrap_or(0))
}

/// Same for `u64` (unix timestamps).
pub(crate) fn de_u64_null_to_zero<'de, D>(d: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<u64>::deserialize(d)?.unwrap_or(0))
}

/// Generic UEX response envelope.
#[derive(Debug, Deserialize)]
pub struct Envelope<T> {
    pub status: String,
    #[serde(default)]
    pub data: Option<T>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub http_code: Option<u16>,
}

/// `/commodities` row.
#[derive(Debug, Deserialize)]
pub struct CommodityRow {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub price_buy: f64,
    #[serde(default)]
    pub price_sell: f64,
}

/// `/terminals` row — carries the full location hierarchy as plain fields.
#[derive(Debug, Deserialize)]
pub struct TerminalRow {
    pub id: u64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub displayname: Option<String>,
    #[serde(default)]
    pub fullname: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    #[allow(non_snake_case)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub star_system_name: Option<String>,
    #[serde(default)]
    pub planet_name: Option<String>,
    #[serde(default)]
    pub moon_name: Option<String>,
    #[serde(default)]
    pub orbit_name: Option<String>,
    #[serde(default)]
    pub space_station_name: Option<String>,
    #[serde(default)]
    pub city_name: Option<String>,
    #[serde(default)]
    pub outpost_name: Option<String>,
    #[serde(default)]
    pub poi_name: Option<String>,
}

/// `/items` row (per category).
#[derive(Debug, Deserialize)]
pub struct ItemRow {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub section: Option<String>,
    #[serde(default)]
    pub company_name: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    /// 1 = the item is a trade commodity (also listed in `/commodities` via
    /// `id_item`) and has no regular shop/storage entry — the dedup
    /// criterion that keeps commodities from also appearing as items.
    #[serde(default)]
    pub is_commodity: u8,
}

/// `/categories` row (used to enumerate items).
#[derive(Debug, Deserialize)]
pub struct CategoryRow {
    pub id: u64,
}

/// `/commodities_prices` row.
#[derive(Debug, Deserialize)]
pub struct PriceRow {
    pub id_commodity: u64,
    pub id_terminal: u64,
    #[serde(default)]
    pub commodity_name: String,
    #[serde(default)]
    pub terminal_name: String,
    #[serde(default, deserialize_with = "de_f64_null_to_zero")]
    pub price_buy: f64,
    #[serde(default, deserialize_with = "de_f64_null_to_zero")]
    pub price_sell: f64,
    #[serde(default, deserialize_with = "de_f64_null_to_zero")]
    pub scu_buy_avg: f64,
    #[serde(default, deserialize_with = "de_f64_null_to_zero")]
    pub scu_sell_avg: f64,
    /// Inventory level, 1..=7; 0 = unknown / out of stock.
    #[serde(default, deserialize_with = "de_u8_null_to_zero")]
    pub status_buy: u8,
    #[serde(default, deserialize_with = "de_u8_null_to_zero")]
    pub status_sell: u8,
    #[serde(default, deserialize_with = "de_u16_null_to_zero")]
    pub quality: u16,
    /// Unix seconds of the last data update.
    #[serde(default, deserialize_with = "de_u64_null_to_zero")]
    pub date_modified: u64,
}

/// `/marketplace_listings` row.
#[derive(Debug, Deserialize)]
pub struct ListingRow {
    pub id: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub id_item: u64,
    #[serde(default)]
    pub id_category: u64,
    /// Star system the listing is placed in (0 = unknown) — resolved to a
    /// region name via `/star_systems`.
    #[serde(default)]
    pub id_star_system: u64,
    /// "buy" | "sell".
    #[serde(default)]
    pub operation: String,
    /// "item" | "service" | "contract".
    #[serde(default)]
    #[allow(non_snake_case)]
    pub r#type: String,
    /// The API sends the price as a STRING to preserve precision.
    #[serde(default)]
    pub price: String,
    #[serde(default)]
    pub currency: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub in_stock: u64,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub user_name: String,
    /// Quality on the source's scale (UEX: 0–1000); null when not set.
    #[serde(default)]
    pub quality: Option<u16>,
    /// The live API sends this sometimes as `null`, sometimes as empty string
    /// `""` — kept as String so both shapes deserialize. Not used in Phase 2.
    #[serde(default)]
    pub hours_expiration: Option<String>,
    #[serde(default)]
    pub date_added: u64,
    #[serde(default)]
    pub date_expiration: Option<u64>,
    /// Permalink slug on uexcorp.space (deep-link target).
    #[serde(default)]
    pub slug: String,
}

/// `/items_prices` row (per terminal or per item). Only `price_buy` carries
/// the terminal's purchase price (what the player pays); `price_sell` is 0
/// for items (shops don't buy items back). The `min/max/avg` variants and
/// durability/faction fields are omitted — they mirror the buy price.
#[derive(Debug, Deserialize)]
pub struct ItemPriceRow {
    pub id: u64,
    pub id_item: u64,
    pub id_terminal: u64,
    #[serde(default, deserialize_with = "de_f64_null_to_zero")]
    pub price_buy: f64,
    #[serde(default, deserialize_with = "de_f64_null_to_zero")]
    pub price_sell: f64,
    #[serde(default)]
    pub item_name: String,
    #[serde(default)]
    pub terminal_name: String,
    #[serde(default)]
    pub terminal_code: Option<String>,
    #[serde(default, deserialize_with = "de_u64_null_to_zero")]
    pub date_modified: u64,
}

/// `/star_systems` row — few rows, used to resolve listing regions.
#[derive(Debug, Deserialize)]
pub struct StarSystemRow {
    pub id: u64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub code: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commodity_row_from_live_sample() {
        let json = r#"{
            "id": 1, "id_parent": 2, "uuid": null,
            "name": "Agricium", "code": "AGRI", "kind": "Metal",
            "weight_scu": 1.2, "price_buy": 8631, "price_sell": 9725,
            "is_mineral": 1, "is_illegal": 0, "wiki": null
        }"#;
        let row: CommodityRow = serde_json::from_str(json).unwrap();
        assert_eq!(row.id, 1);
        assert_eq!(row.name, "Agricium");
        assert_eq!(row.code.as_deref(), Some("AGRI"));
        assert_eq!(row.price_buy, 8631.0);
    }

    #[test]
    fn price_row_defaults_missing_fields() {
        // Real responder omits optics we don't use; defaults must not fail.
        let json = r#"{
            "id": 4, "id_commodity": 1, "id_terminal": 12,
            "commodity_name": "Agricium", "terminal_name": "TDD - Area 18",
            "price_buy": 0, "price_sell": 10000,
            "scu_sell_avg": 301, "status_sell": 1,
            "date_modified": 1792341485
        }"#;
        let row: PriceRow = serde_json::from_str(json).unwrap();
        assert_eq!(row.status_buy, 0);
        assert_eq!(row.scu_buy_avg, 0.0);
        assert_eq!(row.quality, 0);
        assert_eq!(row.price_sell, 10000.0);
    }

    #[test]
    fn price_row_tolerates_explicit_null_fields() {
        // UEX sends explicit `null` for "not traded" prices and unknown
        // inventory (QA runde 2, Punkt 3/7). `#[serde(default)]` alone does
        // NOT cover an explicit null — only a *missing* field.
        let row: PriceRow = serde_json::from_str(
            r#"{
                "id_commodity": 1, "id_terminal": 12,
                "commodity_name": "Agricium", "terminal_name": "X",
                "price_buy": null, "price_sell": 4200,
                "scu_buy_avg": null, "scu_sell_avg": null,
                "status_buy": null, "status_sell": 3,
                "quality": null, "date_modified": null
            }"#,
        )
        .unwrap();
        assert_eq!(row.price_buy, 0.0, "null buy price must map to 0");
        assert_eq!(row.price_sell, 4200.0);
        assert_eq!(row.scu_buy_avg, 0.0);
        assert_eq!(row.scu_sell_avg, 0.0);
        assert_eq!(row.status_buy, 0);
        assert_eq!(row.status_sell, 3);
        assert_eq!(row.quality, 0);
        assert_eq!(row.date_modified, 0);
    }

    #[test]
    fn item_price_row_tolerates_explicit_null_price() {
        // /items_prices rows carry `"price_sell": null` (shops never buy
        // back) — live measured on id_item=465 and id_terminal=437. Must
        // deserialize to 0, not fail the whole table.
        let row: ItemPriceRow = serde_json::from_str(
            r#"{
                "id": 1, "id_item": 465, "id_terminal": 107,
                "price_buy": 295, "price_sell": null,
                "item_name": "CSP-68H Backpack",
                "terminal_name": "CenterMass - ArcCorp",
                "terminal_code": "CMAC", "date_modified": null
            }"#,
        )
        .unwrap();
        assert_eq!(row.price_buy, 295.0);
        assert_eq!(row.price_sell, 0.0, "null sell must map to 0");
        assert_eq!(row.date_modified, 0, "null ts must map to 0");
    }

    #[test]
    fn listing_price_string_parses() {
        let row: ListingRow = serde_json::from_str(
            r#"{"id": 179901, "operation": "sell", "type": "item",
                "price": "400000", "in_stock": 8, "location": "Levski",
                "user_name": "alexlejero", "date_added": 1792551295,
                "date_expiration": 1797738895, "quality": null,
                "id_star_system": 55,
                "slug": "stark-compensator-2-UKCqAtSM3p"}"#,
        )
        .unwrap();
        assert_eq!(row.price, "400000");
        assert_eq!(row.date_expiration, Some(1_797_738_895));
        assert_eq!(row.quality, None);
        assert_eq!(row.id_star_system, 55);
        assert_eq!(row.slug, "stark-compensator-2-UKCqAtSM3p");
    }

    #[test]
    fn listing_row_defaults_missing_slug_and_system() {
        // Live responses sometimes omit quality/slug/system; defaults must not
        // fail (older fixtures without the Phase-2 fields).
        let row: ListingRow =
            serde_json::from_str(r#"{"id": 1, "operation": "buy", "type": "service"}"#).unwrap();
        assert_eq!(row.slug, "");
        assert_eq!(row.id_star_system, 0);
        assert_eq!(row.quality, None);
    }

    #[test]
    fn star_system_row_parses() {
        let row: StarSystemRow =
            serde_json::from_str(r#"{"id": 68, "name": "Stanton", "code": "ST"}"#).unwrap();
        assert_eq!(row.id, 68);
        assert_eq!(row.name, "Stanton");
        assert_eq!(row.code.as_deref(), Some("ST"));
    }

    #[test]
    fn envelope_error_shape() {
        let env: Envelope<serde_json::Value> = serde_json::from_str(
            r#"{"status":"missing_required_input","http_code":400,"data":null,"message":"Missing required input"}"#,
        )
        .unwrap();
        assert_eq!(env.status, "missing_required_input");
        assert_eq!(env.message.as_deref(), Some("Missing required input"));
        assert!(env.data.is_none());
    }
}
