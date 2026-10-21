//! Shared DTOs crossing the `DataProvider` seam.
//!
//! These are deliberately source-agnostic — they describe *what* a result means,
//! not which API produced it. Field types must match the research doc; the
//! UEX-Client task fills in real mappings without changing these shapes.

use serde::Serialize;

/// Which kind of entity a search should look at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum SearchEntities {
    Commodities,
    Items,
    Locations,
    Terminals,
}

/// One search match: an entity plus enough for the UI to show a row.
#[derive(Clone, Debug, Serialize)]
pub struct SearchHit {
    pub id: u64,
    pub name: String,
    pub kind: String,
    pub subtitle: String,
}

/// A price observation (buy/sell) for a commodity or item at a location.
#[derive(Clone, Debug, Serialize)]
pub struct PricePoint {
    pub commodity: String,
    pub location: String,
    pub sell_price: u64,
    pub buy_price: u64,
    pub updated: u64,
}

/// Filters for the marketplace listing.
#[derive(Clone, Debug, Serialize)]
pub struct MarketplaceFilter {
    pub location: String,
    pub q: String,
}

/// A live marketplace offer.
#[derive(Clone, Debug, Serialize)]
pub struct Offer {
    pub id: u64,
    pub commodity: String,
    pub quantity: u64,
    pub price: u64,
    pub location: String,
    pub updated: u64,
}
