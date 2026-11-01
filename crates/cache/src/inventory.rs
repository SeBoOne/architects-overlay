//! # cache::inventory
//!
//! Local, private inventory (Karte t_1bab1fea, v0.3.0 K2): the user's own
//! stock with mandatory quality (0–1000), a location and a quantity
//! (SCU or pieces), optional purchase price (aUEC). Rows either reference a
//! cached entity (`entities`, kind item/commodity — `entity_kind` +
//! `entity_id` + resolved `name`) or are free-text lines (`entity_kind`/`entity_id`
//! = NULL, name = the typed text). Locations likewise: a reference to a
//! location entity (`location_id` + resolved `location_name`) or a free-text
//! depot ("my Carrack", "home hangar").
//!
//! Deliberately NO sync/sharing/API: this table is local-first, written only
//! by the app, never touched by the background sync (which only refreshes
//! reference data).

use rusqlite::ToSql;
use rusqlite::types::ToSqlOutput;

use super::{Cache, CacheError, Result};

/// One inventory position. `entity_kind` is `"commodity"` or `"item"`
/// (entity reference) or `None` (free-text line).
///
/// Ids cross the IPC boundary to a JS WebView as STRINGS: wiki entity ids
/// (> 2^53) silently lose precision as JS numbers (t_925a577a convention,
/// t_1bab1fea blocker 2). Deserialize accepts bare JSON numbers as well.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct InventoryEntry {
    #[serde(serialize_with = "provider::serialize_id_as_string")]
    pub id: u64,
    /// `"commodity" | "item"` for entity rows, `None` for free text.
    pub entity_kind: Option<String>,
    /// Reference into `entities (kind, id)` — `None` for free text.
    #[serde(serialize_with = "ser_opt_id_as_string")]
    pub entity_id: Option<u64>,
    /// Display name (resolved from the entity, or the free text).
    pub name: String,
    /// Mandatory quality, 0–1000.
    pub quality: u32,
    /// Reference into `entities` (kind = location) — `None` for a free-text
    /// depot.
    #[serde(serialize_with = "ser_opt_id_as_string")]
    pub location_id: Option<u64>,
    /// Display location (resolved entity name or free-text depot).
    pub location_name: String,
    /// Quantity: SCU for commodities, pieces for items.
    pub quantity: f64,
    /// Optional purchase price in aUEC.
    pub price: Option<f64>,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Payload for adding a position (no id/timestamps — those are assigned).
/// Entity/location ids arrive from the JS frontend as STRINGS (wiki ids
/// > 2^53 lose precision as JS numbers — t_1bab1fea blocker 2); bare JSON
/// > numbers are still accepted for older callers.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct NewInventoryEntry {
    pub entity_kind: Option<String>,
    #[serde(deserialize_with = "provider::deserialize_opt_id_flexible")]
    pub entity_id: Option<u64>,
    pub name: String,
    pub quality: u32,
    #[serde(deserialize_with = "provider::deserialize_opt_id_flexible")]
    pub location_id: Option<u64>,
    pub location_name: String,
    pub quantity: f64,
    pub price: Option<f64>,
}

/// `Option<u64>` maps to NULL in SQLite (rusqlite implements ToSql for
/// Option; we store u64 as i64-compatible — u64 entity ids exceed i64 only
/// beyond 2^63, wiki ids are ~4·10^17 < 2^63, so a plain cast is safe).
fn u64_sql(v: Option<u64>) -> impl ToSql {
    struct Opt64(Option<u64>);
    impl ToSql for Opt64 {
        fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
            match self.0 {
                Some(v) => Ok(ToSqlOutput::from(v as i64)),
                None => Ok(ToSqlOutput::Owned(rusqlite::types::Value::Null)),
            }
        }
    }
    Opt64(v)
}

/// `Option<u64>` id → JSON: Some as string (JS-WebView precision loss above
/// 2^53), None as null (t_1bab1fea blocker 2, same convention as SearchHit).
fn ser_opt_id_as_string<S: serde::Serializer>(
    v: &Option<u64>,
    s: S,
) -> std::result::Result<S::Ok, S::Error> {
    match v {
        Some(id) => s.serialize_str(&id.to_string()),
        None => s.serialize_none(),
    }
}

fn row_to_entry(r: &rusqlite::Row<'_>) -> rusqlite::Result<InventoryEntry> {
    let entity_kind: Option<String> = r.get("entity_kind")?;
    let entity_id: Option<i64> = r.get("entity_id")?;
    let location_id: Option<i64> = r.get("location_id")?;
    let price: Option<f64> = r.get("price")?;
    Ok(InventoryEntry {
        id: r.get::<_, i64>("id")? as u64,
        entity_kind,
        entity_id: entity_id.map(|v| v as u64),
        name: r.get("name")?,
        quality: r.get::<_, i64>("quality")? as u32,
        location_id: location_id.map(|v| v as u64),
        location_name: r.get("location_name")?,
        quantity: r.get("quantity")?,
        price,
        created_at: r.get::<_, i64>("created_at")? as u64,
        updated_at: r.get::<_, i64>("updated_at")? as u64,
    })
}

const COLS: &str = "id, entity_kind, entity_id, name, quality, location_id, location_name, \
                    quantity, price, created_at, updated_at";

/// Sanity for an incoming payload: quality must be within 0–1000 (the card's
/// mandatory field), name and location must carry text.
fn validate(new: &NewInventoryEntry) -> Result<()> {
    if new.quality > 1000 {
        return Err(CacheError::Query(
            i18n_err::err_key("inventory.err_quality").into(),
        ));
    }
    if new.name.trim().is_empty() {
        return Err(CacheError::Query(i18n_err::err_key("inventory.err_name")));
    }
    if new.location_name.trim().is_empty() {
        return Err(CacheError::Query(i18n_err::err_key(
            "inventory.err_location",
        )));
    }
    Ok(())
}

impl Cache {
    /// Insert one position, returns its id.
    pub fn inventory_add(&self, new: NewInventoryEntry) -> Result<u64> {
        validate(&new)?;
        let now = super::now_unix();
        self.with(move |conn| {
            conn.execute(
                "INSERT INTO inventory (entity_kind, entity_id, name, quality, location_id, \
                 location_name, quantity, price, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
                rusqlite::params![
                    new.entity_kind,
                    u64_sql(new.entity_id),
                    new.name.trim(),
                    new.quality as i64,
                    u64_sql(new.location_id),
                    new.location_name.trim(),
                    new.quantity,
                    new.price,
                    now as i64,
                ],
            )?;
            Ok(conn.last_insert_rowid() as u64)
        })
    }

    /// Overwrite an existing position (full-row update; the frontend edits
    /// one row at a time). `Err` when the id does not exist.
    pub fn inventory_update(&self, id: u64, new: NewInventoryEntry) -> Result<()> {
        validate(&new)?;
        let now = super::now_unix();
        let changed = self.with(move |conn| {
            conn.execute(
                "UPDATE inventory SET entity_kind = ?1, entity_id = ?2, name = ?3, quality = ?4, \
                 location_id = ?5, location_name = ?6, quantity = ?7, price = ?8, updated_at = ?9 \
                 WHERE id = ?10",
                rusqlite::params![
                    new.entity_kind,
                    u64_sql(new.entity_id),
                    new.name.trim(),
                    new.quality as i64,
                    u64_sql(new.location_id),
                    new.location_name.trim(),
                    new.quantity,
                    new.price,
                    now as i64,
                    id as i64,
                ],
            )
        })?;
        if changed == 0 {
            return Err(CacheError::Query(i18n_err::err_keyp(
                "inventory.err_position_missing",
                serde_json::json!({ "id": id }),
            )));
        }
        Ok(())
    }

    /// Delete one position. `Err` when the id does not exist.
    pub fn inventory_delete(&self, id: u64) -> Result<()> {
        let changed = self
            .with(move |conn| conn.execute("DELETE FROM inventory WHERE id = ?1", [id as i64]))?;
        if changed == 0 {
            return Err(CacheError::Query(format!("Position {id} existiert nicht")));
        }
        Ok(())
    }

    /// All positions, grouped-ready: ordered by location (case-insensitive),
    /// then item name, then quality descending — the overview's default
    /// order (Standort → Item → Qualität).
    pub fn inventory_list(&self) -> Result<Vec<InventoryEntry>> {
        self.with(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {COLS} FROM inventory \
                     ORDER BY location_name COLLATE NOCASE ASC, name COLLATE NOCASE ASC, \
                     quality DESC, id ASC"
            ))?;
            let rows = stmt.query_map([], row_to_entry)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
    }

    /// Distinct free-text/normalized location names present in the inventory
    /// (for the add-form datalist). Case-insensitively deduplicated, first
    /// spelling wins.
    pub fn inventory_locations(&self) -> Result<Vec<String>> {
        self.with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT location_name FROM inventory ORDER BY location_name COLLATE NOCASE",
            )?;
            let names: Vec<String> = stmt
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut seen = std::collections::HashSet::new();
            Ok(names
                .into_iter()
                .filter(|n| seen.insert(n.to_lowercase()))
                .collect())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new(name: &str, quality: u32, loc: &str, qty: f64) -> NewInventoryEntry {
        NewInventoryEntry {
            entity_kind: Some("commodity".into()),
            entity_id: Some(7),
            name: name.into(),
            quality,
            location_id: None,
            location_name: loc.into(),
            quantity: qty,
            price: None,
        }
    }

    #[test]
    fn add_list_update_delete_roundtrip() {
        let c = Cache::open_in_memory().unwrap();
        let _a = c
            .inventory_add(new("Gold", 900, "Heim-Hangar", 12.0))
            .unwrap();
        let b = c
            .inventory_add(new("Gold", 364, "Meine Carrack", 3.0))
            .unwrap();
        // Overview order: location group, then name, then quality DESC.
        let list = c.inventory_list().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].location_name, "Heim-Hangar");
        assert_eq!(list[0].quality, 900);
        assert_eq!(list[1].location_name, "Meine Carrack");

        c.inventory_update(b, new("Gold", 420, "Heim-Hangar", 5.0))
            .unwrap();
        let list = c.inventory_list().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!((list[0].quality, list[1].quality), (900, 420));
        assert_eq!(list[1].quantity, 5.0);

        c.inventory_delete(b).unwrap();
        assert_eq!(c.inventory_list().unwrap().len(), 1);
        assert!(c.inventory_delete(b).is_err());
        assert!(c.inventory_update(99_999, new("x", 1, "y", 1.0)).is_err());
    }

    #[test]
    fn free_text_row_without_entity_reference() {
        let c = Cache::open_in_memory().unwrap();
        let e = NewInventoryEntry {
            entity_kind: None,
            entity_id: None,
            name: "Spare Ejection Seat".into(),
            quality: 750,
            location_id: None,
            location_name: "Org-Depot".into(),
            quantity: 1.0,
            price: Some(25_000.0),
        };
        let id = c.inventory_add(e).unwrap();
        let list = c.inventory_list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, id);
        assert_eq!(list[0].entity_kind, None);
        assert_eq!(list[0].entity_id, None);
        assert_eq!(list[0].price, Some(25_000.0));
    }

    #[test]
    fn quality_bounds_are_enforced() {
        let c = Cache::open_in_memory().unwrap();
        assert!(c.inventory_add(new("Gold", 1001, "x", 1.0)).is_err());
        assert!(c.inventory_add(new("Gold", 1000, "x", 1.0)).is_ok());
        assert!(c.inventory_add(new("", 500, "x", 1.0)).is_err());
        assert!(c.inventory_add(new("Gold", 500, "  ", 1.0)).is_err());
    }

    #[test]
    fn locations_dedupe_case_insensitive() {
        let c = Cache::open_in_memory().unwrap();
        c.inventory_add(new("Gold", 900, "Heim-Hangar", 1.0))
            .unwrap();
        c.inventory_add(new("Gold", 300, "heim-hangar", 1.0))
            .unwrap();
        c.inventory_add(new("Tit", 200, "Carrack", 1.0)).unwrap();
        assert_eq!(
            c.inventory_locations().unwrap(),
            ["Carrack", "Heim-Hangar"] // NOCASE-Sortierung, erste Schreibweise
        );
    }

    #[test]
    fn inventory_survives_reopen_and_is_untouched_by_v4_migration() {
        let dir = std::env::temp_dir().join(format!(
            "arch-inv-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("cache.sqlite3");
        let id = Cache::open(&path)
            .unwrap()
            .inventory_add(new("Gold", 900, "Depot", 2.0))
            .unwrap();
        // Reopen (runs the migration path again over an existing DB).
        let list = Cache::open(&path).unwrap().inventory_list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, id);
        std::fs::remove_dir_all(dir).ok();
    }

    /// t_1bab1fea blocker 2: ids cross IPC as strings (wiki ids > 2^53 lose
    /// precision as JS numbers). The frontend sends entity_id/location_id as
    /// strings — deserialize must parse them exactly, including negative wiki
    /// ids (two's-complement-wrapped u64), and re-serialize as strings.
    #[test]
    fn ids_cross_ipc_as_strings_and_roundtrip_losslessly() {
        // Wiki-scale id: above 2^53, a JS number would round this silently.
        const WIKI_LOC: &str = "9218937313948769813";
        // Negative wiki id (two's-complement u64 representation).
        const WIKI_NEG: &str = "-9216218779739218542";
        let json = format!(
            r#"{{"entity_kind":"item","entity_id":"{WIKI_LOC}","name":"Testar","quality":900,
                "location_id":"{WIKI_NEG}","location_name":"Mining Claim #QR4-25Q","quantity":1.0,"price":null}}"#
        );
        let new: NewInventoryEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(new.entity_id, Some(WIKI_LOC.parse::<u64>().unwrap()));
        assert_eq!(
            new.location_id,
            Some(WIKI_NEG.parse::<i64>().unwrap() as u64)
        );

        let c = Cache::open_in_memory().unwrap();
        let id = c.inventory_add(new).unwrap();
        let list = c.inventory_list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].entity_id, Some(WIKI_LOC.parse::<u64>().unwrap()));
        assert_eq!(
            list[0].location_id,
            Some(WIKI_NEG.parse::<i64>().unwrap() as u64)
        );

        // Wire format: ids must arrive as strings (not lossy JS numbers).
        // The negative wiki id is stored/returned in its two's-complement u64
        // representation — the cast chain (i64 → u64 → i64) is lossless, so
        // the exact wiki id survives every hop.
        let wrapped = WIKI_NEG.parse::<i64>().unwrap() as u64;
        let wire = serde_json::to_string(&list[0]).unwrap();
        assert!(wire.contains(&format!(r#""id":"{id}""#)), "wire={wire}");
        assert!(
            wire.contains(r#""entity_id":"9218937313948769813""#),
            "wire={wire}"
        );
        assert!(
            wire.contains(&format!(r#""location_id":"{wrapped}""#)),
            "wire={wire}"
        );

        // Bare numbers remain accepted (older callers / smoke stubs).
        let num: NewInventoryEntry =
            serde_json::from_str(r#"{"entity_id":42,"location_id":null,"name":"n","quality":1,"location_name":"l","quantity":1.0}"#).unwrap();
        assert_eq!(num.entity_id, Some(42));
        assert_eq!(num.location_id, None);
    }
}
