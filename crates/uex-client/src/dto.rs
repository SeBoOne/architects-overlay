//! Wire DTOs for the UEX API v2.0 (see research doc / api.uexcorp.uk).
//!
//! These structs mirror the *actual* JSON shapes captured from the live API —
//! every field type was verified against real responses (October 2026), not
//! guessed. Notable quirks handled here:
//!
//! * responses always carry `{"status": …, "data": …}`; status `"ok"` OR the
//!   API's string-coded error names (`requests_limit_reached`, …);
//! * marketplace `price` arrives as a **string** (`"400000"`), parsed to f64;
//! * `/marketplace_listings` sends explicit `null` (not missing fields) for
//!   optional u64 columns (`id_item`, `id_category`, `id_star_system`,
//!   `in_stock`, `date_added`) — mapped to 0 via `de_u64_null_to_zero`
//!   (live bug t_cbd4a605: `invalid type: null, expected a integer` broke
//!   the entire offers fetch);
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

/// Same for `i64` (can be negative — e.g. UEX route `score`).
pub(crate) fn de_i64_null_to_zero<'de, D>(d: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<i64>::deserialize(d)?.unwrap_or(0))
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

/// Same for `Option<u64>`, but normalizes an explicit `null` to `Some(0)`.
/// serde's built-in `Option` mapping (null → None) looks tidy but loses the
/// UEX semantic: `null` and `0` both mean "no expiry / not set", and 0 is the
/// sentinel every consumer (sort, display) already treats as "none". Mapping
/// null → None instead would re-introduce `invalid type: null, expected u64`
/// as soon as the field is narrowed to a plain `u64`.
pub(crate) fn de_opt_u64_null_to_zero<'de, D>(d: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<u64>::deserialize(d)?.unwrap_or(0)))
}

/// `null` → `""` for plain string fields. The live `/marketplace_listings`
/// endpoint sends `null` for optional text fields (e.g. `location` in ~2% of
/// rows) instead of an empty string — `#[serde(default)]` alone does NOT cover
/// that, because an explicit `null` is not a missing field.
pub(crate) fn de_string_null_to_empty<'de, D>(d: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

// ---------------------------------------------------------------------------
// Flexible Marketplace-Zahlen (t_89d1e32e): UEX liefert Zahlen auf den
// Marketplace-Endpunkten (`/marketplace_negotiations`, `/marketplace_listings`,
// `/user_trades`, `/marketplace_advertise`) teils als JSON-Zahl, teils als
// JSON-String (`"7000000"`), teils als explizites `null`. Bare `f64`/`u64`
// crasht auf der String-Form (`invalid type: string "7000000", expected f64`)
// und reißt den ganzen Fetch mit. Diese Deserializer akzeptieren alle drei
// Formen; null/""/nicht-numerisch → 0 (gleiche Toleranzlinie wie die
// null→0-Helfer oben — der Feed bleibt lebendig, Einzelwert 0 ist der
// etablierte "unbekannt"-Sentinel).
// ---------------------------------------------------------------------------

/// String/Number/null → `f64`, nicht repräsentierbares → `None`.
fn flex_f64(v: Option<serde_json::Value>) -> Option<f64> {
    match v {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::Number(n)) => n.as_f64(),
        Some(serde_json::Value::String(s)) => s.trim().parse::<f64>().ok(),
        Some(_) => None,
    }
}

/// String/Number/null → `u64` (Float-Form wird ganzzahlig gerundet).
fn flex_u64(v: Option<serde_json::Value>) -> Option<u64> {
    match flex_f64(v) {
        Some(x) if x.is_finite() && x >= 0.0 => Some(x.round() as u64),
        _ => None,
    }
}

/// f64-Feld: Zahl, String-Zahl, null oder leerer String → 0.0.
pub(crate) fn de_f64_flex_null_to_zero<'de, D>(d: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(flex_f64(Option::<serde_json::Value>::deserialize(d)?).unwrap_or(0.0))
}

/// u64-Feld: Zahl, String-Zahl, null oder leerer String → 0.
pub(crate) fn de_u64_flex_null_to_zero<'de, D>(d: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(flex_u64(Option::<serde_json::Value>::deserialize(d)?).unwrap_or(0))
}

/// u8-Feld (0/1-Flags): Zahl, String-Zahl, null → 0.
pub(crate) fn de_u8_flex_null_to_zero<'de, D>(d: D) -> Result<u8, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(flex_u64(Option::<serde_json::Value>::deserialize(d)?)
        .unwrap_or(0)
        .min(u8::MAX as u64) as u8)
}

/// u16-Feld (quality): Zahl, String-Zahl, null → 0.
pub(crate) fn de_u16_flex_null_to_zero<'de, D>(d: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(flex_u64(Option::<serde_json::Value>::deserialize(d)?)
        .unwrap_or(0)
        .min(u16::MAX as u64) as u16)
}

/// `Option<f64>`: null/""/nicht-numerisch → None (offener Deal bleibt offen),
/// Zahl oder String-Zahl → Some.
pub(crate) fn de_opt_f64_flex<'de, D>(d: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(flex_f64(Option::<serde_json::Value>::deserialize(d)?))
}

/// `Option<u64>`: null/""/nicht-numerisch → None, Zahl oder String-Zahl → Some.
pub(crate) fn de_opt_u64_flex<'de, D>(d: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(flex_u64(Option::<serde_json::Value>::deserialize(d)?))
}

/// Wie [`de_opt_u64_null_to_zero`], plus flexibel: String-Zahl akzeptieren.
/// null → Some(0) (UEX-Semantik „no expiry“ bleibt erhalten).
pub(crate) fn de_opt_u64_flex_null_to_zero<'de, D>(d: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(
        flex_u64(Option::<serde_json::Value>::deserialize(d)?).unwrap_or(0),
    ))
}

/// `Option<u16>` (quality): null/""/nicht-numerisch → None, sonst Some.
pub(crate) fn de_opt_u16_flex<'de, D>(d: D) -> Result<Option<u16>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(flex_u64(Option::<serde_json::Value>::deserialize(d)?)
        .map(|x| x.min(u16::MAX as u64) as u16))
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
    /// Routen-Flags (Karte t_128b7547): Illegal-Gut, handelbar in welcher
    /// Richtung (0/1 auf dem Wire, null → 0).
    #[serde(default, deserialize_with = "de_u8_null_to_zero")]
    pub is_illegal: u8,
    #[serde(default, deserialize_with = "de_u8_null_to_zero")]
    pub is_sellable: u8,
    #[serde(default, deserialize_with = "de_u8_null_to_zero")]
    pub is_buyable: u8,
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
    /// Routen-Flags (Karte t_128b7547): Autoload-Fähigkeit, Cargodeck,
    /// Ladedock/Frachtaufzug. `serde(default)`-Struct-Felder fehlen nicht.
    #[serde(default, deserialize_with = "de_u8_null_to_zero")]
    pub is_auto_load: u8,
    #[serde(default, deserialize_with = "de_u8_null_to_zero")]
    pub is_cargo_center: u8,
    #[serde(default, deserialize_with = "de_u8_null_to_zero")]
    pub has_loading_dock: u8,
    #[serde(default, deserialize_with = "de_u8_null_to_zero")]
    pub has_freight_elevator: u8,
}

/// `/vehicles` row (Schiffs-SCU-Kapazitäten für Routen-Filter, Karte
/// t_128b7547): nur die Felder, die der Routen-Filter braucht.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // id/is_ground_vehicle/is_available_live: Komplettbild des Rows
pub struct VehicleRow {
    pub id: u64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub name_full: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    /// Cargoraum in SCU (0 bei Fahrzeugen ohne Cargogrid).
    #[serde(default, deserialize_with = "de_f64_null_to_zero")]
    pub scu: f64,
    #[serde(default, deserialize_with = "de_u8_null_to_zero")]
    pub is_ground_vehicle: u8,
    #[serde(default, deserialize_with = "de_u8_null_to_zero")]
    pub is_available_live: u8,
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

/// `/categories` row (used to enumerate items AND to offer the user a
/// category picker in the advertise form — UEX requires `id_category` for
/// `type=item`, so the selector must present id + display name).
#[derive(Debug, Deserialize)]
pub struct CategoryRow {
    pub id: u64,
    /// Category name, e.g. "Backpacks" (null-tolerant — live API may send
    /// explicit nulls on optional text fields, same rule as everywhere).
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub name: String,
    /// Parent section, e.g. "Armor" — useful as a dropdown group label.
    #[serde(default)]
    pub section: Option<String>,
    /// "item" | "service" | … we only ever advertise items.
    #[serde(default)]
    pub r#type: Option<String>,
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

/// `data`-Envelope einer erfolgreichen `POST /marketplace_advertise/`
/// AntwORT (M2b). UEX liefert hier KEINE vollständige ListingRow (die
/// erscheint erst nach Approval in `/marketplace_listings`, M2b-4) — nur
/// `{id, url}` (der Deep-Link zur Listung). Beide Felder sind tolerant
/// defaultet: selbst wenn die API eine abweichende/leere `data`-Struktur
/// schickt, darf das Anlegen NICHT als Fehler brechen (Review t_6e26ec82
/// Runde 2: der vorherige ListingRow-Parse warf „missing field id" obwohl
/// der Server das Listing angelegt hatte).
#[derive(Debug, Deserialize)]
pub struct CreateResponse {
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub id: u64,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub url: String,
}

/// `/marketplace_listings` row.
#[derive(Debug, Deserialize)]
pub struct ListingRow {
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub id: u64,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub title: String,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub description: String,
    /// `#[serde(default)]` alone does NOT cover an explicit `null` (live
    /// bug t_cbd4a605: `/marketplace_listings` sends `null` for these u64
    /// fields, e.g. deleted/unresolved items — deserializing `null` into a
    /// bare `u64` failed the whole fetch with
    /// `invalid type: null, expected a integer`). null maps to 0.
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub id_item: u64,
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub id_category: u64,
    /// Star system the listing is placed in (0 = unknown) — resolved to a
    /// region name via `/star_systems`.
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub id_star_system: u64,
    /// "buy" | "sell".
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub operation: String,
    /// "item" | "service" | "contract".
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    #[allow(non_snake_case)]
    pub r#type: String,
    /// The API sends the price as a STRING to preserve precision.
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub price: String,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub currency: String,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub unit: String,
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub in_stock: u64,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub location: String,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub user_name: String,
    /// Quality on the source's scale (UEX: 0–1000); null when not set —
    /// teils als String-Zahl (t_89d1e32e).
    #[serde(default, deserialize_with = "de_opt_u16_flex")]
    pub quality: Option<u16>,
    /// The live API sends this sometimes as `null`, sometimes as empty string
    /// `""` — kept as String so both shapes deserialize. Not used in Phase 2.
    #[serde(default)]
    pub hours_expiration: Option<String>,
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub date_added: u64,
    /// null and `0` mean "no expiry" (see `de_opt_u64_null_to_zero`).
    #[serde(default, deserialize_with = "de_opt_u64_flex_null_to_zero")]
    pub date_expiration: Option<u64>,
    /// Permalink slug on uexcorp.space (deep-link target).
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub slug: String,
    /// M3a (Faktencheck, Live-Probe 2026-10-28): `/marketplace_listings`
    /// liefert `photos` als KOMMA-verbundene CDN-URLs (Thumbnail-Variante
    /// `-t.jpg`; mit abschließendem Komma). Kann fehlen/null sein.
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub photos: String,
    /// unix seconds of the UEX-Approval; `null`/`0` = Listing wartet auf
    /// Freigabe (pending). Öffentliche Listings sind immer approved — der
    /// Pending-Fall erscheint nur im eigenen Pull (secret-key).
    #[serde(default, deserialize_with = "de_u64_null_to_zero")]
    pub date_approved: u64,
}

/// M3a: `photos`-Wire-Feld (`"url1,url2,"`) → saubere URL-Liste. Trimmt
/// Leerraum, verwirft leere Segmente (abschließendes Komma ist normal).
pub fn split_photos(photos: &str) -> Vec<String> {
    photos
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod photos_tests {
    use super::split_photos;

    #[test]
    fn split_photos_handles_trailing_comma_and_gaps() {
        assert_eq!(
            split_photos("https://a/x-t.jpg,"),
            vec!["https://a/x-t.jpg"]
        );
        assert_eq!(split_photos(""), Vec::<String>::new());
        assert_eq!(split_photos(" , ,b ,"), vec!["b"]);
    }
}

/// `/user_trades` row (eigener Trade-Verlauf, M2a). Null-Härtung wie überall:
/// UEX sendet explizite `null`s auf optionalen numerischen/String-Feldern.
/// `operation` ist "buy"|"sell"; `scu` (int) und `price` (float) pro SCU.
#[derive(Debug, Deserialize)]
pub struct UserTradeRow {
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub id: u64,
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub id_terminal: u64,
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub id_commodity: u64,
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub id_user_fleet: u64,
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub id_vehicle: u64,
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub id_organization: u64,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub operation: String,
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub scu: u64,
    #[serde(default, deserialize_with = "de_f64_flex_null_to_zero")]
    pub price: f64,
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub date_added: u64,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub commodity_name: String,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub terminal_name: String,
    #[serde(default)]
    pub user_fleet_name: Option<String>,
    #[serde(default)]
    pub vehicle_name: Option<String>,
    #[serde(default)]
    pub organization_name: Option<String>,
}

/// `/marketplace_negotiations` row (eigene Marktplatz-Deals, t_350c4300 B2).
/// Null-Härtung wie überall (Serde-Regel): UEX sendet explizite `null`s auf
/// optionalen numerischen/String-Feldern — `#[serde(default)]` deckt nur
/// FEHLENDE Felder ab, deshalb hier die null→…-Deserializer pro optionalem
/// Feld. `is_listing_advertiser` ist ein 0/1-int; `deal_value`/`date_closed`
/// sind null-bar (offene Verhandlung / noch kein Abschluss).
#[allow(dead_code)] // advertiser_avatar/client_avatar/date_closed_client: Wire-Komplettbild, UI liest sie (noch) nicht
#[derive(Debug, Deserialize)]
pub struct NegotiationRow {
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub id: u64,
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub id_listing: u64,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub hash: String,
    /// UEX liefert den Preis hier (live belegt, t_89d1e32e) teils als
    /// String (`"7000000"`) statt Zahl.
    #[serde(default, deserialize_with = "de_f64_flex_null_to_zero")]
    pub price: f64,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub unit: String,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub currency: String,
    /// Final agreed deal value („set by the paying side on successful close“).
    /// Zahl ODER String-Zahl (t_89d1e32e); null bleibt None (offen).
    #[serde(default, deserialize_with = "de_opt_f64_flex")]
    pub deal_value: Option<f64>,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub deal_value_currency: String,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub listing_title: String,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub listing_slug: String,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub advertiser_name: String,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub advertiser_username: String,
    #[serde(default)]
    pub advertiser_avatar: Option<String>,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub client_name: String,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub client_username: String,
    #[serde(default)]
    pub client_avatar: Option<String>,
    /// 0/1-int auf dem Wire (teils als String, t_89d1e32e).
    #[serde(default, deserialize_with = "de_u8_flex_null_to_zero")]
    pub is_listing_advertiser: u8,
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub date_added: u64,
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub date_modified: u64,
    #[serde(default, deserialize_with = "de_opt_u64_flex")]
    pub date_closed: Option<u64>,
    #[serde(default, deserialize_with = "de_opt_u64_flex")]
    pub date_closed_client: Option<u64>,
}

/// `/marketplace_negotiations_messages` row (Deal-Chat, M3b t_893761f1).
/// Null-Härtung wie überall (Serde-Regel): UEX sendet explizite `null`s auf
/// optionalen Feldern (`event`/`message` sind null bei System-Events bzw.
/// als Makierung leerer Nachrichten) — `#[serde(default)]` deckt nur
/// FEHLENDE Felder ab. Zahlen teils als String (t_89d1e32e-Muster).
#[derive(Debug, Deserialize)]
pub struct NegotiationMessageRow {
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub id: u64,
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub id_listing: u64,
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub id_negotiation: u64,
    #[serde(default)]
    pub event: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub listing_title: String,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub listing_slug: String,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub negotiation_hash: String,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub user_name: String,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub user_username: String,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub user_avatar: String,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub api_name: String,
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub date_added: u64,
    #[serde(default, deserialize_with = "de_opt_u64_flex")]
    pub date_read: Option<u64>,
}

/// POST `/marketplace_negotiations_messages/`-Antwort: nur die neue
/// Message-ID (Doku §2.1 Output `id_message`).
#[derive(Debug, Deserialize)]
pub struct MessageCreatedRow {
    #[serde(default, deserialize_with = "de_u64_flex_null_to_zero")]
    pub id_message: u64,
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

/// `/commodities_routes` row (PLURAL! — der echte Endpoint, t_9c33139c /
/// t_851c9b2d). Für die UEX-Enrichment-Schicht (Option b) werden NUR die
/// ergänzenden Felder übernommen — Preise/Marge/ROI/Profit sind BEWUSST NICHT
/// dabei (die lokale K1-Matrix bleibt Owner der Profit-Logik).
///
/// Null-Härtung wie überall: UEX sendet bei unbekannten Werten explizites
/// `null` (nicht ein fehlendes Feld), `#[serde(default)]` deckt nur fehlende
/// Felder ab. Alle numerischen Felder laufen deshalb durch die
/// `de_*_null_to_zero`-Deserializer (Serde-Regel, UEX-live belegt).
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Wire-DTO: nur die Enrichment-relevanten Felder werden gelesen
pub struct CommodityRouteRow {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub id_commodity: u64,
    #[serde(default, deserialize_with = "de_u64_null_to_zero")]
    pub id_terminal_origin: u64,
    #[serde(default, deserialize_with = "de_u64_null_to_zero")]
    pub id_terminal_destination: u64,
    /// `score` — UEX-Gesamtgüte (Anzeige; nie Sortierung).
    #[serde(default, deserialize_with = "de_i64_null_to_zero")]
    pub score: i64,
    /// Volumen: verfügbare SCU je Seite + erreichbare SCU.
    #[serde(default, deserialize_with = "de_f64_null_to_zero")]
    pub scu_origin: f64,
    #[serde(default, deserialize_with = "de_f64_null_to_zero")]
    pub scu_destination: f64,
    #[serde(default, deserialize_with = "de_f64_null_to_zero")]
    pub scu_reachable: f64,
    /// Scarcity/Nachfrage: Lagerbestand-Level 0..=7.
    #[serde(default, deserialize_with = "de_u8_null_to_zero")]
    pub status_origin: u8,
    #[serde(default, deserialize_with = "de_u8_null_to_zero")]
    pub status_destination: u8,
    /// Container-CSV ("1,2,4,8,16,24,32").
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub container_sizes_origin: String,
    #[serde(default, deserialize_with = "de_string_null_to_empty")]
    pub container_sizes_destination: String,
    /// Nachfragetiefe: Anzahl Community-Preismeldungen.
    #[serde(default, deserialize_with = "de_u64_null_to_zero")]
    pub price_origin_users_rows: u64,
    #[serde(default, deserialize_with = "de_u64_null_to_zero")]
    pub price_destination_users_rows: u64,
    #[serde(default)]
    pub game_version_origin: Option<String>,
    #[serde(default)]
    pub game_version_destination: Option<String>,
    #[serde(default)]
    pub origin_faction_name: Option<String>,
    #[serde(default)]
    pub destination_faction_name: Option<String>,
    /// 1 = Quantenmarker vorhanden.
    #[serde(default, deserialize_with = "de_u8_null_to_zero")]
    pub has_quantum_marker_origin: u8,
    #[serde(default, deserialize_with = "de_u8_null_to_zero")]
    pub has_quantum_marker_destination: u8,
    /// Unix-Timestamp der Routen-Erfassung.
    #[serde(default, deserialize_with = "de_u64_null_to_zero")]
    pub date_added: u64,
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
    fn listing_row_photos_and_approval_m3a() {
        // M3a (Live-Probe 2026-10-28): `/marketplace_listings` trägt
        // `photos` als Komma-Liste (abschließendes Komma!) und
        // `date_approved` (0/null = wartet auf Freigabe).
        let row: ListingRow = serde_json::from_str(
            r#"{"id": 181946, "title": "SnowBlind", "description": "x",
                 "id_item": 927, "id_category": 19, "id_star_system": 0,
                 "operation": "sell", "type": "item", "price": "200000",
                 "currency": "UEC", "unit": "unit", "in_stock": 1,
                 "location": "", "user_name": "skuudy", "quality": 500,
                 "hours_expiration": null, "date_added": 1793217025,
                 "date_expiration": 1798401025, "slug": "snowblind-j0bujzBKci",
                 "photos": "https://cdn.uexcorp.space/a-t.jpg,https://cdn.uexcorp.space/b-t.jpg,",
                 "date_approved": 1793217625}"#,
        )
        .unwrap();
        assert_eq!(
            split_photos(&row.photos),
            vec![
                "https://cdn.uexcorp.space/a-t.jpg",
                "https://cdn.uexcorp.space/b-t.jpg"
            ]
        );
        assert!(row.date_approved > 0);

        // Pending: null/0 → wartet auf Freigabe; fehlendes photos → leer.
        let pending: ListingRow = serde_json::from_str(
            r#"{"id": 1, "title": "t", "description": "", "id_item": null,
                 "id_category": null, "id_star_system": null, "operation": "sell",
                 "type": "item", "price": "1", "currency": "UEC",
                 "unit": "unit", "in_stock": 0, "location": "",
                 "user_name": "u", "date_added": 1, "slug": "s",
                 "date_approved": null, "photos": null}"#,
        )
        .unwrap();
        assert_eq!(pending.date_approved, 0);
        assert!(split_photos(&pending.photos).is_empty());
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
    fn listing_row_tolerates_explicit_null_u64_fields() {
        // Regression (t_cbd4a605, live Run433/434 Marktplatz):
        // 'Ungültige Antwort: invalid type: null, expected a integer' —
        // /marketplace_listings sends explicit `null` (not a missing field)
        // for id_item/id_category/id_star_system/in_stock/date_added on
        // rows with unresolved items. `#[serde(default)]` only covers
        // *missing* fields; every such row failed the whole fetch. The
        // crafted fixture mirrors the observed wire shape: numbers where
        // the API has data, explicit nulls where it has none, price as
        // string (UEX sends prices as strings for precision).
        let row: ListingRow = serde_json::from_str(
            r#"{
                "id": 181224,
                "title": null,
                "description": null,
                "id_item": null,
                "id_category": null,
                "id_star_system": null,
                "operation": "sell",
                "type": "item",
                "price": null,
                "currency": null,
                "unit": null,
                "in_stock": null,
                "location": null,
                "user_name": "sebo_one",
                "quality": null,
                "hours_expiration": null,
                "date_added": null,
                "date_expiration": null,
                "slug": null
            }"#,
        )
        .expect("explicit-null u64 fields must not fail the fetch");
        assert_eq!(row.id, 181224);
        assert_eq!(row.id_item, 0, "null id_item must map to 0");
        assert_eq!(row.id_category, 0, "null id_category must map to 0");
        assert_eq!(row.id_star_system, 0, "null star system must map to 0");
        assert_eq!(row.in_stock, 0, "null in_stock must map to 0");
        assert_eq!(row.date_added, 0, "null date_added must map to 0");
        // 0 is the established "no expiry" sentinel (UI: falsy).
        assert_eq!(row.date_expiration, Some(0));
        assert_eq!(row.title, "");
        assert_eq!(row.price, "");
        // A normal row with real numbers keeps its values untouched.
        let ok: ListingRow = serde_json::from_str(
            r#"{"id": 179901, "operation": "sell", "type": "item",
               "id_item": 465, "id_category": 12, "id_star_system": 55,
               "price": "400000", "in_stock": 8, "date_added": 1792551295,
               "date_expiration": 1797738895, "slug": "x-UKCqAtSM3p"}"#,
        )
        .unwrap();
        assert_eq!(ok.id_item, 465);
        assert_eq!(ok.id_category, 12);
        assert_eq!(ok.in_stock, 8);
        assert_eq!(ok.date_added, 1_792_551_295);
        assert_eq!(ok.date_expiration, Some(1_797_738_895));
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

    /// t_65a91d5b C3b: `/commodities_routes`-Wire-Row (echtes Sample-Feld,
    /// inkl. expliziter `null`s auf numerischen Feldern) deserialisiert und
    /// das Enrichment-Mapping übernimmt NUR die ergänzenden Felder.
    #[test]
    fn commodity_route_row_tolerates_nulls_and_full_schema() {
        let row: CommodityRouteRow = serde_json::from_str(
            r#"{
                "id": 474, "id_commodity": 79,
                "id_terminal_origin": 1, "id_terminal_destination": 274,
                "score": -33, "price_origin": 229, "price_destination": 210,
                "price_margin": -9.04762, "price_roi": -8.29694, "profit": -133,
                "scu_origin": 7, "scu_destination": 7, "scu_reachable": 7,
                "status_origin": 2, "status_destination": 1,
                "container_sizes_origin": "1,2,4,8,16,24,32",
                "container_sizes_destination": "1,2,4,8,16,24",
                "price_origin_users_rows": 0, "price_destination_users_rows": 3,
                "game_version_origin": "4.10.2", "game_version_destination": "4.10.2",
                "origin_faction_name": "United Empire of Earth",
                "destination_faction_name": null,
                "has_quantum_marker_origin": 1, "has_quantum_marker_destination": 0,
                "date_added": 1793092627,
                "commodity_name": "Waste", "commodity_slug": "waste"
            }"#,
        )
        .unwrap();
        assert_eq!(row.id_terminal_origin, 1);
        assert_eq!(row.id_terminal_destination, 274);
        assert_eq!(row.score, -33);
        assert_eq!(row.scu_reachable, 7.0);
        assert_eq!(row.status_origin, 2);
        assert_eq!(row.price_destination_users_rows, 3);
        // Explizites null auf destination_faction_name → None, kein Fehler.
        assert_eq!(row.destination_faction_name, None);
        assert_eq!(
            row.origin_faction_name.as_deref(),
            Some("United Empire of Earth")
        );
        assert_eq!(row.has_quantum_marker_origin, 1);
    }

    /// t_65a91d5b C3b: `null`-Werte auf den numerischen Enrichment-Feldern
    /// (UEX sendet `null` statt 0, wenn unbekannt) dürfen das Mapping nicht
    /// reißen — Serde-Regel (explizites null ≠ fehlendes Feld).
    #[test]
    fn negotiation_row_parses_live_shape() {
        // /marketplace_negotiations-Response (UEX-Doku „List all advertisement
        // deals associated with a user"): Optionals null-bar (deal_value,
        // avatar, date_closed), is_listing_advertiser 0/1-int. Serde-Regel:
        // explizites null auf Optionals MUSS deserialisieren.
        let row: NegotiationRow = serde_json::from_str(
            r#"{
                "id": 5512, "id_listing": 4141, "hash": "a1b2c3",
                "price": 4500000.00, "unit": "unit", "currency": "UEC",
                "deal_value": null, "deal_value_currency": null,
                "listing_title": "Drake Nettle S", "listing_slug": "drake-nettle-s",
                "advertiser_name": "Semih Balci", "advertiser_username": "sebo_one",
                "advertiser_avatar": "https://…/avatar.png",
                "client_name": "John Doe", "client_username": "johndoe",
                "client_avatar": null,
                "is_listing_advertiser": 1,
                "date_added": 1730000100, "date_modified": 1730000200,
                "date_closed": null, "date_closed_client": null
            }"#,
        )
        .unwrap();
        assert_eq!(row.id, 5512);
        assert_eq!(row.id_listing, 4141);
        assert_eq!(row.listing_slug, "drake-nettle-s");
        assert_eq!(row.deal_value, None, "offener Deal: null bleibt None");
        assert_eq!(row.is_listing_advertiser, 1);
        assert_eq!(row.client_avatar, None);
        assert_eq!(row.date_closed, None);
        assert_eq!(row.date_modified, 1730000200);
        // Abgeschlossener Deal (deal_value gesetzt, date_closed int).
        let closed: NegotiationRow = serde_json::from_str(
            r#"{"id":5513,"id_listing":4142,"hash":"x7","price":5000000.0,
                "unit":"unit","currency":"UEC","deal_value":4900000.0,
                "deal_value_currency":"UEC","listing_title":"Nettles",
                "listing_slug":"nettles","advertiser_name":"a","advertiser_username":"u",
                "advertiser_avatar":null,"client_name":"c","client_username":"cu",
                "client_avatar":null,"is_listing_advertiser":0,
                "date_added":1730000100,"date_modified":1730000300,
                "date_closed":1730000300,"date_closed_client":1730000300}"#,
        )
        .unwrap();
        assert_eq!(closed.deal_value, Some(4900000.0));
        assert_eq!(closed.date_closed, Some(1730000300));
    }

    #[test]
    fn user_trade_row_parses_live_shape() {
        // /user_trades-Response (Research §3): optionale Strings null-bar,
        // operation buy/sell, scu int, price float pro SCU.
        let row: UserTradeRow = serde_json::from_str(
            r#"{
                "id": 12345, "id_terminal": 29, "id_commodity": 79,
                "id_user_fleet": 0, "id_vehicle": 3, "id_organization": null,
                "operation": "sell", "scu": 250, "price": 10000.00,
                "date_added": 1730000100,
                "commodity_name": "Waste", "terminal_name": "Port Tressler",
                "user_fleet_name": null, "vehicle_name": "Caterpillar",
                "organization_name": "AAWA"
            }"#,
        )
        .unwrap();
        assert_eq!(row.id, 12345);
        assert_eq!(row.operation, "sell");
        assert_eq!(row.scu, 250);
        assert_eq!(row.price, 10000.0);
        assert_eq!(row.vehicle_name.as_deref(), Some("Caterpillar"));
        assert_eq!(row.organization_name.as_deref(), Some("AAWA"));
        assert!(row.user_fleet_name.is_none());
    }

    #[test]
    fn commodity_route_row_nulls_map_to_zero() {
        let row: CommodityRouteRow = serde_json::from_str(
            r#"{
                "id": 475, "id_commodity": 79,
                "id_terminal_origin": 1, "id_terminal_destination": 276,
                "score": null, "scu_origin": null, "scu_destination": null,
                "scu_reachable": null, "status_origin": null, "status_destination": null,
                "container_sizes_origin": null, "container_sizes_destination": null,
                "price_origin_users_rows": null, "price_destination_users_rows": null,
                "game_version_origin": null, "game_version_destination": null,
                "origin_faction_name": null, "destination_faction_name": null,
                "has_quantum_marker_origin": null, "has_quantum_marker_destination": null,
                "date_added": null
            }"#,
        )
        .unwrap();
        assert_eq!(row.score, 0);
        assert_eq!(row.scu_reachable, 0.0);
        assert_eq!(row.status_origin, 0);
        assert_eq!(row.container_sizes_origin, "");
        assert_eq!(row.price_origin_users_rows, 0);
        assert_eq!(row.date_added, 0);
        assert_eq!(row.has_quantum_marker_destination, 0);
        assert_eq!(row.game_version_origin, None);
    }

    /// t_89d1e32e: UEX liefert auf /marketplace_negotiations Zahlen teils als
    /// JSON-String (`"7000000"`) — das Live-Fehler-Payload-Fenster. Der ganze
    /// Fetch darf daran nicht mehr reißen.
    #[test]
    fn negotiation_row_parses_string_encoded_numbers() {
        let row: NegotiationRow = serde_json::from_str(
            r#"{
                "id": "5512", "id_listing": "4141", "hash": "a1b2c3",
                "price": "7000000", "unit": "unit", "currency": "UEC",
                "deal_value": "6900000", "deal_value_currency": null,
                "listing_title": "Drake Nettle S", "listing_slug": "drake-nettle-s",
                "advertiser_name": "Semih Balci", "advertiser_username": "sebo_one",
                "advertiser_avatar": null,
                "client_name": "John Doe", "client_username": "johndoe",
                "client_avatar": null,
                "is_listing_advertiser": "1",
                "date_added": "1730000100", "date_modified": "1730000200",
                "date_closed": null, "date_closed_client": null
            }"#,
        )
        .expect("String-Zahlen muessen deserialisieren (Live-Bug t_89d1e32e)");
        assert_eq!(row.id, 5512);
        assert_eq!(row.id_listing, 4141);
        assert_eq!(row.price, 7_000_000.0);
        assert_eq!(
            row.deal_value,
            Some(6_900_000.0),
            "String-deal_value -> Some"
        );
        assert_eq!(row.is_listing_advertiser, 1);
        assert_eq!(row.date_added, 1_730_000_100);
        assert_eq!(row.date_closed, None, "null bleibt None (offener Deal)");
        // Offene Verhandlung: price als Zahl, deal_value null — Original-Semantik intakt.
        let open: NegotiationRow = serde_json::from_str(
            r#"{"id":5514,"id_listing":4143,"hash":"z9","price":4500000.00,
                "unit":"unit","currency":"UEC","deal_value":null,
                "deal_value_currency":null,"listing_title":"t","listing_slug":"t",
                "advertiser_name":"a","advertiser_username":"u","advertiser_avatar":null,
                "client_name":"c","client_username":"cu","client_avatar":null,
                "is_listing_advertiser":0,"date_added":1730000100,
                "date_modified":1730000200,"date_closed":null,"date_closed_client":null}"#,
        )
        .unwrap();
        assert_eq!(open.price, 4_500_000.0, "echte Zahl bleibt ok");
        assert_eq!(open.deal_value, None);
    }

    /// t_89d1e32e: `/marketplace_listings` (Offers) mit String-Zahlen auf
    /// IDs/in_stock/date-Feldern — gleiches Fehlerklassen-Fenster.
    #[test]
    fn listing_row_parses_string_encoded_numbers() {
        let row: ListingRow = serde_json::from_str(
            r#"{
                "id": "9001", "title": "Waste", "description": "",
                "id_item": "79", "id_category": "2", "id_star_system": "3",
                "operation": "sell", "type": "item",
                "price": "400000", "currency": "UEC", "unit": "SCU",
                "in_stock": "250", "location": null, "user_name": "sebo_one",
                "quality": "1000", "hours_expiration": null,
                "date_added": "1730000100", "date_expiration": "1730100000",
                "slug": "waste-9001"
            }"#,
        )
        .expect("String-Zahlen in ListingRow muessen deserialisieren");
        assert_eq!(row.id, 9001);
        assert_eq!(row.id_item, 79);
        assert_eq!(row.in_stock, 250);
        assert_eq!(row.quality, Some(1000));
        assert_eq!(row.date_added, 1_730_000_100);
        assert_eq!(row.date_expiration, Some(1_730_100_000));
        // Zahl-Form unverändert ok; null-quality bleibt None.
        let numeric: ListingRow = serde_json::from_str(
            r#"{"id":9002,"title":"x","description":"","id_item":79,"id_category":2,
                "id_star_system":3,"operation":"sell","type":"item","price":"400000",
                "currency":"UEC","unit":"SCU","in_stock":0,"location":"","user_name":"u",
                "quality":null,"hours_expiration":"","date_added":1730000100,
                "date_expiration":null,"slug":"x"}"#,
        )
        .unwrap();
        assert_eq!(numeric.in_stock, 0);
        assert_eq!(numeric.quality, None);
        assert_eq!(
            numeric.date_expiration,
            Some(0),
            "null -> Some(0), Semantik 'no expiry'"
        );
    }

    /// t_89d1e32e: `/user_trades` mit String-Zahlen (scu/price/IDs).
    #[test]
    fn user_trade_row_parses_string_encoded_numbers() {
        let row: UserTradeRow = serde_json::from_str(
            r#"{
                "id": "12345", "id_terminal": "29", "id_commodity": "79",
                "id_user_fleet": "0", "id_vehicle": "3", "id_organization": null,
                "operation": "sell", "scu": "250", "price": "10000.50",
                "date_added": "1730000100",
                "commodity_name": "Waste", "terminal_name": "Port Tressler",
                "user_fleet_name": null, "vehicle_name": "Caterpillar",
                "organization_name": "AAWA"
            }"#,
        )
        .expect("String-Zahlen in UserTradeRow muessen deserialisieren");
        assert_eq!(row.id, 12345);
        assert_eq!(row.scu, 250);
        assert_eq!(row.price, 10000.5);
        assert_eq!(row.date_added, 1_730_000_100);
    }

    /// t_89d1e32e: `/marketplace_advertise`-Response mit String-id
    /// (Runde-2-Quirk-Fenster von CreateResponse).
    #[test]
    fn create_response_parses_string_id() {
        let res: CreateResponse = serde_json::from_str(r#"{"id": "4141", "url": "https://…/x"}"#)
            .expect("String-id muss deserialisieren");
        assert_eq!(res.id, 4141);
        let numeric: CreateResponse =
            serde_json::from_str(r#"{"id": 4142, "url": "https://…/y"}"#).unwrap();
        assert_eq!(numeric.id, 4142, "echte Zahl bleibt ok");
    }
}
