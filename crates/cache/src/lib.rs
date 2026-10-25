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

pub mod inventory;
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
/// Wiki reference data changes on game-patch cadence, not hourly — the sync
/// pulls it at most twice a day (research report: locations ~2k rows, static
/// between patches; commodities likewise).
pub const TTL_WIKI_REFERENCE: Duration = Duration::from_secs(43_200); // 12h
/// Marketplace listings are realtime; Phase 2 caches the last snapshot for a
/// short TTL (5 min) so the offers view opens instantly from cache while the
/// background sync keeps it warm — rate-limit-schonend (one listing fetch per
/// TTL window at most).
pub const TTL_MARKETPLACE: Option<Duration> = Some(Duration::from_secs(300));

/// Data-source tags stored per entity/price row (`entities.source`,
/// `prices.source`, …). Legacy rows (pre-scheme v2) are all `uex`.
pub const SOURCE_UEX: &str = "uex";
pub const SOURCE_WIKI: &str = "wiki";

/// TTL for one reference dataset. Wiki-sourced rows live on the patch-driven
/// long TTL, UEX rows keep the research-doc values.
pub fn ttl_for_source(kind: EntityKind, source: &str) -> Duration {
    if source == SOURCE_WIKI {
        return TTL_WIKI_REFERENCE;
    }
    match kind {
        EntityKind::Commodities => TTL_COMMODITIES,
        EntityKind::Items => TTL_ITEMS,
        EntityKind::Terminals => TTL_TERMINALS,
        EntityKind::Locations => TTL_TERMINALS,
        // Star systems change on game-patch cadence, like terminals.
        EntityKind::StarSystems => TTL_TERMINALS,
    }
}

/// TTL for one reference dataset (UEX rows — the historical default).
pub fn ttl_for(kind: EntityKind) -> Duration {
    ttl_for_source(kind, SOURCE_UEX)
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
  source     TEXT    NOT NULL DEFAULT 'uex',
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
  source       TEXT    NOT NULL DEFAULT 'uex',
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
  source       TEXT    NOT NULL DEFAULT 'uex',
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
-- Lokal-privates Inventar (Karte t_1bab1fea, v0.3.0 K2). Eine Zeile = eine
-- Position: Item-/Commodity-Referenz (entity_kind/entity_id, name aufgelöst)
-- ODER Freitext (beide NULL), Qualitaet 0–1000 (Pflicht), Standort als
-- Location-Entity-Referenz ODER Freitext-Depot (location_id NULL), Menge
-- (SCU bzw. Stueck), optional Kaufpreis aUEC. KEIN Sync, KEIN API-Zugriff —
-- nur die App schreibt hier.
CREATE TABLE IF NOT EXISTS inventory (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  entity_kind   TEXT,
  entity_id     INTEGER,
  name          TEXT    NOT NULL,
  quality       INTEGER NOT NULL,
  location_id   INTEGER,
  location_name TEXT    NOT NULL,
  quantity      REAL    NOT NULL DEFAULT 0,
  price         REAL,
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
-- Handelsrouten (v6, Karte t_128b7547): JSON-Metadatensätze hinter einem
-- Frischemarkierer — Terminal-Flags (Autoload/Cargodeck/Boden/Kette) und
-- Ware-Flags (illegal/sellable/buyable) aus UEX, plus der Routen-Sweep-
-- Zeitstempel. Payloads sind klein (einige 100 KB) und leben bewusst NICHT
-- in den Entity-Tabellen (dort sind nur die Such-Felder).
CREATE TABLE IF NOT EXISTS route_meta (
  key        TEXT PRIMARY KEY,
  fetched_at INTEGER NOT NULL,
  payload    TEXT    NOT NULL
);
-- Hangar (v4): user-owned private data — pledge/ingame ships, vehicles and
-- modules. `origin` distinguishes pledge vs ingame purchases; `insurance`
-- carries the pledge's insurance (LTI/Standard/…) and is NULL for ingame.
-- Modules may reference their base ship by name.
CREATE TABLE IF NOT EXISTS hangar (
  id             INTEGER PRIMARY KEY,
  name           TEXT    NOT NULL,
  origin         TEXT    NOT NULL DEFAULT 'pledge',
  insurance      TEXT,
  custom_name    TEXT    NOT NULL DEFAULT '',
  entity_type    TEXT    NOT NULL DEFAULT 'ship',
  base_ship      TEXT    NOT NULL DEFAULT '',
  ship_code      TEXT    NOT NULL DEFAULT '',
  manufacturer   TEXT    NOT NULL DEFAULT '',
  purchase_price REAL,
  purchase_date  TEXT    NOT NULL DEFAULT '',
  pledge_id      TEXT    NOT NULL DEFAULT '',
  pledge_name    TEXT    NOT NULL DEFAULT '',
  warbond        INTEGER NOT NULL DEFAULT 0,
  ccud           INTEGER NOT NULL DEFAULT 0,
  source         TEXT    NOT NULL DEFAULT '',
  created_at     INTEGER NOT NULL,
  updated_at     INTEGER NOT NULL
);
"#;

fn meta_key(kind: EntityKind) -> String {
    format!("fetched_at:{}", kind.as_str())
}

/// Freshness marker key per (kind, source). The plain UEX key stays in its
/// historical form so pre-v2 caches keep their freshness markers.
fn meta_key_sourced(kind: EntityKind, source: &str) -> String {
    if source == SOURCE_UEX {
        meta_key(kind)
    } else {
        format!("fetched_at:{}@{}", kind.as_str(), source)
    }
}

fn prices_meta_key(commodity_id: u64) -> String {
    format!("fetched_at:prices:{}", commodity_id)
}

fn prices_meta_key_sourced(commodity_id: u64, source: &str) -> String {
    if source == SOURCE_UEX {
        prices_meta_key(commodity_id)
    } else {
        format!("fetched_at:prices:{}@{}", commodity_id, source)
    }
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

/// Cache schema version (`PRAGMA user_version`).
///
/// * v1 (implicit legacy) — no `source` columns.
/// * v2 — wiki-provider migration: `entities`, `prices`, `terminal_prices`,
///   `item_prices` gain a `source TEXT NOT NULL DEFAULT 'uex'` column; every
///   pre-existing row is attributed to UEX (the only source up to v1), so
///   legacy data stays valid and is NOT invalidated — only wiki rows carry
///   `source = 'wiki'`. Freshness is tracked per (kind, source), so the first
///   wiki sync is triggered by the wiki dataset simply being absent.
/// * v3 — Feedback-Runde 4: einmaliger UEX-Terminal-Purge (Service-Terminals
///   vor dem Typ-Filter); Terminal-Dataset wird beim nächsten ensure neu
///   importiert.
/// * v4 — Inventar-Modus (Karte t_1bab1fea): neue Tabelle `inventory`
///   (lokal-privat, nur App-Schreibzugriff, siehe `inventory`-Modul). Der
///   Bump ist rein additiv: `CREATE TABLE IF NOT EXISTS` im SCHEMA erzeugt
///   die Tabelle auf jedem Open (auch auf Bestands-Caches); die Migration
///   läuft daher einmalig durch und verändert bestehende Daten NICHT.
/// * v5 — Hangar-Modus (Karte t_317f19c5): neue Tabelle `hangar` (Schiffe,
///   Fahrzeuge, Module mit Herkunft pledge/ingame + Versicherung); die
///   `source`-Spalte vermerkt die Herkunft (manual > xplorer > ccugame) für
///   Import-Merges. Bump ebenfalls rein additiv.
pub const SCHEMA_VERSION: i64 = 5;

/// Column migrations for the v1 → v2 bump. `CREATE TABLE IF NOT EXISTS` never
/// alters an existing table, so older caches need the ALTERs here.
const V2_SOURCE_MIGRATIONS: &[(&str, &str)] = &[
    (
        "entities",
        "ALTER TABLE entities ADD COLUMN source TEXT NOT NULL DEFAULT 'uex'",
    ),
    (
        "prices",
        "ALTER TABLE prices ADD COLUMN source TEXT NOT NULL DEFAULT 'uex'",
    ),
    (
        "terminal_prices",
        "ALTER TABLE terminal_prices ADD COLUMN source TEXT NOT NULL DEFAULT 'uex'",
    ),
    (
        "item_prices",
        "ALTER TABLE item_prices ADD COLUMN source TEXT NOT NULL DEFAULT 'uex'",
    ),
];

/// Bring a pre-v2 cache to the current schema version: add the missing
/// `source` columns and stamp `PRAGMA user_version`. Legacy rows keep their
/// data (attributed to `uex`).
fn migrate_source_columns(conn: &Connection) -> Result<()> {
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| CacheError::Open(format!("version probe: {e}")))?;
    if version >= SCHEMA_VERSION {
        return Ok(());
    }
    for (table, alter) in V2_SOURCE_MIGRATIONS {
        let has = conn
            .prepare(&format!(
                "SELECT 1 FROM pragma_table_info('{table}') WHERE name = 'source'"
            ))
            .map_err(|e| CacheError::Open(format!("migration probe: {e}")))?
            .exists([])
            .map_err(|e| CacheError::Open(format!("migration probe: {e}")))?;
        if !has {
            conn.execute(alter, [])
                .map_err(|e| CacheError::Open(format!("migration {alter}: {e}")))?;
        }
    }
    // v3: einmaliger Terminal-Purge (Service-Terminals vor dem Typ-Filter) —
    // nur für Caches, die v3 noch NIE gesehen haben (version < 3); spätere
    // Migrationen (v4 → v5) dürfen den Purge nicht wiederholen.
    if version < 3 {
        purge_service_terminals(conn)?;
    }
    // v5: `hangar.source` — Quellen-Stempel für die Namens-Priorität beim
    // Import-Merge. Bestehende Zeilen behalten ihre Daten (source = '').
    let hangar_has_source = conn
        .prepare("SELECT 1 FROM pragma_table_info('hangar') WHERE name = 'source'")
        .map_err(|e| CacheError::Open(format!("migration probe: {e}")))?
        .exists([])
        .map_err(|e| CacheError::Open(format!("migration probe: {e}")))?;
    if !hangar_has_source {
        conn.execute(
            "ALTER TABLE hangar ADD COLUMN source TEXT NOT NULL DEFAULT ''",
            [],
        )
        .map_err(|e| CacheError::Open(format!("migration hangar.source: {e}")))?;
    }
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|e| CacheError::Open(format!("version bump: {e}")))?;
    Ok(())
}

/// v2 → v3 data cleanup (Feedback-Runde 4 / P2): terminals imported BEFORE
/// the trade-type filter existed still sit in the cache — Landing Services,
/// Refinement Centers, rentals & co. would keep polluting search and the
/// location aggregates until their (long) TTL expired. The version-gated
/// migration (runs exactly once, when user_version < 3) drops the whole UEX
/// terminal dataset (FTS rows included) and clears its freshness marker, so
/// the next ensure/search re-imports with the filter applied.
fn purge_service_terminals(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM entities_fts WHERE kind = 'terminal'", [])
        .map_err(|e| CacheError::Open(format!("v3 terminal purge (fts): {e}")))?;
    conn.execute("DELETE FROM entities WHERE kind = 'terminal'", [])
        .map_err(|e| CacheError::Open(format!("v3 terminal purge: {e}")))?;
    conn.execute(
        "DELETE FROM meta WHERE key = ?1",
        [meta_key(EntityKind::Terminals)],
    )
    .map_err(|e| CacheError::Open(format!("v3 terminal purge (meta): {e}")))?;
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
        migrate_source_columns(&conn)?;
        Ok(Cache {
            conn: Mutex::new(conn),
        })
    }

    /// Führt eine closure mit exklusivem DB-Access aus. Öffentlich für
    /// Besitzer-Datenmodule (z. B. hangar), die ihre eigene Tabelle pflegen.
    pub fn with<T>(&self, f: impl FnOnce(&mut Connection) -> rusqlite::Result<T>) -> Result<T> {
        let mut conn = self.conn.lock().expect("cache mutex poisoned");
        f(&mut conn).map_err(|e| CacheError::Query(e.to_string()))
    }

    // ---------- reference entities ----------

    /// Whether the reference dataset of `kind` is fresher than its TTL.
    pub fn is_fresh(&self, kind: EntityKind) -> Result<bool> {
        self.is_fresh_sourced(kind, SOURCE_UEX)
    }

    /// Freshness per (kind, source) — wiki rows carry their own fetched-at
    /// marker and the patch-driven TTL ([`ttl_for_source`]).
    pub fn is_fresh_sourced(&self, kind: EntityKind, source: &str) -> Result<bool> {
        let fetched = self.meta_get(&meta_key_sourced(kind, source))?;
        Ok(match fetched {
            Some(ts) => now_unix().saturating_sub(ts) < ttl_for_source(kind, source).as_secs(),
            None => false,
        })
    }

    /// Replace the reference dataset of one kind (full refresh → DELETE +
    /// re-insert, also rebuilding the FTS index for that kind).
    pub fn store_entities(&self, kind: EntityKind, entities: &[Entity]) -> Result<()> {
        self.store_entities_sourced(kind, SOURCE_UEX, entities)
    }

    /// Replace one kind's dataset for a named source (`uex` | `wiki`). Only
    /// rows of this (kind, source) pair are deleted, so UEX and wiki datasets
    /// can coexist in one table.
    pub fn store_entities_sourced(
        &self,
        kind: EntityKind,
        source: &str,
        entities: &[Entity],
    ) -> Result<()> {
        let kind_str = kind.as_str();
        // Dedupe by id within the batch (keep first): two source rows with
        // the same id would violate the (kind, id) primary key and sink the
        // whole dataset (e.g. wiki rows with an empty uuid all hash to one
        // id).
        let mut seen_ids: std::collections::HashSet<u64> = std::collections::HashSet::new();
        let entities: Vec<&Entity> = entities.iter().filter(|e| seen_ids.insert(e.id)).collect();
        let entities: Vec<Entity> = entities.into_iter().cloned().collect();
        let entities = &entities;
        self.with(|conn| {
            let tx = conn.transaction()?;
            // FTS first: the entity_id subquery must see the rows being
            // replaced — deleting `entities` before this would empty it.
            tx.execute(
                "DELETE FROM entities_fts WHERE kind = ?1 AND entity_id IN
                    (SELECT id FROM entities WHERE kind = ?1 AND source = ?2)",
                [kind_str, source],
            )?;
            tx.execute(
                "DELETE FROM entities WHERE kind = ?1 AND source = ?2",
                [kind_str, source],
            )?;
            {
                let mut stmt = tx.prepare(
                    "INSERT INTO entities (kind, id, name, code, subtitle, source, fetched_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )?;
                for e in entities {
                    stmt.execute(rusqlite::params![
                        kind_str,
                        e.id as i64,
                        e.name,
                        e.code,
                        e.subtitle,
                        source,
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
                (meta_key_sourced(kind, source), now_unix().to_string()),
            )?;
            tx.commit()
        })
    }

    /// FTS5 prefix search across the given kinds (`ORDER BY rank` = best match
    /// first). `query` is escaped; a literal `*`-suffix enables prefix matching.
    /// Empty query → browse mode: the first `limit` rows of the requested kinds
    /// (Arkanis: a bare `:c`/`:i`/`:l` prefix lists that category).
    pub fn search(&self, kinds: &[EntityKind], query: &str, limit: u32) -> Result<Vec<SearchHit>> {
        self.search_sourced(kinds, &[], query, &[], limit)
    }

    /// [`Cache::search`] with a source filter: `sources` empty = any source
    /// (historical behavior); otherwise only rows whose `source` is listed.
    /// Used by the composite provider to keep the commodity search on the UEX
    /// id space (wiki commodity rows are internal resolution data) while the
    /// location group serves BOTH datasets.
    ///
    /// `name_only_kinds`: for these kinds ONLY `name`/`code` participate in
    /// the FTS match — `subtitle` is ignored. Feedback-Runde 4 (P1): a wiki
    /// location matches its NAME, not its chain ("Mercy Hospital" must NOT
    /// surface for `levski` just because its subtitle reads
    /// "Nyx > Levski > Mercy Hospital"). Terminals keep the full-text match
    /// (name + code + subtitle).
    pub fn search_sourced(
        &self,
        kinds: &[EntityKind],
        sources: &[&str],
        query: &str,
        name_only_kinds: &[EntityKind],
        limit: u32,
    ) -> Result<Vec<SearchHit>> {
        let query = query.trim();
        // rusqlite numbers parameters by token VALUE ("?1"?1" is ONE slot), so
        // each kind gets its own index.
        let placeholders = (1..=kinds.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let kind_params: Vec<String> = kinds.iter().map(|k| k.as_str().to_string()).collect();
        // Optional source filter: `AND source IN (?, …)` after the kind list.
        // The FTS table carries no source column, so the filter resolves
        // through the entities table by id.
        let (source_clause, source_params): (String, Vec<String>) = if sources.is_empty() {
            (String::new(), Vec::new())
        } else {
            let n = kinds.len();
            let ph = (0..sources.len())
                .map(|i| format!("?{}", n + 1 + i))
                .collect::<Vec<_>>()
                .join(",");
            (
                format!(" AND source IN ({ph})"),
                sources.iter().map(|s| (*s).to_string()).collect(),
            )
        };
        let source_filter = if sources.is_empty() {
            String::new()
        } else {
            format!(
                " AND entity_id IN (SELECT id FROM entities WHERE kind IN ({placeholders}){source_clause})"
            )
        };
        let limit_idx = kinds.len() + source_params.len() + 1;

        let map_row = |row: &rusqlite::Row| {
            Ok(SearchHit {
                id: row.get::<_, i64>(1)? as u64,
                name: row.get(2)?,
                kind: row.get(0)?,
                code: row.get(3)?,
                subtitle: row.get(4)?,
            })
        };

        if query.is_empty() {
            // No FTS match expression to rank by — list the kind in id order.
            let sql = format!(
                "SELECT kind, entity_id, name, code, subtitle
                 FROM entities_fts
                 WHERE kind IN ({placeholders}){source_filter}
                 ORDER BY entity_id LIMIT ?{limit_idx}"
            );
            return self.with(|conn| {
                let mut stmt = conn.prepare(&sql)?;
                let mut params: Vec<&dyn rusqlite::ToSql> = Vec::new();
                for p in &kind_params {
                    params.push(p);
                }
                for p in &source_params {
                    params.push(p);
                }
                params.push(&limit);
                let rows = stmt.query_map(rusqlite::params_from_iter(params), map_row)?;
                let mut out = Vec::new();
                for r in rows {
                    out.push(r?);
                }
                Ok(out)
            });
        }
        let escaped = query.replace('"', "\"\"");
        let base = format!("\"{escaped}\"*");
        // Name-only kinds match against name/code ONLY (P1): the FTS
        // column-filter syntax (`name:"q"*`) restricts the match to the name
        // and code columns. `kind` itself is UNINDEXED in the FTS table, so
        // the kind split happens in the WHERE clause — the search runs as
        // TWO queries (name-only kinds first, then the full-text kinds) and
        // the results are concatenated in that order.
        let name_only_cols = format!("name:\"{escaped}\"* OR code:\"{escaped}\"*");
        let run_query = |kinds: &[EntityKind], match_expr: &str| -> Result<Vec<SearchHit>> {
            if kinds.is_empty() {
                return Ok(Vec::new());
            }
            let placeholders = (1..=kinds.len())
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(",");
            let kind_params: Vec<String> = kinds.iter().map(|k| k.as_str().to_string()).collect();
            let (source_clause, source_params): (String, Vec<String>) = if sources.is_empty() {
                (String::new(), Vec::new())
            } else {
                let n = kinds.len();
                let ph = (0..sources.len())
                    .map(|i| format!("?{}", n + 1 + i))
                    .collect::<Vec<_>>()
                    .join(",");
                (
                    format!(" AND source IN ({ph})"),
                    sources.iter().map(|s| (*s).to_string()).collect(),
                )
            };
            let source_filter = if sources.is_empty() {
                String::new()
            } else {
                format!(
                    " AND entity_id IN (SELECT id FROM entities WHERE kind IN ({placeholders}){source_clause})"
                )
            };
            let limit_idx = kinds.len() + source_params.len() + 1;
            let sql = format!(
                "SELECT kind, entity_id, name, code, subtitle
                 FROM entities_fts
                 WHERE kind IN ({placeholders}){source_filter} AND entities_fts MATCH ?{limit_idx}
                 ORDER BY rank LIMIT ?{}",
                limit_idx + 1
            );
            self.with(|conn| {
                let mut stmt = conn.prepare(&sql)?;
                let mut params: Vec<&dyn rusqlite::ToSql> = Vec::new();
                for p in &kind_params {
                    params.push(p);
                }
                for p in &source_params {
                    params.push(p);
                }
                params.push(&match_expr);
                params.push(&limit);
                let rows = stmt.query_map(rusqlite::params_from_iter(params), map_row)?;
                let mut out = Vec::new();
                for r in rows {
                    out.push(r?);
                }
                Ok(out)
            })
        };
        let name_only: Vec<EntityKind> = kinds
            .iter()
            .filter(|k| name_only_kinds.contains(k))
            .copied()
            .collect();
        let full_text: Vec<EntityKind> = kinds
            .iter()
            .filter(|k| !name_only_kinds.contains(k))
            .copied()
            .collect();
        let mut out = run_query(&name_only, &name_only_cols)?;
        out.extend(run_query(&full_text, &base)?);
        Ok(out)
    }

    // ---------- bookkeeping ----------

    /// Seconds since the reference dataset of `kind` was fetched
    /// (`Ok(None)` = never fetched). Feeds the UI's freshness indicator.
    pub fn dataset_age(&self, kind: EntityKind) -> Result<Option<u64>> {
        self.dataset_age_sourced(kind, SOURCE_UEX)
    }

    /// Per-source variant of [`Cache::dataset_age`].
    pub fn dataset_age_sourced(&self, kind: EntityKind, source: &str) -> Result<Option<u64>> {
        Ok(self
            .meta_get(&meta_key_sourced(kind, source))?
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

    /// Number of cached entities of one (kind, source) pair.
    pub fn count_entities_sourced(&self, kind: EntityKind, source: &str) -> Result<u64> {
        self.with(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM entities WHERE kind = ?1 AND source = ?2",
                [kind.as_str(), source],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n as u64)
        })
    }

    /// All cached reference entities of one kind, id → row. Used for small
    /// lookups (e.g. star system id → region name) rather than FTS search.
    pub fn entities_of(&self, kind: EntityKind) -> Result<Vec<Entity>> {
        self.entities_of_sourced(kind, None)
    }

    /// All entities of one kind, optionally restricted to one source
    /// (`Some("wiki")` → only wiki rows). `None` keeps the historical
    /// any-source behavior.
    pub fn entities_of_sourced(
        &self,
        kind: EntityKind,
        source: Option<&str>,
    ) -> Result<Vec<Entity>> {
        self.with(|conn| {
            let sql = match source {
                Some(_) => {
                    "SELECT id, name, code, subtitle FROM entities
                     WHERE kind = ?1 AND source = ?2"
                }
                None => "SELECT id, name, code, subtitle FROM entities WHERE kind = ?1",
            };
            let mut stmt = conn.prepare(sql)?;
            let map = |row: &rusqlite::Row| {
                Ok(Entity {
                    id: row.get::<_, i64>(0)? as u64,
                    kind,
                    name: row.get(1)?,
                    code: row.get(2)?,
                    subtitle: row.get(3)?,
                })
            };
            let rows = match source {
                Some(src) => stmt.query_map([kind.as_str(), src], map)?,
                None => stmt.query_map([kind.as_str()], map)?,
            };
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
        self.prices_fresh_sourced(commodity_id, SOURCE_UEX)
    }

    /// Per-source freshness marker for one commodity's price table.
    pub fn prices_fresh_sourced(&self, commodity_id: u64, source: &str) -> Result<bool> {
        let fetched = self.meta_get(&prices_meta_key_sourced(commodity_id, source))?;
        Ok(match fetched {
            Some(ts) => now_unix().saturating_sub(ts) < TTL_PRICES.as_secs(),
            None => false,
        })
    }

    /// Upsert price points (key = commodity × terminal) and mark them fetched.
    pub fn store_prices(&self, points: &[PricePoint]) -> Result<()> {
        self.store_prices_sourced(points, SOURCE_UEX)
    }

    /// Upsert price points attributed to a named source and mark them fetched
    /// under that source's freshness key. Wiki price rows (hash commodity ids)
    /// and UEX rows never overwrite each other.
    pub fn store_prices_sourced(&self, points: &[PricePoint], source: &str) -> Result<()> {
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
                                          buy_status, sell_status, quality, source, updated, fetched_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
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
                       source = excluded.source,
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
                        source,
                        p.updated as i64,
                        now
                    ])?;
                }
            }
            tx.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                (prices_meta_key_sourced(commodity_id, source), now_unix().to_string()),
            )?;
            tx.commit()
        })
    }

    /// Cached price points for one commodity (may be stale — caller checks
    /// [`Cache::prices_fresh`] first).
    pub fn prices_for(&self, commodity_id: u64) -> Result<Vec<PricePoint>> {
        self.prices_for_sourced(commodity_id, None)
    }

    /// Cached price points for one commodity, restricted to one source
    /// (`None` = any source, historical behavior).
    pub fn prices_for_sourced(
        &self,
        commodity_id: u64,
        source: Option<&str>,
    ) -> Result<Vec<PricePoint>> {
        self.with(|conn| {
            let (sql, params): (&str, Vec<Box<dyn rusqlite::ToSql>>) = match source {
                Some(_) => (
                    "SELECT commodity, id_terminal, terminal, buy_price, sell_price,
                            buy_stock, sell_stock, buy_status, sell_status, quality, updated
                     FROM prices WHERE id_commodity = ?1 AND source = ?2 ORDER BY terminal",
                    vec![
                        Box::new(commodity_id as i64),
                        Box::new(source.unwrap_or_default().to_string()),
                    ],
                ),
                None => (
                    "SELECT commodity, id_terminal, terminal, buy_price, sell_price,
                            buy_stock, sell_stock, buy_status, sell_status, quality, updated
                     FROM prices WHERE id_commodity = ?1 ORDER BY terminal",
                    vec![Box::new(commodity_id as i64)],
                ),
            };
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
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

    // ---------- Handelsrouten (Karte t_128b7547) ----------

    /// JSON-Metadatensatz der Routen (Terminal-/Ware-Flags) lesen.
    pub fn route_meta_get(&self, key: &str) -> Result<Option<(u64, String)>> {
        self.with(|conn| {
            let mut stmt =
                conn.prepare("SELECT fetched_at, payload FROM route_meta WHERE key = ?1")?;
            let mut rows = stmt.query([key])?;
            let row = rows.next()?;
            match row {
                Some(r) => Ok(Some((r.get::<_, i64>(0)? as u64, r.get::<_, String>(1)?))),
                None => Ok(None),
            }
        })
    }

    /// JSON-Metadatensatz der Routen schreiben (Upsert).
    pub fn route_meta_set(&self, key: &str, fetched_at: u64, payload: &str) -> Result<()> {
        self.with(|conn| {
            conn.execute(
                "INSERT INTO route_meta (key, fetched_at, payload) VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET fetched_at = excluded.fetched_at,
                                                payload = excluded.payload",
                rusqlite::params![key, fetched_at as i64, payload],
            )?;
            Ok(())
        })
    }

    /// ALLE Terminal-Preiszeilen (id_terminal + Punkt) — die Basis des
    /// Routen-Sweeps: der Sweep füllt `terminal_prices` über alle
    /// Commodity-Terminals, der Routen-Compute liest die Gesamtheit.
    pub fn terminal_price_rows_all(&self) -> Result<Vec<(u64, Vec<PricePoint>)>> {
        self.with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id_terminal, commodity, id_commodity, buy_price, sell_price,
                        buy_stock, sell_stock, buy_status, sell_status, quality, updated
                 FROM terminal_prices ORDER BY id_terminal, commodity",
            )?;
            let mut out: Vec<(u64, Vec<PricePoint>)> = Vec::new();
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)? as u64,
                    PricePoint {
                        location: String::new(),
                        commodity: row.get(1)?,
                        commodity_id: row.get::<_, i64>(2)? as u64,
                        terminal_id: row.get::<_, i64>(0)? as u64,
                        buy_price: row.get(3)?,
                        sell_price: row.get(4)?,
                        buy_stock: row.get(5)?,
                        sell_stock: row.get(6)?,
                        buy_status: row.get(7)?,
                        sell_status: row.get(8)?,
                        quality: row.get(9)?,
                        updated: row.get::<_, i64>(10)? as u64,
                    },
                ))
            })?;
            for row in rows {
                let (terminal_id, point) = row?;
                match out.last_mut() {
                    Some((id, points)) if *id == terminal_id => points.push(point),
                    _ => out.push((terminal_id, vec![point])),
                }
            }
            Ok(out)
        })
    }

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

    #[test]
    fn v2_migration_adds_source_columns_and_keeps_legacy_rows() {
        // Simulate a v1 cache (SCHEMA without the source columns): opening it
        // must ALTER the columns in, stamp user_version = 2 and attribute all
        // legacy rows to `uex` — data stays valid, nothing is invalidated.
        let legacy_schema = SCHEMA
            .replace("  source     TEXT    NOT NULL DEFAULT 'uex',\n", "")
            .replace("  source       TEXT    NOT NULL DEFAULT 'uex',\n", "");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v1.sqlite3");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(&legacy_schema).unwrap();
            conn.execute_batch(
                "INSERT INTO entities (kind, id, name, code, subtitle, fetched_at)
                 VALUES ('commodity', 1, 'Agricium', 'AGRI', 'Metal', 100);",
            )
            .unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
        }
        let cache = Cache::open(&path).unwrap();
        // user_version stamped, legacy row kept and attributed to uex.
        let conn_ok = cache
            .with(|conn| {
                let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
                let src: String =
                    conn.query_row("SELECT source FROM entities WHERE id = 1", [], |r| r.get(0))?;
                Ok((v, src))
            })
            .unwrap();
        assert_eq!(conn_ok, (SCHEMA_VERSION, "uex".to_string()));
        // Wiki datasets can be stored beside the legacy rows without overlap.
        cache
            .store_entities_sourced(
                EntityKind::Locations,
                SOURCE_WIKI,
                &[entity(
                    dto_like_hash(),
                    EntityKind::Locations,
                    "Levski",
                    "levski",
                    "Nyx > Delamar",
                )],
            )
            .unwrap();
        assert_eq!(
            cache
                .count_entities_sourced(EntityKind::Locations, SOURCE_WIKI)
                .unwrap(),
            1
        );
        let _ = path;
        drop(dir);
    }

    fn dto_like_hash() -> u64 {
        0xcbf29ce484222325_u64.wrapping_mul(0x100000001b3) ^ 42
    }

    #[test]
    fn sourced_datasets_are_isolated_per_source() {
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
        cache
            .store_entities_sourced(
                EntityKind::Commodities,
                SOURCE_WIKI,
                &[entity(
                    99,
                    EntityKind::Commodities,
                    "Agricium",
                    "agricium",
                    "Raw",
                )],
            )
            .unwrap();
        assert_eq!(cache.count_entities(EntityKind::Commodities).unwrap(), 2);
        assert_eq!(
            cache
                .count_entities_sourced(EntityKind::Commodities, SOURCE_WIKI)
                .unwrap(),
            1
        );
        // Replacing one source leaves the other intact.
        cache
            .store_entities_sourced(EntityKind::Commodities, SOURCE_WIKI, &[])
            .unwrap();
        assert_eq!(cache.count_entities(EntityKind::Commodities).unwrap(), 1);
    }

    #[test]
    fn wiki_ttl_is_patch_driven_and_long() {
        // Freshness per source: the wiki marker ages against the long patch
        // TTL, the UEX marker against the research-doc TTL.
        let cache = Cache::open_in_memory().unwrap();
        cache
            .store_entities_sourced(
                EntityKind::Locations,
                SOURCE_WIKI,
                &[entity(1, EntityKind::Locations, "Levski", "levski", "Nyx")],
            )
            .unwrap();
        assert!(
            cache
                .is_fresh_sourced(EntityKind::Locations, SOURCE_WIKI)
                .unwrap()
        );
        let long = now_unix() - ttl_for_source(EntityKind::Locations, SOURCE_WIKI).as_secs() - 1;
        cache
            .with(|conn| {
                conn.execute(
                    "UPDATE meta SET value = ?1 WHERE key = ?2",
                    [
                        long.to_string(),
                        meta_key_sourced(EntityKind::Locations, SOURCE_WIKI),
                    ],
                )?;
                Ok(())
            })
            .unwrap();
        assert!(
            !cache
                .is_fresh_sourced(EntityKind::Locations, SOURCE_WIKI)
                .unwrap()
        );
    }

    #[test]
    fn wiki_price_rows_carry_their_own_freshness() {
        let cache = Cache::open_in_memory().unwrap();
        let p = PricePoint {
            commodity: "Agricium".into(),
            commodity_id: 9_000_000_000,
            location: "Levski".into(),
            terminal_id: 778,
            sell_price: 0.0,
            buy_price: 8_379.0,
            sell_stock: 0.0,
            buy_stock: 0.0,
            sell_status: 0,
            buy_status: 7,
            quality: 0,
            updated: 1_700_000_000,
        };
        cache
            .store_prices_sourced(&[p.clone()], SOURCE_WIKI)
            .unwrap();
        assert!(
            cache
                .prices_fresh_sourced(9_000_000_000, SOURCE_WIKI)
                .unwrap()
        );
        assert!(
            !cache.prices_fresh(9_000_000_000).unwrap(),
            "UEX marker must stay independent"
        );
        let rows = cache
            .prices_for_sourced(9_000_000_000, Some(SOURCE_WIKI))
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].buy_price, 8_379.0);
        // UEX store under the same (commodity, terminal) pair must not clobber
        // the wiki row — the sourced writes are isolated via distinct PK rows.
        cache
            .store_prices_sourced(
                &[PricePoint {
                    updated: 1_700_000_001,
                    ..p.clone()
                }],
                SOURCE_WIKI,
            )
            .unwrap();
        assert_eq!(
            cache
                .prices_for_sourced(9_000_000_000, Some(SOURCE_WIKI))
                .unwrap()
                .len(),
            1
        );
        let _ = p;
    }

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

    /// P1 (Feedback-Runde 4): in der Location-Gruppe zählt nur der NAME der
    /// Location-Entity — die Kette (subtitle) matcht nicht mehr
    /// ("Nyx > Levski > Mercy Hospital" taucht bei `levski` NICHT auf).
    /// Terminals im selben Query behalten den vollen Match.
    #[test]
    fn location_search_matches_name_only() {
        let cache = Cache::open_in_memory().unwrap();
        cache
            .store_entities(
                EntityKind::Locations,
                &[
                    entity(
                        900,
                        EntityKind::Locations,
                        "Mercy Hospital",
                        "MERCY",
                        "Nyx > Levski > Mercy Hospital",
                    ),
                    entity(901, EntityKind::Locations, "Levski", "LEVS", "Nyx > Levski"),
                ],
            )
            .unwrap();
        cache
            .store_entities(
                EntityKind::Terminals,
                &[
                    entity(
                        777,
                        EntityKind::Terminals,
                        "Pharmacy - Levski",
                        "",
                        "Stanton > Nyx > Levski",
                    ),
                    entity(
                        778,
                        EntityKind::Terminals,
                        "Commodity Shop - Levski",
                        "",
                        "Stanton > Nyx > Levski",
                    ),
                ],
            )
            .unwrap();

        // Name-only: Mercy (subtitle match) ist weg, Levski (Name) da,
        // Terminals matchen weiter über den vollen Text.
        let hits = cache
            .search_sourced(
                &[EntityKind::Locations, EntityKind::Terminals],
                &[],
                "levski",
                &[EntityKind::Locations],
                25,
            )
            .unwrap();
        let names: Vec<&str> = hits.iter().map(|h| h.name.as_str()).collect();
        assert!(
            names.contains(&"Levski"),
            "location name must match: {names:?}"
        );
        assert!(
            names.contains(&"Pharmacy - Levski"),
            "terminal full-text match must be kept: {names:?}"
        );
        assert!(
            !names.contains(&"Mercy Hospital"),
            "location subtitle must NOT match: {names:?}"
        );

        // Ohne name_only bleibt das alte Verhalten (Regression-Anker).
        let hits = cache
            .search_sourced(&[EntityKind::Locations], &[], "levski", &[], 25)
            .unwrap();
        assert!(hits.iter().any(|h| h.name == "Mercy Hospital"));

        // Browse-Modus (leere Query) ist von name_only unberührt.
        let hits = cache
            .search_sourced(
                &[EntityKind::Locations],
                &[],
                "",
                &[EntityKind::Locations],
                25,
            )
            .unwrap();
        assert_eq!(hits.len(), 2);
    }

    /// P2-Begleiter: die v3-Migration räumt die Alt-Bestände (Terminals VOR
    /// dem Typ-Filter) weg und setzt die Freshness-Marker zurück — nur beim
    /// Versionssprung (user_version < 3), danach nie wieder.
    #[test]
    fn service_terminal_purge_runs_once_and_resets_freshness() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.db");
        {
            let cache = Cache::open(&path).unwrap();
            cache
                .store_entities(
                    EntityKind::Terminals,
                    &[entity(
                        393,
                        EntityKind::Terminals,
                        "Landing Services - Levski",
                        "",
                        "Stanton > Nyx > Levski",
                    )],
                )
                .unwrap();
            assert!(cache.is_fresh(EntityKind::Terminals).unwrap());
            // Simuliere einen v2-Cache (vor dem Feature): der nächste Open
            // muss die Terminal-Daten wegwerfen.
            cache
                .with(|conn| conn.pragma_update(None, "user_version", 2))
                .unwrap();
        }
        {
            let cache = Cache::open(&path).unwrap();
            assert!(
                !cache.is_fresh(EntityKind::Terminals).unwrap(),
                "the v3 migration must drop the stale terminal dataset"
            );
            assert_eq!(
                cache.count_entities(EntityKind::Terminals).unwrap(),
                0,
                "purged rows must be gone"
            );
            // Ein frischer Import überlebt das nächste Öffnen (Purge läuft
            // nicht erneut — Versionssprung ist vorbei).
            cache
                .store_entities(
                    EntityKind::Terminals,
                    &[entity(
                        777,
                        EntityKind::Terminals,
                        "Pharmacy - Levski",
                        "",
                        "Stanton > Nyx > Levski",
                    )],
                )
                .unwrap();
        }
        let cache = Cache::open(&path).unwrap();
        assert!(cache.is_fresh(EntityKind::Terminals).unwrap());
        assert_eq!(cache.count_entities(EntityKind::Terminals).unwrap(), 1);
    }
}
