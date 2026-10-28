//! Shared DTOs crossing the `DataProvider` seam.
//!
//! These are deliberately source-agnostic — they describe *what* a result means,
//! not which API produced it. Field types match the UEX API research doc
//! (prices are numeric, marketplace `price` arrives as a string on the wire and
//! is parsed into `f64` here, ids are integers). All DTOs derive both `Serialize`
//! and `Deserialize` so they can cross the Tauri IPC boundary in both
//! directions.

use serde::{Deserialize, Serialize};

/// Reference datasets a provider can deliver in full (backfill / sync).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntityKind {
    Commodities,
    Items,
    Terminals,
    /// Star systems — only used to resolve a listing's `id_star_system` to a
    /// region name for the marketplace view; never a searchable dataset.
    StarSystems,
    /// Wiki locations (api.star-citizen.wiki, `source: wiki`): rich parent-
    /// child hierarchy (~2k rows). Searchable; NOT a UEX terminal — price
    /// aggregates resolve through the UEX terminal dataset (see
    /// `SearchEntities::Locations`).
    Locations,
}

impl EntityKind {
    /// Stable storage/kind tag used in the cache and search results.
    pub fn as_str(&self) -> &'static str {
        match self {
            EntityKind::Commodities => "commodity",
            EntityKind::Items => "item",
            EntityKind::Terminals => "terminal",
            EntityKind::StarSystems => "star_system",
            EntityKind::Locations => "location",
        }
    }
}

impl std::fmt::Display for EntityKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which kind of entity a search should look at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SearchEntities {
    Commodities,
    Items,
    Locations,
    Terminals,
}

impl SearchEntities {
    /// Reference datasets serving this search group. `Locations` searches BOTH
    /// datasets: the wiki location hierarchy (rich, `source: wiki`) AND the
    /// UEX terminal rows (trade terminals, `source: uex`) — wiki first so the
    /// richer structure ranks first; the price aggregate still opens through a
    /// UEX terminal of the matched chain.
    pub fn kinds(&self) -> &'static [EntityKind] {
        match self {
            SearchEntities::Commodities => &[EntityKind::Commodities],
            SearchEntities::Items => &[EntityKind::Items],
            SearchEntities::Locations => &[EntityKind::Locations, EntityKind::Terminals],
            SearchEntities::Terminals => &[EntityKind::Terminals],
        }
    }
}

/// A reference entity (commodity, item or terminal) as returned by a provider
/// for backfill. The UI-facing search result is [`SearchHit`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entity {
    pub id: u64,
    pub kind: EntityKind,
    pub name: String,
    pub code: Option<String>,
    /// Human-readable hierarchy, e.g. "Stanton > ArcCorp > Area 18".
    pub subtitle: String,
}

/// One search match: an entity plus enough for the UI to show a row.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchHit {
    /// Über IPC als STRING serialisiert: Wiki-Entity-IDs liegen über 2^53 und
    /// ein JS `number` rundet sie still (404707429735935130 → …35100) — damit
    /// brach der Location→Terminal-Resolve auf dem Rückweg in `get_prices`
    /// (t_925a577a). Deserialize akzeptiert weiterhin auch bare JSON-Numbers.
    #[serde(
        serialize_with = "serialize_id_as_string",
        deserialize_with = "deserialize_id_flexible"
    )]
    pub id: u64,
    pub name: String,
    /// Entity group tag: "commodity" | "item" | "terminal".
    pub kind: String,
    pub subtitle: String,
    pub code: Option<String>,
}

/// t_925a577a: IDs überqueren die IPC-Grenze zu einer JS-WebView. Wiki-IDs
/// (> 2^53) verlieren als JS-Number Präzision — deshalb als String
/// serialisieren, beim Lesen beides (Number oder String) akzeptieren.
pub fn serialize_id_as_string<S: serde::Serializer>(id: &u64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&id.to_string())
}

pub fn deserialize_id_flexible<'de, D>(d: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct IdVisitor;
    impl<'de> serde::de::Visitor<'de> for IdVisitor {
        type Value = u64;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("entity id (number or numeric string)")
        }
        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<u64, E> {
            Ok(v)
        }
        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<u64, E> {
            u64::try_from(v).map_err(serde::de::Error::custom)
        }
        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<u64, E> {
            // Negative wiki ids exist in the entities table (i64 storage);
            // they arrive here wrapped via the u64 two's-complement cast.
            // Accept the raw negative string as well and wrap it back, so a
            // negative id round-trips losslessly (t_1bab1fea blocker 2).
            match v.parse::<u64>() {
                Ok(id) => Ok(id),
                Err(_) => v
                    .parse::<i64>()
                    .map(|x| x as u64)
                    .map_err(serde::de::Error::custom),
            }
        }
    }
    d.deserialize_any(IdVisitor)
}

/// `Option<u64>`-Variante von [`deserialize_id_flexible`]: JSON null/missing →
/// None, sonst Number oder numeric string → Some(id). Negative wiki-id-Strings
/// (two's-complement-wrapped u64) werden ebenfalls akzeptiert (t_1bab1fea).
pub fn deserialize_opt_id_flexible<'de, D>(d: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct OptIdVisitor;
    impl<'de> serde::de::Visitor<'de> for OptIdVisitor {
        type Value = Option<u64>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("entity id (null, number or numeric string)")
        }
        fn visit_unit<E: serde::de::Error>(self) -> Result<Option<u64>, E> {
            Ok(None)
        }
        fn visit_none<E: serde::de::Error>(self) -> Result<Option<u64>, E> {
            Ok(None)
        }
        fn visit_some<D: serde::Deserializer<'de>>(self, d: D) -> Result<Option<u64>, D::Error> {
            deserialize_id_flexible(d).map(Some)
        }
        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Option<u64>, E> {
            Ok(Some(v))
        }
        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Option<u64>, E> {
            u64::try_from(v).map(Some).map_err(serde::de::Error::custom)
        }
        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Option<u64>, E> {
            // Negative wiki ids exist in the entities table (i64 storage);
            // they arrive here wrapped via the u64 two's-complement cast.
            // Accept the raw negative string as well and wrap it back, so a
            // negative id round-trips losslessly (t_1bab1fea blocker 2).
            match v.parse::<u64>() {
                Ok(id) => Ok(Some(id)),
                Err(_) => v
                    .parse::<i64>()
                    .map(|x| Some(x as u64))
                    .map_err(serde::de::Error::custom),
            }
        }
    }
    d.deserialize_any(OptIdVisitor)
}

/// One terminal's price table inside a location aggregate (see
/// [`LocationPrices`]). `prices` may be empty when the terminal trades no
/// commodities (fuel/refinery docks, empty shops) — the UI renders that as
/// "keine Preisdaten" rather than an error.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TerminalPriceGroup {
    pub terminal_id: u64,
    /// Trade-display name of the terminal (provider maps it to the most
    /// complete source field, e.g. "Commodity Shop - Admin - Checkmate").
    pub terminal_name: String,
    pub prices: Vec<PricePoint>,
}

/// Location price aggregate: all terminals of one location (grouped by the
/// cached location chain) with their commodity prices. `location` is the
/// human-readable chain ("Pyro > Monox > … > Checkmate Station"),
/// `terminal` the display name of the terminal the user opened.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocationPrices {
    pub location: String,
    pub terminal: String,
    pub groups: Vec<TerminalPriceGroup>,
}

/// A price observation (buy/sell) for a commodity at a terminal.
///
/// UEX semantics: `buy_price`/`sell_price` are the last reported per-SCU
/// values at the terminal; 0 + `*_status == 0` means "not traded here". Stock
/// mirrors `scu_buy_avg`/`scu_sell_avg` (the API also reports `*_stock`
/// variants; averages are the most stable proxy). `updated` is the data's
/// timestamp (unix seconds), not the fetch time.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PricePoint {
    pub commodity: String,
    pub commodity_id: u64,
    pub location: String,
    pub terminal_id: u64,
    pub sell_price: f64,
    pub buy_price: f64,
    pub sell_stock: f64,
    pub buy_stock: f64,
    /// Inventory level 0 = unknown/out of stock, 1..=7 max.
    pub sell_status: u8,
    pub buy_status: u8,
    pub quality: u16,
    pub updated: u64,
}

/// Marketplace listing operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarketOperation {
    Buy,
    Sell,
}

impl MarketOperation {
    pub fn as_str(&self) -> &'static str {
        match self {
            MarketOperation::Buy => "buy",
            MarketOperation::Sell => "sell",
        }
    }
}

impl std::fmt::Display for MarketOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for MarketOperation {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "buy" => Ok(MarketOperation::Buy),
            "sell" => Ok(MarketOperation::Sell),
            other => Err(format!("unknown marketplace operation: {other}")),
        }
    }
}

/// Marketplace listing type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ListingType {
    Item,
    Service,
    Contract,
}

impl ListingType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ListingType::Item => "item",
            ListingType::Service => "service",
            ListingType::Contract => "contract",
        }
    }
}

impl std::fmt::Display for ListingType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ListingType {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "item" => Ok(ListingType::Item),
            "service" => Ok(ListingType::Service),
            "contract" => Ok(ListingType::Contract),
            other => Err(format!("unknown listing type: {other}")),
        }
    }
}

/// How marketplace offers are ordered (client-side — the UEX API exposes no
/// sort parameter, only filter inputs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OfferSort {
    /// `date_added` descending (API default order).
    #[serde(rename = "newest")]
    Newest,
    /// Unit price ascending (cheapest first).
    #[serde(rename = "price_asc")]
    PriceAsc,
    /// Unit price descending (most expensive first).
    #[serde(rename = "price_desc")]
    PriceDesc,
}

/// Filters for the marketplace listing.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MarketplaceFilter {
    /// Free-text location filter (terminal name/code or system), matched
    /// locally by the provider — the API exposes no location text search.
    pub location: String,
    /// Free-text query, matched locally against title/description/seller,
    /// location (terminal) and listing type/operation — every whitespace-
    /// separated term must hit at least one field (market main search field:
    /// Name/Typ/Terminal/Verkäufer).
    pub q: String,
    /// Restrict to one item (API `id_item`).
    pub item_id: Option<u64>,
    /// Restrict to buy or sell offers (API `operation`).
    pub operation: Option<MarketOperation>,
    /// Restrict to one star-system region by name (e.g. "Stanton", "Nyx");
    /// matched locally against the resolved region of each listing.
    pub region: String,
    /// Ordering of the returned offers (default [`OfferSort::Newest`]).
    pub sort: Option<OfferSort>,
    /// Page size hint (API allows 100 default, up to 1000 with item+operation).
    pub limit: u32,
}

/// One entry of the user's own trade history (UEX `/user_trades/`),
/// source-agnostic projection. Mirrors the UEX wire row from the research
/// doc: 1:1 all string/number fields, denormalized names already resolved
/// by the source (commodity_name / terminal_name).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trade {
    pub id: u64,
    pub terminal_id: u64,
    pub commodity_id: u64,
    pub operation: MarketOperation,
    /// Menge in SCU (int auf dem Wire).
    pub scu: u64,
    /// Preis pro SCU (float auf dem Wire).
    pub price: f64,
    /// Unix-Timestamp (date_added).
    pub date_added: u64,
    pub commodity: String,
    pub terminal: String,
    pub vehicle: Option<String>,
    pub fleet_name: Option<String>,
    pub organization: Option<String>,
}

/// Write-Payload für `POST /marketplace_advertise/` (M2b, source-agnostic
/// Container über dem Doku-Schema [Research §1]). Alle Felder tragen die
/// offiziellen Input-Feldnamen (snake_case auf dem Wire) inkl. der
/// Referenzlisten (unit/availability/source/…). Optionale Felder werden nur
/// gesendet, wenn gesetzt — der UEX-CLIENT serialisiert das; ein `None` hier
/// bedeutet „Feld weglassen“, nicht „null senden“.
///
/// Die Pflichtfelder des Doku-Schemas für type=item sind NICHT optionale:
/// `operation`, `type`, `language`, `unit`, `price`, `currency`, `title`,
/// `description`, `in_stock`. Der UEX-Provider validiert sie vor dem POST
/// und übersetzt eine fehlende Pflichtangabe in eine deutsche
/// Konfigurations-Fehlermeldung (keine rohe API-Antwort).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdvertisePayload {
    /// Link zu `get_items` (type=item). Optional — UEX akzeptiert auch
    /// reine Text-Listings über title/description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_item: Option<u64>,
    /// Link zu `get_categories` (Pflicht für type=item laut Doku).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_category: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_star_system: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_terminal: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_organization: Option<u64>,
    /// `buy` | `sell` | `rent` | `trade`.
    pub operation: String,
    /// `en_US`, `de_DE`, … (Referenzliste Research §1).
    pub language: String,
    /// `item` | `service` | `contract`.
    pub r#type: String,
    /// Für type=item: `box`, `crate`, `cscu`, `dozen`, `hundred`, `pack`,
    /// `pair`, `scu`, `set`, `stack`, `thousand`, `unit`.
    pub unit: String,
    /// Preis als Ganzzahl (Doku: integer only).
    pub price: u64,
    /// `UEC` | `WIF` | `MGS`.
    pub currency: String,
    /// Trade-Location, z. B. „Port Tressler“.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Alphanumerisch + Bindestriche, max 140 Zeichen.
    pub title: String,
    /// Freitext, max 65535 Zeichen.
    pub description: String,
    /// `looted` | `pledged` | `purchased_in_game` | `pirated` | `gifted` | `crafted`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Verfügbare Menge.
    pub in_stock: u64,
    /// `immediate` | `ready_pickup` | `on_demand` | … (Referenzliste).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability: Option<String>,
    /// 0–100.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub durability: Option<u8>,
    /// Offer-Laufzeit in Std.: 1|2|3|5|12|24|48|72|120|168|336|720.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hours_expiration: Option<u64>,
    /// 1 = hidden (nicht veröffentlichen), 0/null = veröffentlichen nach Approval.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_hidden: Option<u8>,
    /// 1 = Production, 0 = Sandbox/Test. Wir senden IMMER explizit 1 in
    /// der Produktions-App (Research: `is_production=0` für Tests).
    pub is_production: u8,
}

impl AdvertisePayload {
    /// Minimales, selbstkonsistentes Test-Fixture (type=item, sell).
    #[cfg(test)]
    pub fn minimal_sell_item() -> Self {
        AdvertisePayload {
            id_item: None,
            id_category: None,
            id_star_system: None,
            id_terminal: None,
            id_organization: None,
            operation: "sell".into(),
            language: "en_US".into(),
            r#type: "item".into(),
            unit: "unit".into(),
            price: 120_000,
            currency: "UEC".into(),
            location: Some("Port Tressler".into()),
            title: "DRAKE NETTLE S | Used".into(),
            description: "Minor scuffs on left wing. Fully functional.".into(),
            source: Some("purchased_in_game".into()),
            in_stock: 1,
            availability: Some("ready_pickup".into()),
            durability: Some(85),
            hours_expiration: Some(168),
            is_hidden: None,
            is_production: 1,
        }
    }
}

/// A live marketplace offer (source-agnostic projection of a listing).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Offer {
    pub id: u64,
    pub title: String,
    pub description: String,
    pub item_id: u64,
    pub category_id: u64,
    pub operation: MarketOperation,
    pub listing_type: ListingType,
    pub price: f64,
    pub currency: String,
    pub unit: String,
    /// In-stock quantity.
    pub quantity: u64,
    pub location: String,
    /// Resolved star-system (region) name, e.g. "Stanton" — empty when the
    /// source did not carry a system id. Not a free-text field.
    pub region: String,
    pub seller: String,
    /// Quality on the source's scale (UEX: 0–1000).
    pub quality: u16,
    /// unix seconds the listing was added.
    pub updated: u64,
    /// unix seconds the listing expires, if set.
    pub expires: Option<u64>,
    /// Permalink slug on the source website (deep-link target).
    pub slug: String,
}

#[cfg(test)]
mod tests {
    use super::SearchHit;

    /// M2b (Write-Layer): der Advertise-Payload serialisiert snake_case und
    /// lässt ungesetzte Optionale WEG (FEHLT im JSON — nicht "null") — exakt
    /// das Doku-Schema [Research §1]. Verhindert, dass wir versehentlich
    /// `id_category: null` etc. an UEX schicken (Pflichtfeld-Semantik).
    #[test]
    fn advertise_payload_serializes_snake_case_and_skips_none() {
        let p = super::AdvertisePayload::minimal_sell_item();
        let json = serde_json::to_string(&p).unwrap();
        // Pflichtfelder an Bord, in offizieller Schreibweise.
        for key in [
            "\"operation\":\"sell\"",
            "\"language\":\"en_US\"",
            "\"type\":\"item\"",
            "\"unit\":\"unit\"",
            "\"price\":120000",
            "\"currency\":\"UEC\"",
            "\"title\":\"DRAKE NETTLE S | Used\"",
            "\"in_stock\":1",
            "\"hours_expiration\":168",
            "\"is_production\":1",
        ] {
            assert!(json.contains(key), "payload lacks {key}: {json}");
        }
        // Optionale ohne Wert fehlen KOMPLETT (nicht "field":null).
        for skipped in [
            "id_item",
            "id_category",
            "id_terminal",
            "id_organization",
            "is_hidden",
        ] {
            assert!(
                !json.contains(&format!("\"{skipped}\"")),
                "Optional {skipped} must be omitted (skip_serializing_if), got {json}"
            );
        }
        // Gesetzte Optionale sind dabei.
        assert!(json.contains("\"location\":\"Port Tressler\""));
        assert!(json.contains("\"source\":\"purchased_in_game\""));
        assert!(json.contains("\"availability\":\"ready_pickup\""));
        assert!(json.contains("\"durability\":85"));
    }

    /// Regression (t_925a577a): über IPC wird die SearchHit-ID als STRING
    /// serialisiert (Wiki-IDs > 2^53 runden als JS-Number still —
    /// 404707429735935130 → …35100); beim Lesen bleiben Number und String
    /// beide akzeptiert.
    #[test]
    fn search_hit_id_roundtrips_large_ids_as_string() {
        let hit = SearchHit {
            id: 404707429735935130,
            name: "Levski".into(),
            kind: "location".into(),
            subtitle: "Nyx > Levski".into(),
            code: None,
        };
        let json = serde_json::to_string(&hit).unwrap();
        assert!(
            json.contains("\"404707429735935130\""),
            "id muss als String serialisiert werden, got {json}"
        );
        let back: SearchHit = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, 404707429735935130);
        // Backwards compatibility: bare Number deserialisiert weiterhin.
        let num: SearchHit = serde_json::from_str(
            r#"{"id":778,"name":"Levski","kind":"location","subtitle":"Nyx > Levski","code":null}"#,
        )
        .unwrap();
        assert_eq!(num.id, 778);
    }
}
