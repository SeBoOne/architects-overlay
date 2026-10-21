//! # cache
//!
//! SQLite-backed cache (rusqlite, bundled SQLite incl. FTS5) for everything the
//! providers fetch:
//!
//! * `entities` — reference datasets (commodities / items / terminals), keyed
//!   `(kind, id)`, plus an FTS5 index for instant search;
//! * `prices` — commodity price points per terminal, TTL **30 min**;
//! * `marketplace_offers` — last marketplace snapshot; never served as fresh
//!   (TTL `None`), kept for offline display only;
//! * `meta` — bookkeeping: `fetched_at` markers per key.
//!
//! TTLs follow the UEX research doc: commodities 1h, items 4h, terminals 12h,
//! prices 30min, marketplace no caching. `Cache` is a plain symbol behind a
//! `Mutex<Connection>` — SQLite ops are microsecond-scale here and never held
//! across an await point, so no pool is needed for the overlay's load.
//!
//! [`sync::spawn_sync`] provides the background refetch loop (tokio).

pub mod sync;

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use provider::{Entity, EntityKind, Offer, PricePoint, SearchHit};
use rusqlite::Connection;

/// Reference-data TTLs, from the research doc (patch-cycle data).
pub const TTL_COMMODITIES: Duration = Duration::from_secs(3_600); // 1h
pub const TTL_TERMINALS: Duration = Duration::from_secs(43_200); // 12h
pub const TTL_ITEMS: Duration = Duration::from_secs(14_400); // 4h
/// Prices are volatile (hourly updates upstream) — short TTL.
pub const TTL_PRICES: Duration = Duration::from_secs(1_800); // 30min
/// Marketplace listings are realtime; never served stale.
pub const TTL_MARKETPLACE: Option<Duration> = None;

/// TTL for one reference dataset.
pub fn ttl_for(kind: EntityKind) -> Duration {
    match kind {
        EntityKind::Commodities => TTL_COMMODITIES,
        EntityKind::Items => TTL_ITEMS,
        EntityKind::Terminals => TTL_TERMINALS,
    }
}

/// Cache failures (SQLite I/O, schema, …).
#[derive(Debug)]
pub enum CacheError {
    Open(String),
    Query(String),
}

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheError::Open(msg) => write!(f, "cannot open cache: {msg}"),
            CacheError::Query(msg) => write!(f, "cache query failed: {msg}"),
        }
    }
}

impl std::error::Error for CacheError {}

pub type Result<T> = std::result::Result<T, CacheError>;

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS entities (
  kind       TEXT    NOT NULL,
  id         INTEGER NOT NULL,
  name       TEXT    NOT NULL,
  code       TEXT,
  subtitle   TEXT    NOT NULL DEFAULT '',
  fetched_at INTEGER NOT NULL,
  PRIMARY KEY (kind, id)
);
CREATE VIRTUAL TABLE IF NOT EXISTS entities_fts USING fts5(
  kind UNINDEXED, entity_id UNINDEXED, name, code, subtitle
);
CREATE TABLE IF NOT EXISTS prices (
  id_commodity INTEGER NOT NULL,
  id_terminal  INTEGER NOT NULL,
  commodity    TEXT    NOT NULL,
  terminal     TEXT    NOT NULL,
  buy_price    REAL    NOT NULL DEFAULT 0,
  sell_price   REAL    NOT NULL DEFAULT 0,
  buy_stock    REAL    NOT NULL DEFAULT 0,
  sell_stock   REAL    NOT NULL DEFAULT 0,
  buy_status   INTEGER NOT NULL DEFAULT 0,
  sell_status  INTEGER NOT NULL DEFAULT 0,
  quality      INTEGER NOT NULL DEFAULT 0,
  updated      INTEGER NOT NULL,
  fetched_at   INTEGER NOT NULL,
  PRIMARY KEY (id_commodity, id_terminal)
);
CREATE TABLE IF NOT EXISTS marketplace_offers (
  id           INTEGER PRIMARY KEY,
  title        TEXT    NOT NULL DEFAULT '',
  description  TEXT    NOT NULL DEFAULT '',
  item_id      INTEGER NOT NULL DEFAULT 0,
  category_id  INTEGER NOT NULL DEFAULT 0,
  operation    TEXT    NOT NULL DEFAULT 'sell',
  listing_type TEXT    NOT NULL DEFAULT 'item',
  price        REAL    NOT NULL DEFAULT 0,
  currency     TEXT    NOT NULL DEFAULT 'UEC',
  unit         TEXT    NOT NULL DEFAULT '',
  quantity     INTEGER NOT NULL DEFAULT 0,
  location     TEXT    NOT NULL DEFAULT '',
  seller       TEXT    NOT NULL DEFAULT '',
  quality      INTEGER NOT NULL DEFAULT 0,
  updated      INTEGER NOT NULL,
  expires      INTEGER,
  fetched_at   INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
"#;

fn meta_key(kind: EntityKind) -> String {
    format!("fetched_at:{}", kind.as_str())
}

fn prices_meta_key(commodity_id: u64) -> String {
    format!("fetched_at:prices:{}", commodity_id)
}

const MARKETPLACE_META_KEY: &str = "fetched_at:marketplace";

/// The local SQLite cache. Share via `Arc<Cache>`.
pub struct Cache {
    conn: Mutex<Connection>,
}

impl Cache {
    /// Open (or create) the cache database at `path`. Parent directories are
    /// created on demand.
    pub fn open(path: &Path) -> Result<Cache> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| CacheError::Open(e.to_string()))?;
        }
        let conn = Connection::open(path).map_err(|e| CacheError::Open(e.to_string()))?;
        Self::init(conn)
    }

    /// In-memory database, for tests / short-lived sessions.
    pub fn open_in_memory() -> Result<Cache> {
        let conn = Connection::open_in_memory().map_err(|e| CacheError::Open(e.to_string()))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Cache> {
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| CacheError::Open(e.to_string()))?;
        conn.execute_batch(SCHEMA)
            .map_err(|e| CacheError::Open(format!("schema: {e}")))?;
        Ok(Cache {
            conn: Mutex::new(conn),
        })
    }

    fn with<T>(&self, f: impl FnOnce(&mut Connection) -> rusqlite::Result<T>) -> Result<T> {
        let mut conn = self.conn.lock().expect("cache mutex poisoned");
        f(&mut conn).map_err(|e| CacheError::Query(e.to_string()))
    }

    // ---------- reference entities ----------

    /// Whether the reference dataset of `kind` is fresher than its TTL.
    pub fn is_fresh(&self, kind: EntityKind) -> Result<bool> {
        let fetched = self.meta_get(&meta_key(kind))?;
        Ok(match fetched {
            Some(ts) => now_unix().saturating_sub(ts) < ttl_for(kind).as_secs(),
            None => false,
        })
    }

    /// Replace the reference dataset of one kind (full refresh → DELETE +
    /// re-insert, also rebuilding the FTS index for that kind).
    pub fn store_entities(&self, kind: EntityKind, entities: &[Entity]) -> Result<()> {
        let kind_str = kind.as_str();
        self.with(|conn| {
            let tx = conn.transaction()?;
            tx.execute("DELETE FROM entities WHERE kind = ?1", [kind_str])?;
            tx.execute("DELETE FROM entities_fts WHERE kind = ?1", [kind_str])?;
            {
                let mut stmt = tx.prepare(
                    "INSERT INTO entities (kind, id, name, code, subtitle, fetched_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )?;
                for e in entities {
                    stmt.execute(rusqlite::params![
                        kind_str,
                        e.id as i64,
                        e.name,
                        e.code,
                        e.subtitle,
                        now_unix() as i64
                    ])?;
                }
            }
            {
                let mut stmt = tx.prepare(
                    "INSERT INTO entities_fts (kind, entity_id, name, code, subtitle)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )?;
                for e in entities {
                    stmt.execute(rusqlite::params![
                        kind_str,
                        e.id as i64,
                        e.name,
                        e.code.clone().unwrap_or_default(),
                        e.subtitle
                    ])?;
                }
            }
            tx.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                (meta_key(kind), now_unix().to_string()),
            )?;
            tx.commit()
        })
    }

    /// FTS5 prefix search across the given kinds (`ORDER BY rank` = best match
    /// first). `query` is escaped; a literal `*`-suffix enables prefix matching.
    /// Empty query → no rows.
    pub fn search(&self, kinds: &[EntityKind], query: &str, limit: u32) -> Result<Vec<SearchHit>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let escaped = query.replace('"', "\"\"");
        let match_expr = format!("\"{escaped}\"*");
        let mut placeholders = String::from("?1,").repeat(kinds.len());
        placeholders.pop(); // drop trailing comma
        let sql = format!(
            "SELECT kind, entity_id, name, code, subtitle
             FROM entities_fts
             WHERE kind IN ({placeholders}) AND entities_fts MATCH ?2
             ORDER BY rank LIMIT ?3"
        );
        self.with(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            for k in kinds {
                params.push(Box::new(k.as_str()));
            }
            params.push(Box::new(match_expr));
            params.push(Box::new(limit as i64));
            let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
                Ok(SearchHit {
                    id: row.get::<_, i64>(1)? as u64,
                    name: row.get(2)?,
                    kind: row.get(0)?,
                    code: row.get(3)?,
                    subtitle: row.get(4)?,
                })
            })?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
    }

    // ---------- bookkeeping ----------

    /// Seconds since the reference dataset of `kind` was fetched
    /// (`Ok(None)` = never fetched). Feeds the UI's freshness indicator.
    pub fn dataset_age(&self, kind: EntityKind) -> Result<Option<u64>> {
        Ok(self
            .meta_get(&meta_key(kind))?
            .map(|ts| now_unix().saturating_sub(ts)))
    }

    /// Number of cached reference entities of one kind (sync progress UI).
    pub fn count_entities(&self, kind: EntityKind) -> Result<u64> {
        self.with(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM entities WHERE kind = ?1",
                [kind.as_str()],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n as u64)
        })
    }

    // ---------- prices ----------

    /// Whether cached prices for `commodity_id` are younger than `TTL_PRICES`.
    pub fn prices_fresh(&self, commodity_id: u64) -> Result<bool> {
        let fetched = self.meta_get(&prices_meta_key(commodity_id))?;
        Ok(match fetched {
            Some(ts) => now_unix().saturating_sub(ts) < TTL_PRICES.as_secs(),
            None => false,
        })
    }

    /// Upsert price points (key = commodity × terminal) and mark them fetched.
    pub fn store_prices(&self, points: &[PricePoint]) -> Result<()> {
        if points.is_empty() {
            return Ok(());
        }
        let commodity_id = points[0].commodity_id;
        self.with(|conn| {
            let tx = conn.transaction()?;
            {
                let mut stmt = tx.prepare(
                    "INSERT INTO prices (id_commodity, id_terminal, commodity, terminal,
                                          buy_price, sell_price, buy_stock, sell_stock,
                                          buy_status, sell_status, quality, updated, fetched_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                     ON CONFLICT(id_commodity, id_terminal) DO UPDATE SET
                       commodity = excluded.commodity,
                       terminal = excluded.terminal,
                       buy_price = excluded.buy_price,
                       sell_price = excluded.sell_price,
                       buy_stock = excluded.buy_stock,
                       sell_stock = excluded.sell_stock,
                       buy_status = excluded.buy_status,
                       sell_status = excluded.sell_status,
                       quality = excluded.quality,
                       updated = excluded.updated,
                       fetched_at = excluded.fetched_at",
                )?;
                let now = now_unix() as i64;
                for p in points {
                    stmt.execute(rusqlite::params![
                        p.commodity_id as i64,
                        p.terminal_id as i64,
                        p.commodity,
                        p.location,
                        p.buy_price,
                        p.sell_price,
                        p.buy_stock,
                        p.sell_stock,
                        p.buy_status as i64,
                        p.sell_status as i64,
                        p.quality as i64,
                        p.updated as i64,
                        now
                    ])?;
                }
            }
            tx.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                (prices_meta_key(commodity_id), now_unix().to_string()),
            )?;
            tx.commit()
        })
    }

    /// Cached price points for one commodity (may be stale — caller checks
    /// [`Cache::prices_fresh`] first).
    pub fn prices_for(&self, commodity_id: u64) -> Result<Vec<PricePoint>> {
        self.with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT commodity, id_terminal, terminal, buy_price, sell_price,
                        buy_stock, sell_stock, buy_status, sell_status, quality, updated
                 FROM prices WHERE id_commodity = ?1 ORDER BY terminal",
            )?;
            let rows = stmt.query_map([commodity_id as i64], |row| {
                Ok(PricePoint {
                    commodity: row.get(0)?,
                    commodity_id,
                    terminal_id: row.get::<_, i64>(1)? as u64,
                    location: row.get(2)?,
                    buy_price: row.get(3)?,
                    sell_price: row.get(4)?,
                    buy_stock: row.get(5)?,
                    sell_stock: row.get(6)?,
                    buy_status: row.get::<_, i64>(7)? as u8,
                    sell_status: row.get::<_, i64>(8)? as u8,
                    quality: row.get::<_, i64>(9)? as u16,
                    updated: row.get::<_, i64>(10)? as u64,
                })
            })?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
    }

    // ---------- marketplace ----------

    /// Replace the marketplace snapshot (full snapshot semantics). The snapshot
    /// is marked fetched but is *never* considered fresh (TTL `None`).
    pub fn store_marketplace(&self, offers: &[Offer]) -> Result<()> {
        self.with(|conn| {
            let tx = conn.transaction()?;
            tx.execute("DELETE FROM marketplace_offers", [])?;
            {
                let mut stmt = tx.prepare(
                    "INSERT INTO marketplace_offers (id, title, description, item_id,
                        category_id, operation, listing_type, price, currency, unit,
                        quantity, location, seller, quality, updated, expires, fetched_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
                )?;
                let now = now_unix() as i64;
                for o in offers {
                    stmt.execute(rusqlite::params![
                        o.id as i64,
                        o.title,
                        o.description,
                        o.item_id as i64,
                        o.category_id as i64,
                        o.operation.as_str(),
                        o.listing_type.as_str(),
                        o.price,
                        o.currency,
                        o.unit,
                        o.quantity as i64,
                        o.location,
                        o.seller,
                        o.quality as i64,
                        o.updated as i64,
                        o.expires.map(|e| e as i64),
                        now
                    ])?;
                }
            }
            tx.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                (MARKETPLACE_META_KEY, now_unix().to_string()),
            )?;
            tx.commit()
        })
    }

    /// The last marketplace snapshot, newest first (offline display only —
    /// always stale by design).
    pub fn marketplace_snapshot(&self) -> Result<Vec<Offer>> {
        self.with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, title, description, item_id, category_id, operation,
                        listing_type, price, currency, unit, quantity, location, seller,
                        quality, updated, expires
                 FROM marketplace_offers ORDER BY updated DESC, id DESC",
            )?;
            let rows = stmt.query_map([], |row| {
                let operation: String = row.get(5)?;
                let listing_type: String = row.get(6)?;
                Ok(Offer {
                    id: row.get::<_, i64>(0)? as u64,
                    title: row.get(1)?,
                    description: row.get(2)?,
                    item_id: row.get::<_, i64>(3)? as u64,
                    category_id: row.get::<_, i64>(4)? as u64,
                    operation: operation.parse().unwrap_or(provider::MarketOperation::Sell),
                    listing_type: listing_type.parse().unwrap_or(provider::ListingType::Item),
                    price: row.get(7)?,
                    currency: row.get(8)?,
                    unit: row.get(9)?,
                    quantity: row.get::<_, i64>(10)? as u64,
                    location: row.get(11)?,
                    seller: row.get(12)?,
                    quality: row.get::<_, i64>(13)? as u16,
                    updated: row.get::<_, i64>(14)? as u64,
                    expires: row.get::<_, Option<i64>>(15)?.map(|e| e as u64),
                })
            })?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
    }

    fn meta_get(&self, key: &str) -> Result<Option<u64>> {
        self.with(|conn| {
            let mut stmt = conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
            let mut rows = stmt.query([key])?;
            match rows.next()? {
                Some(row) => Ok(row
                    .get::<_, Option<String>>(0)?
                    .and_then(|v| v.parse().ok())),
                None => Ok(None),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use provider::{ListingType, MarketOperation};

    fn entity(id: u64, kind: EntityKind, name: &str, code: &str, subtitle: &str) -> Entity {
        Entity {
            id,
            kind,
            name: name.to_string(),
            code: Some(code.to_string()),
            subtitle: subtitle.to_string(),
        }
    }

    #[test]
    fn store_and_search_entities() {
        let cache = Cache::open_in_memory().unwrap();
        assert!(!cache.is_fresh(EntityKind::Commodities).unwrap());

        cache
            .store_entities(
                EntityKind::Commodities,
                &[
                    entity(1, EntityKind::Commodities, "Agricium", "AGRI", "Metal"),
                    entity(2, EntityKind::Commodities, "Titanium", "TITN", "Metal"),
                    entity(
                        3,
                        EntityKind::Commodities,
                        "Distilled Spirits",
                        "DISP",
                        "Agricultural",
                    ),
                ],
            )
            .unwrap();
        assert!(cache.is_fresh(EntityKind::Commodities).unwrap());

        // prefix match — "agri" also hits the subtitle "Agricultural"
        // (Distilled Spirits); the name match must be among the results
        let hits = cache
            .search(&[EntityKind::Commodities], "agri", 10)
            .unwrap();
        assert_eq!(hits.len(), 2);
        assert!(
            hits.iter().any(|h| h.name == "Agricium"),
            "name match must rank first: {hits:?}"
        );
        assert_eq!(hits[0].name, "Agricium");
        assert_eq!(hits[0].kind, "commodity");
        assert_eq!(hits[0].code.as_deref(), Some("AGRI"));

        // code search
        let hits = cache
            .search(&[EntityKind::Commodities], "titn", 10)
            .unwrap();
        assert_eq!(hits[0].code.as_deref(), Some("TITN"));

        // phrase prefix: "distilled s" matches both words as a phrase
        let hits = cache
            .search(&[EntityKind::Commodities], "distilled", 10)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "Distilled Spirits");

        // kind filtering
        cache
            .store_entities(
                EntityKind::Terminals,
                &[entity(
                    10,
                    EntityKind::Terminals,
                    "Area 18",
                    "TDA18",
                    "Stanton > ArcCorp",
                )],
            )
            .unwrap();
        let hits = cache
            .search(&[EntityKind::Commodities], "area", 10)
            .unwrap();
        assert!(
            hits.is_empty(),
            "terminal must not leak into commodity search"
        );
        let hits = cache.search(&[EntityKind::Terminals], "area", 10).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn search_escapes_user_input() {
        let cache = Cache::open_in_memory().unwrap();
        // Quote characters must not break the FTS query or leak into SQL.
        let hits = cache
            .search(&[EntityKind::Commodities], "\" OR 1=1 --", 10)
            .unwrap();
        assert!(hits.is_empty());
        let hits = cache.search(&[EntityKind::Commodities], "   ", 10).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn ttl_expiry() {
        let cache = Cache::open_in_memory().unwrap();
        cache
            .store_entities(
                EntityKind::Commodities,
                &[entity(
                    1,
                    EntityKind::Commodities,
                    "Agricium",
                    "AGRI",
                    "Metal",
                )],
            )
            .unwrap();
        assert!(cache.is_fresh(EntityKind::Commodities).unwrap());
        // Rewind the stored timestamp beyond the TTL.
        let old = now_unix() - ttl_for(EntityKind::Commodities).as_secs() - 1;
        cache
            .with(|conn| {
                conn.execute(
                    "UPDATE meta SET value = ?1 WHERE key = ?2",
                    [old.to_string(), meta_key(EntityKind::Commodities)],
                )?;
                Ok(())
            })
            .unwrap();
        assert!(!cache.is_fresh(EntityKind::Commodities).unwrap());
    }

    #[test]
    fn prices_upsert_and_expiry() {
        let cache = Cache::open_in_memory().unwrap();
        let p = |t: u64, sell: f64| PricePoint {
            commodity: "Agricium".into(),
            commodity_id: 1,
            location: format!("Terminal {t}"),
            terminal_id: t,
            sell_price: sell,
            buy_price: 0.0,
            sell_stock: 100.0,
            buy_stock: 0.0,
            sell_status: 2,
            buy_status: 0,
            quality: 10,
            updated: 1_700_000_000,
        };
        cache
            .store_prices(&[p(12, 9_725.0), p(13, 9_600.0)])
            .unwrap();
        assert!(cache.prices_fresh(1).unwrap());
        let rows = cache.prices_for(1).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].terminal_id, 12);

        // upsert updates in place
        cache.store_prices(&[p(12, 9_900.0)]).unwrap();
        let rows = cache.prices_for(1).unwrap();
        assert_eq!(rows.len(), 2, "upsert must not duplicate rows");
        assert_eq!(
            rows.iter()
                .find(|r| r.terminal_id == 12)
                .unwrap()
                .sell_price,
            9_900.0
        );

        // expiry
        let old = now_unix() - TTL_PRICES.as_secs() - 1;
        cache
            .with(|conn| {
                conn.execute(
                    "UPDATE meta SET value = ?1 WHERE key = ?2",
                    [old.to_string(), prices_meta_key(1)],
                )?;
                Ok(())
            })
            .unwrap();
        assert!(!cache.prices_fresh(1).unwrap());
    }

    #[test]
    fn marketplace_snapshot_roundtrip() {
        let cache = Cache::open_in_memory().unwrap();
        let offer = |id: u64, title: &str| Offer {
            id,
            title: title.into(),
            description: "desc".into(),
            item_id: 5332,
            category_id: 17,
            operation: MarketOperation::Sell,
            listing_type: ListingType::Item,
            price: 400_000.0,
            currency: "UEC".into(),
            unit: "unit".into(),
            quantity: 8,
            location: "Levski".into(),
            seller: "alexlejero".into(),
            quality: 0,
            updated: 1_792_551_295,
            expires: Some(1_797_738_895),
        };
        cache
            .store_marketplace(&[offer(1, "a"), offer(2, "b")])
            .unwrap();
        let snap = cache.marketplace_snapshot().unwrap();
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].id, 2, "snapshot ordered newest first");
        assert_eq!(snap[0].price, 400_000.0);
        // replacing wipes the old snapshot
        cache.store_marketplace(&[offer(3, "c")]).unwrap();
        assert_eq!(cache.marketplace_snapshot().unwrap().len(), 1);
    }
}
