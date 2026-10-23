//! Wire DTOs for the Star Citizen Wiki API (OpenAPI v3 — `api.star-citizen.wiki`).
//!
//! All fields are tolerant (`Option` + `#[serde(default)]` everywhere): the
//! API is community-maintained and can grow or null fields between versions,
//! so a single unexpected `null` must never sink an entire fetch (same lesson
//! as the UEX provider, QA runde 2).

use serde::{Deserialize, Serialize};

/// A manufacturer reference inside items (`manufacturer.name` / `.code`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ManufacturerRef {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub code: Option<String>,
}

/// `/api/items` list row — subset of `item_link` used by the overlay
/// (metadata only; prices stay with UEX because the wiki `shops` arrays are
/// deprecated upstream and `uex_prices.purchase` is empty for most items).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ItemRow {
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub slug: String,
    /// In-game type path, e.g. `Char_Armor_Arms` (100+ types).
    #[serde(default, rename = "type")]
    pub item_type: Option<String>,
    /// Classification path, e.g. `FPS.Armor.Arms`.
    #[serde(default)]
    pub classification: Option<String>,
    #[serde(default)]
    pub manufacturer: Option<ManufacturerRef>,
    #[serde(default)]
    pub is_lootable: Option<bool>,
    #[serde(default)]
    pub is_craftable: Option<bool>,
    #[serde(default)]
    pub rarity: Option<String>,
    /// Multilingual in-game description (`en_EN` / `de_DE` …).
    #[serde(default)]
    pub description: Option<serde_json::Value>,
}

/// `/api/commodities` list row — mining/resource metadata per `commodity_link`.
/// NOTE: contrary to the earlier research note, this endpoint carries **no**
/// terminal buy/sell prices (that data is not in the official schema).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommodityRow {
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub display_name: Option<String>,
    /// Internal key (e.g. "Quartz").
    #[serde(default)]
    pub key: Option<String>,
    /// Refinement tier ("Raw" / "Refined").
    #[serde(default)]
    pub tier: Option<String>,
    /// Ordered commodity groups root → leaf, e.g. ["ProcessedGoods", "Vice"].
    #[serde(default)]
    pub commodity_groups: Option<Vec<String>>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub is_mineable: Option<bool>,
    #[serde(default)]
    pub systems: Option<Vec<String>>,
}

/// `/api/locations` list row — rich location metadata (structure only; shop
/// data is deprecated upstream and therefore not mapped).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocationRow {
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub system: Option<String>,
    #[serde(default)]
    pub parent_name: Option<String>,
    #[serde(default)]
    pub parent_type: Option<String>,
    #[serde(default)]
    pub location_type: Option<String>,
}

/// Pagination `meta` block of every list response.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct PageMeta {
    #[serde(default)]
    pub current_page: u32,
    #[serde(default)]
    pub per_page: u32,
    #[serde(default)]
    pub total: u32,
}

/// Pagination `links` block (only `next` is consumed by the client).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PageLinks {
    #[serde(default)]
    pub next: Option<String>,
    #[serde(default)]
    pub first: Option<String>,
    #[serde(default)]
    pub last: Option<String>,
    #[serde(default)]
    pub prev: Option<String>,
}

/// Envelope of every list endpoint: `{ data: [...], links, meta }`.
///
/// NOTE: no `#[serde(default)]` on `data` — the derive would then demand
/// `T: Default` ("imperfect derives", rustc #26925) for every caller. The
/// wiki API always sends `data` (an empty array when there are no rows), so
/// requiring the field is safe; `links`/`meta` keep their tolerant defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page<T> {
    pub data: Vec<T>,
    #[serde(default)]
    pub links: PageLinks,
    #[serde(default)]
    pub meta: PageMeta,
}

// Manual impl so the blanket bounds don't require `T: Default` (the derived
// one does — "imperfect derives", rustc #26925).
impl<T> Default for Page<T> {
    fn default() -> Self {
        Page {
            data: Vec::new(),
            links: PageLinks::default(),
            meta: PageMeta::default(),
        }
    }
}

/// Single-detail envelope: `{ data: {...} }`. Same imperfect-derive caveat as
/// [`Page`]: no `#[serde(default)]` on `data`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detail<T> {
    pub data: T,
}

/// Derive a stable numeric id from a wiki UUID (FNV-1a over the string).
/// The wiki uses UUIDs while the seam/cache uses u64 ids and UEX uses its own
/// numeric ids — hashing the UUID keeps wiki ids distinct with an astronom-
/// ically low collision risk, and the raw uuid travels in `Entity.code`.
pub fn stable_id(uuid: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in uuid.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    // Reserve 0 as "no id" marker.
    if hash == 0 {
        hash = 1;
    }
    hash
}
