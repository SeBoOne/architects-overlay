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
/// Marketplace listings are realtime; Phase 2 caches the last snapshot for a
/// short TTL (5 min) so the offers view opens instantly from cache while the
/// background sync keeps it warm — rate-limit-schonend (one listing fetch per
/// TTL window at most).
pub const TTL_MARKETPLACE: Option<Duration> = Some(Duration::from_secs(300));

/// TTL for one reference dataset.
pub fn ttl_for(kind: EntityKind) -> Duration {
    match kind {
        EntityKind::Commodities => TTL_COMMODITIES,
        EntityKind::Items => TTL_ITEMS,
        EntityKind::Terminals => TTL_TERMINALS,
        // Star systems change on game-patch cadence, like terminals.
        EntityKind::StarSystems => TTL_TERMINALS,
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
-- Location price aggregate (feedback fix): the price table of ONE terminal
-- (all commodities it trades), fetched on demand per terminal and cached
-- with the same 30-min TTL as commodity prices.
CREATE TABLE IF NOT EXISTS terminal_prices (
  id_terminal  INTEGER NOT NULL,
  id_commodity INTEGER NOT NULL,
  commodity    TEXT    NOT NULL,
  buy_price    REAL    NOT NULL DEFAULT 0,
  sell_price   REAL    NOT NULL DEFAULT 0,
  buy_stock    REAL    NOT NULL DEFAULT 0,
  sell_stock   REAL    NOT NULL DEFAULT 0,
  buy_status   INTEGER NOT NULL DEFAULT 0,
  sell_status  INTEGER NOT NULL DEFAULT 0,
  quality      INTEGER NOT NULL DEFAULT 0,
  updated      INTEGER NOT NULL,
  fetched_at   INTEGER NOT NULL,
  PRIMARY KEY (id_terminal, id_commodity)
);
-- Item price table (feedback fix Punkt 8): buy/sell prices for ONE item
-- across all terminals that sell it, fetched on demand per item and cached
-- with the same 30-min TTL as commodity prices.
CREATE TABLE IF NOT EXISTS item_prices (
  id_item       INTEGER NOT NULL,
  id_terminal   INTEGER NOT NULL,
  commodity     TEXT    NOT NULL,
  buy_price     REAL    NOT NULL DEFAULT 0,
  sell_price    REAL    NOT NULL DEFAULT 0,
  buy_stock     REAL    NOT NULL DEFAULT 0,
  sell_stock    REAL    NOT NULL DEFAULT 0,
  buy_status    INTEGER NOT NULL DEFAULT 0,
  sell_status   INTEGER NOT NULL DEFAULT 0,
  quality       INTEGER NOT NULL DEFAULT 0,
  updated       INTEGER NOT NULL,
  fetched_at    INTEGER NOT NULL,
  PRIMARY KEY (id_item, id_terminal)
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
  region       TEXT    NOT NULL DEFAULT '',
  seller       TEXT    NOT NULL DEFAULT '',
  quality      INTEGER NOT NULL DEFAULT 0,
  updated      INTEGER NOT NULL,
  expires      INTEGER,
  slug         TEXT    NOT NULL DEFAULT '',
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

/// Freshness marker for one terminal's price table (location aggregate).
fn terminal_prices_meta_key(terminal_id: u64) -> String {
    format!("fetched_at:terminal_prices:{}", terminal_id)
}

/// Freshness marker for one item's price table (item detail).
fn item_prices_meta_key(item_id: u64) -> String {
    format!("fetched_at:item_prices:{}", item_id)
}

const MARKETPLACE_META_KEY: &str = "fetched_at:marketplace";

/// Columns added to `marketplace_offers` after the initial Phase-1 schema.
/// `CREATE TABLE IF NOT EXISTS` does not alter an existing table, so pre-Phase-2
/// caches need a column migration here.
const MARKETPLACE_MIGRATIONS: &[(&str, &str)] = &[
    (
        "region",
        "ALTER TABLE marketplace_offers ADD COLUMN region TEXT NOT NULL DEFAULT ''",
    ),
    (
        "slug",
        "ALTER TABLE marketplace_offers ADD COLUMN slug TEXT NOT NULL DEFAULT ''",
    ),
];

fn migrate_columns(conn: &Connection) -> Result<()> {
    for (column, alter) in MARKETPLACE_MIGRATIONS {
        let has = conn
            .prepare("SELECT 1 FROM pragma_table_info('marketplace_offers') WHERE name = ?1")
            .map_err(|e| CacheError::Open(format!("migration probe: {e}")))?
            .exists([column])
            .map_err(|e| CacheError::Open(format!("migration probe: {e}")))?;
        if !has {
            conn.execute(alter, [])
                .map_err(|e| CacheError::Open(format!("migration {alter}: {e}")))?;
        }
    }
    Ok(())
}

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
        migrate_columns(&conn)?;
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
    /// Empty query → browse mode: the first `limit` rows of the requested kinds
    /// (Arkanis: a bare `:c`/`:i`/`:l` prefix lists that category).
    pub fn search(&self, kinds: &[EntityKind], query: &str, limit: u32) -> Result<Vec<SearchHit>> {
        let query = query.trim();
        // rusqlite numbers parameters by token VALUE ("?1"?1" is ONE slot), so
        // each kind gets its own index.
        let placeholders = (1..=kinds.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let kind_params = kinds
            .iter()
            .map(|k| Box::new(k.as_str()) as Box<dyn rusqlite::ToSql>)
            .collect::<Vec<_>>();
        if query.is_empty() {
            // No FTS match expression to rank by — list the kind in id order.
            let sql = format!(
                "SELECT kind, entity_id, name, code, subtitle
                 FROM entities_fts
                 WHERE kind IN ({placeholders})
                 ORDER BY entity_id LIMIT ?{}",
                kinds.len() + 1
            );
            return self.with(|conn| {
                let mut stmt = conn.prepare(&sql)?;
                let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
                params.extend(kind_params);
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
            });
        }
        let escaped = query.replace('"', "\"\"");
        let match_expr = format!("\"{escaped}\"*");
        let sql = format!(
            "SELECT kind, entity_id, name, code, subtitle
             FROM entities_fts
             WHERE kind IN ({placeholders}) AND entities_fts MATCH ?{}
             ORDER BY rank LIMIT ?{}",
            kinds.len() + 1,
            kinds.len() + 2
        );
        self.with(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            params.extend(kind_params);
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

    /// All cached reference entities of one kind, id → row. Used for small
    /// lookups (e.g. star system id → region name) rather than FTS search.
    pub fn entities_of(&self, kind: EntityKind) -> Result<Vec<Entity>> {
        self.with(|conn| {
            let mut stmt =
                conn.prepare("SELECT id, name, code, subtitle FROM entities WHERE kind = ?1")?;
            let rows = stmt.query_map([kind.as_str()], |row| {
                Ok(Entity {
                    id: row.get::<_, i64>(0)? as u64,
                    kind,
                    name: row.get(1)?,
                    code: row.get(2)?,
                    subtitle: row.get(3)?,
                })
            })?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
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

    // ---------- terminal prices (location aggregate) ----------

    /// Whether the cached price table of `terminal_id` is younger than
    /// `TTL_PRICES`.
    pub fn terminal_prices_fresh(&self, terminal_id: u64) -> Result<bool> {
        let fetched = self.meta_get(&terminal_prices_meta_key(terminal_id))?;
        Ok(match fetched {
            Some(ts) => now_unix().saturating_sub(ts) < TTL_PRICES.as_secs(),
            None => false,
        })
    }

    /// Upsert one terminal's price table (key = terminal × commodity) and
    /// mark it fetched. Empty tables are allowed (a terminal trading nothing
    /// cached as "checked, nothing there"), so the next open does not refetch
    /// within the TTL window.
    pub fn store_terminal_prices(&self, terminal_id: u64, points: &[PricePoint]) -> Result<()> {
        self.with(|conn| {
            let tx = conn.transaction()?;
            if points.is_empty() {
                // No rows to upsert — still record that this terminal was
                // CHECKED now; an empty result must not trigger a refetch
                // loop while the marker is fresh.
                tx.execute(
                    "INSERT INTO meta (key, value) VALUES (?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    (terminal_prices_meta_key(terminal_id), now_unix().to_string()),
                )?;
                return tx.commit();
            }
            {
                let mut stmt = tx.prepare(
                    "INSERT INTO terminal_prices (id_terminal, id_commodity, commodity,
                                                  buy_price, sell_price, buy_stock, sell_stock,
                                                  buy_status, sell_status, quality, updated, fetched_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                     ON CONFLICT(id_terminal, id_commodity) DO UPDATE SET
                       commodity = excluded.commodity,
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
                        terminal_id as i64,
                        p.commodity_id as i64,
                        p.commodity,
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
                (terminal_prices_meta_key(terminal_id), now_unix().to_string()),
            )?;
            tx.commit()
        })
    }

    /// Cached price table of one terminal (may be stale — caller checks
    /// [`Cache::terminal_prices_fresh`] first).
    pub fn terminal_prices_for(&self, terminal_id: u64) -> Result<Vec<PricePoint>> {
        self.with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT commodity, id_commodity, buy_price, sell_price,
                        buy_stock, sell_stock, buy_status, sell_status, quality, updated
                 FROM terminal_prices WHERE id_terminal = ?1 ORDER BY commodity",
            )?;
            let rows = stmt.query_map([terminal_id as i64], |row| {
                Ok(PricePoint {
                    commodity: row.get(0)?,
                    commodity_id: row.get::<_, i64>(1)? as u64,
                    terminal_id,
                    location: String::new(),
                    buy_price: row.get(2)?,
                    sell_price: row.get(3)?,
                    buy_stock: row.get(4)?,
                    sell_stock: row.get(5)?,
                    buy_status: row.get::<_, i64>(6)? as u8,
                    sell_status: row.get::<_, i64>(7)? as u8,
                    quality: row.get::<_, i64>(8)? as u16,
                    updated: row.get::<_, i64>(9)? as u64,
                })
            })?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
    }

    // ---------- item prices ----------

    /// Whether cached price table of `item_id` is younger than `TTL_PRICES`.
    pub fn item_prices_fresh(&self, item_id: u64) -> Result<bool> {
        let fetched = self.meta_get(&item_prices_meta_key(item_id))?;
        Ok(match fetched {
            Some(ts) => now_unix().saturating_sub(ts) < TTL_PRICES.as_secs(),
            None => false,
        })
    }

    /// Upsert one item's price table (key = item × terminal) and mark it
    /// fetched. Empty tables are allowed.
    pub fn store_item_prices(&self, item_id: u64, points: &[PricePoint]) -> Result<()> {
        self.with(|conn| {
            let tx = conn.transaction()?;
            if points.is_empty() {
                tx.execute(
                    "INSERT INTO meta (key, value) VALUES (?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    (item_prices_meta_key(item_id), now_unix().to_string()),
                )?;
                return tx.commit();
            }
            {
                let mut stmt = tx.prepare(
                    "INSERT INTO item_prices (id_item, id_terminal, commodity,
                                              buy_price, sell_price, buy_stock, sell_stock,
                                              buy_status, sell_status, quality, updated, fetched_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                     ON CONFLICT(id_item, id_terminal) DO UPDATE SET
                       commodity = excluded.commodity,
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
                        item_id as i64,
                        p.terminal_id as i64,
                        p.commodity,
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
                (item_prices_meta_key(item_id), now_unix().to_string()),
            )?;
            tx.commit()
        })
    }

    /// Cached price table of one item (may be stale — caller checks
    /// [`Cache::item_prices_fresh`] first).
    pub fn item_prices_for(&self, item_id: u64) -> Result<Vec<PricePoint>> {
        self.with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT commodity, id_terminal, buy_price, sell_price,
                        buy_stock, sell_stock, buy_status, sell_status, quality, updated
                 FROM item_prices WHERE id_item = ?1 ORDER BY commodity",
            )?;
            let rows = stmt.query_map([item_id as i64], |row| {
                Ok(PricePoint {
                    commodity: row.get(0)?,
                    commodity_id: item_id,
                    terminal_id: row.get::<_, i64>(1)? as u64,
                    location: String::new(),
                    buy_price: row.get(2)?,
                    sell_price: row.get(3)?,
                    buy_stock: row.get(4)?,
                    sell_stock: row.get(5)?,
                    buy_status: row.get::<_, i64>(6)? as u8,
                    sell_status: row.get::<_, i64>(7)? as u8,
                    quality: row.get::<_, i64>(8)? as u16,
                    updated: row.get::<_, i64>(9)? as u64,
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

    /// Whether the cached marketplace snapshot is younger than `TTL_MARKETPLACE`
    /// (always false when the TTL is `None`).
    pub fn marketplace_fresh(&self) -> Result<bool> {
        let Some(ttl) = TTL_MARKETPLACE else {
            return Ok(false);
        };
        let fetched = self.meta_get(MARKETPLACE_META_KEY)?;
        Ok(match fetched {
            Some(ts) => now_unix().saturating_sub(ts) < ttl.as_secs(),
            None => false,
        })
    }

    /// Override the snapshot's fetched-at marker (unix seconds). The next
    /// [`Cache::marketplace_fresh`] call judges against this timestamp — used
    /// by tests to simulate an expired snapshot and by a future manual-refresh
    /// action to force staleness.
    pub fn set_marketplace_fetched_at(&self, unix_secs: u64) -> Result<()> {
        self.with(|conn| {
            conn.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                (MARKETPLACE_META_KEY, unix_secs.to_string()),
            )?;
            Ok(())
        })
    }

    /// Replace the marketplace snapshot (full snapshot semantics). The snapshot
    /// is marked fetched and counts as fresh for `TTL_MARKETPLACE`.
    pub fn store_marketplace(&self, offers: &[Offer]) -> Result<()> {
        self.with(|conn| {
            let tx = conn.transaction()?;
            tx.execute("DELETE FROM marketplace_offers", [])?;
            {
                let mut stmt = tx.prepare(
                    "INSERT INTO marketplace_offers (id, title, description, item_id,
                        category_id, operation, listing_type, price, currency, unit,
                        quantity, location, region, seller, quality, updated, expires, slug, fetched_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
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
                        o.region,
                        o.seller,
                        o.quality as i64,
                        o.updated as i64,
                        o.expires.map(|e| e as i64),
                        o.slug,
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
    /// always stale by design unless [`Cache::marketplace_fresh`]).
    pub fn marketplace_snapshot(&self) -> Result<Vec<Offer>> {
        self.with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, title, description, item_id, category_id, operation,
                        listing_type, price, currency, unit, quantity, location, region, seller,
                        quality, updated, expires, slug
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
                    region: row.get(12)?,
                    seller: row.get(13)?,
                    quality: row.get::<_, i64>(14)? as u16,
                    updated: row.get::<_, i64>(15)? as u64,
                    expires: row.get::<_, Option<i64>>(16)?.map(|e| e as u64),
                    slug: row.get(17)?,
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
    fn empty_query_browses_requested_kinds() {
        let cache = Cache::open_in_memory().unwrap();
        cache
            .store_entities(
                EntityKind::Commodities,
                &[
                    entity(1, EntityKind::Commodities, "Agricium", "AGRI", "Metal"),
                    entity(2, EntityKind::Commodities, "Titanium", "TITN", "Metal"),
                    entity(3, EntityKind::Commodities, "Spirits", "DISP", "Agri"),
                ],
            )
            .unwrap();
        cache
            .store_entities(
                EntityKind::Terminals,
                &[entity(
                    100,
                    EntityKind::Terminals,
                    "Area18",
                    "A18",
                    "Stanton",
                )],
            )
            .unwrap();

        // Bare `:c` → first rows of the commodity kind, in id order.
        let hits = cache.search(&[EntityKind::Commodities], "", 10).unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].name, "Agricium");
        assert_eq!(hits[2].name, "Spirits");

        // Multiple kinds + limit.
        let hits = cache
            .search(&[EntityKind::Commodities, EntityKind::Terminals], "  ", 2)
            .unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].name, "Agricium");
        assert_eq!(hits[1].name, "Titanium");

        // Kinds outside the request never leak in.
        let hits = cache.search(&[EntityKind::Terminals], "", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "Area18");
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
    fn terminal_prices_upsert_freshness_and_expiry() {
        let cache = Cache::open_in_memory().unwrap();
        let p = |c: u64, buy: f64, sell: f64| PricePoint {
            commodity: format!("Commodity {c}"),
            commodity_id: c,
            terminal_id: 436,
            location: String::new(),
            sell_price: sell,
            buy_price: buy,
            sell_stock: 0.0,
            buy_stock: 100.0,
            sell_status: 0,
            buy_status: 7,
            quality: 0,
            updated: 1_700_000_000,
        };
        assert!(!cache.terminal_prices_fresh(436).unwrap());
        cache
            .store_terminal_prices(436, &[p(3, 1108.0, 0.0), p(27, 1427.0, 0.0)])
            .unwrap();
        assert!(cache.terminal_prices_fresh(436).unwrap());
        let rows = cache.terminal_prices_for(436).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.terminal_id == 436));

        // upsert replaces in place, per-terminal isolation
        cache
            .store_terminal_prices(436, &[p(3, 1200.0, 0.0)])
            .unwrap();
        let rows = cache.terminal_prices_for(436).unwrap();
        assert_eq!(rows.len(), 2, "upsert must not duplicate rows");
        assert_eq!(
            rows.iter().find(|r| r.commodity_id == 3).unwrap().buy_price,
            1200.0
        );
        cache.store_terminal_prices(435, &[p(1, 9.0, 0.0)]).unwrap();
        assert!(cache.terminal_prices_fresh(435).unwrap());
        assert_eq!(cache.terminal_prices_for(435).unwrap().len(), 1);
        assert_eq!(cache.terminal_prices_for(436).unwrap().len(), 2);

        // an empty fetch must still mark the terminal as checked
        cache
            .with(|conn| {
                conn.execute(
                    "UPDATE meta SET value = ?1 WHERE key = ?2",
                    [
                        (now_unix() - TTL_PRICES.as_secs() - 1).to_string(),
                        terminal_prices_meta_key(428),
                    ],
                )?;
                Ok(())
            })
            .unwrap();
        cache.store_terminal_prices(428, &[]).unwrap();
        assert!(cache.terminal_prices_fresh(428).unwrap());
        assert!(cache.terminal_prices_for(428).unwrap().is_empty());

        // expiry
        cache
            .with(|conn| {
                conn.execute(
                    "UPDATE meta SET value = ?1 WHERE key = ?2",
                    [
                        (now_unix() - TTL_PRICES.as_secs() - 1).to_string(),
                        terminal_prices_meta_key(436),
                    ],
                )?;
                Ok(())
            })
            .unwrap();
        assert!(!cache.terminal_prices_fresh(436).unwrap());
    }

    #[test]
    fn terminal_prices_table_added_to_existing_db() {
        // Schema migration: a pre-fix cache has no `terminal_prices` table.
        // Opening it must create the table (CREATE IF NOT EXISTS) instead of
        // failing — existing installs keep their data and gain the location
        // price dataset.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.sqlite3");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
                .unwrap();
        }
        let cache = Cache::open(&path).unwrap();
        assert_eq!(cache.terminal_prices_for(436).unwrap().len(), 0);
        cache
            .store_terminal_prices(
                436,
                &[PricePoint {
                    commodity: "Agricium".into(),
                    commodity_id: 1,
                    terminal_id: 436,
                    location: String::new(),
                    sell_price: 0.0,
                    buy_price: 1108.0,
                    sell_stock: 0.0,
                    buy_stock: 0.0,
                    sell_status: 0,
                    buy_status: 1,
                    quality: 0,
                    updated: 1_700_000_000,
                }],
            )
            .unwrap();
        assert_eq!(cache.terminal_prices_for(436).unwrap().len(), 1);
        let _ = path;
        drop(dir);
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
            region: "Nyx".into(),
            seller: "alexlejero".into(),
            quality: 0,
            updated: 1_792_551_295,
            expires: Some(1_797_738_895),
            slug: "stark-compensator-2-UKCqAtSM3p".into(),
        };
        cache
            .store_marketplace(&[offer(1, "a"), offer(2, "b")])
            .unwrap();
        let snap = cache.marketplace_snapshot().unwrap();
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].id, 2, "snapshot ordered newest first");
        assert_eq!(snap[0].price, 400_000.0);
        assert_eq!(snap[0].slug, "stark-compensator-2-UKCqAtSM3p");
        assert_eq!(snap[0].region, "Nyx");
        // replacing wipes the old snapshot
        cache.store_marketplace(&[offer(3, "c")]).unwrap();
        assert_eq!(cache.marketplace_snapshot().unwrap().len(), 1);
    }

    #[test]
    fn marketplace_fresh_respects_ttl() {
        let cache = Cache::open_in_memory().unwrap();
        // nothing stored → not fresh
        assert!(!cache.marketplace_fresh().unwrap());
        cache.store_marketplace(&[]).unwrap();
        // an empty snapshot still marks fetched_at → fresh until TTL elapses
        assert!(cache.marketplace_fresh().unwrap());
        let old = now_unix() - TTL_MARKETPLACE.unwrap().as_secs() - 1;
        cache
            .with(|conn| {
                conn.execute(
                    "UPDATE meta SET value = ?1 WHERE key = ?2",
                    [old.to_string(), MARKETPLACE_META_KEY.to_string()],
                )?;
                Ok(())
            })
            .unwrap();
        assert!(!cache.marketplace_fresh().unwrap());
    }

    #[test]
    fn marketplace_schema_migrates_existing_db() {
        // Simulate a Phase-1 cache (marketplace_offers WITHOUT region/slug):
        // opening it must add the missing columns rather than fail.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.sqlite3");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(SCHEMA).unwrap();
            conn.execute_batch(
                "DROP TABLE marketplace_offers;
                 CREATE TABLE marketplace_offers (
                   id INTEGER PRIMARY KEY, title TEXT NOT NULL DEFAULT '',
                   description TEXT NOT NULL DEFAULT '', item_id INTEGER NOT NULL DEFAULT 0,
                   category_id INTEGER NOT NULL DEFAULT 0, operation TEXT NOT NULL DEFAULT 'sell',
                   listing_type TEXT NOT NULL DEFAULT 'item', price REAL NOT NULL DEFAULT 0,
                   currency TEXT NOT NULL DEFAULT 'UEC', unit TEXT NOT NULL DEFAULT '',
                   quantity INTEGER NOT NULL DEFAULT 0, location TEXT NOT NULL DEFAULT '',
                   seller TEXT NOT NULL DEFAULT '', quality INTEGER NOT NULL DEFAULT 0,
                   updated INTEGER NOT NULL, expires INTEGER, fetched_at INTEGER NOT NULL);",
            )
            .unwrap();
        }
        let cache = Cache::open(&path).unwrap();
        // New columns present after migration.
        cache
            .store_marketplace(&[Offer {
                id: 1,
                title: "x".into(),
                description: String::new(),
                item_id: 0,
                category_id: 0,
                operation: MarketOperation::Sell,
                listing_type: ListingType::Item,
                price: 1.0,
                currency: "UEC".into(),
                unit: "unit".into(),
                quantity: 1,
                location: "L".into(),
                region: "Stanton".into(),
                seller: "s".into(),
                quality: 0,
                updated: 1,
                expires: None,
                slug: "s".into(),
            }])
            .unwrap();
        let snap = cache.marketplace_snapshot().unwrap();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].region, "Stanton");
        assert_eq!(snap[0].slug, "s");
        let _ = path;
        drop(dir);
    }
}
