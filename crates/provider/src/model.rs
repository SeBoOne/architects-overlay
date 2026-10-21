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
}

impl EntityKind {
    /// Stable storage/kind tag used in the cache and search results.
    pub fn as_str(&self) -> &'static str {
        match self {
            EntityKind::Commodities => "commodity",
            EntityKind::Items => "item",
            EntityKind::Terminals => "terminal",
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
    /// Reference datasets serving this search group. `Locations` maps to the
    /// terminal dataset (trade locations carry the full system/planet/station
    /// hierarchy).
    pub fn kinds(&self) -> &'static [EntityKind] {
        match self {
            SearchEntities::Commodities => &[EntityKind::Commodities],
            SearchEntities::Items => &[EntityKind::Items],
            SearchEntities::Locations | SearchEntities::Terminals => &[EntityKind::Terminals],
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
    pub id: u64,
    pub name: String,
    /// Entity group tag: "commodity" | "item" | "terminal".
    pub kind: String,
    pub subtitle: String,
    pub code: Option<String>,
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

/// Filters for the marketplace listing.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MarketplaceFilter {
    /// Free-text location filter (terminal name/code or system), matched
    /// locally by the provider — the API exposes no location text search.
    pub location: String,
    /// Free-text query, matched against title/description/seller locally.
    pub q: String,
    /// Restrict to one item (API `id_item`).
    pub item_id: Option<u64>,
    /// Restrict to buy or sell offers (API `operation`).
    pub operation: Option<MarketOperation>,
    /// Page size hint (API allows 100 default, up to 1000 with item+operation).
    pub limit: u32,
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
    pub seller: String,
    /// Quality on the source's scale (UEX: 0–1000).
    pub quality: u16,
    /// unix seconds the listing was added.
    pub updated: u64,
    /// unix seconds the listing expires, if set.
    pub expires: Option<u64>,
}
