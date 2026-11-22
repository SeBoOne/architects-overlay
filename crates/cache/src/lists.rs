//! # cache::lists
//!
//! Generische Listen-Engine (M9-B, t_36778bc3, Welle 1 B): benannte Listen
//! von Blueprints (Crafting) bzw. Materialien (Mining, M9-C). Modi-unab-
//! hängig gebaut — die Tabelle `lists` trägt eine `kind`-Spalte
//! (`craft` | `mining`), die Modi sind Views darauf.
//!
//! Ein Listen-Eintrag (craft) speichert einen BLUEPRINT-SNAPSHOT: die
//! Materialliste inkl. der beim Hinzufügen eingestellten Qualitätsregler-
//! Werte (Default 500, „Qualität wird im Calculator beim Hinzufügen
//! übernommen"). Die Material-Aggregation (Mengen summiert über alle
//! Einträge, Qualität = MAX — „höhere Qualität ist immer besser") rechnet
//! diese Engine serverseitig im Cache, damit es EINE Quelle der Wahrheit
//! gibt (die UI zeigt nur an).
//!
//! Abhaken: pro aggregiertem Material (Liste + Material + Einheit) eine
//! `done`-Menge — ganz (done = Sollmenge) oder teilweise (freier Wert).
//! Lokal-first wie das Inventar: KEIN Sync, KEIN API-Zugriff, nur die App
//! schreibt hier.

use super::inventory::{InventoryEntry, NewInventoryEntry};
use super::reservations::{
    clamp_list_material_reservations, clamp_list_reservations, list_material_requirement,
    list_material_reserved,
};
use super::{Cache, CacheError, Result};
use rusqlite::{Connection, OptionalExtension};

/// Craft-Zeit-Snapshot eines Eintrags (Sekunden, 0 = unbekannt).
pub const KIND_CRAFT: &str = "craft";
pub const KIND_MINING: &str = "mining";

pub fn kind_valid(kind: &str) -> bool {
    kind == KIND_CRAFT || kind == KIND_MINING
}

/// Eine Material-Zeile eines Blueprint-Snapshots.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ListItemMat {
    pub material: String,
    /// Strata oreId (Mining-Sprung) — null-fähig.
    pub ore_id: Option<String>,
    /// `SCU | gems | items` — Einheiten-Regel: nie mischen.
    pub unit: String,
    pub quantity: f64,
    /// Übernommener Regler-Wert 0–1000 (Default 500).
    pub quality: u32,
}

/// Payload einer Material-Zeile beim Hinzufügen.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct NewListItemMat {
    pub material: String,
    pub ore_id: Option<String>,
    pub unit: String,
    pub quantity: f64,
    pub quality: u32,
}

/// Payload: ein Blueprint-Eintrag mit seinem Material-Snapshot.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct NewListItem {
    pub item_id: String,
    pub item_name: String,
    /// Craft-Zeit in Sekunden (Anzeige-Snapshot).
    pub craft_time: u64,
    /// Wie oft gecraftet wird (Material × copies in der Aggregation).
    pub copies: f64,
    pub mats: Vec<NewListItemMat>,
}

/// Persistenter Listen-Eintrag (Blueprint-Snapshot inkl. Materialien).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ListItem {
    pub id: u64,
    pub item_id: String,
    pub item_name: String,
    pub craft_time: u64,
    pub copies: f64,
    pub created_at: u64,
    pub mats: Vec<ListItemMat>,
}

/// Listen-Kopf für die Auswahl.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ListSummary {
    pub id: u64,
    pub kind: String,
    pub name: String,
    pub created_at: u64,
    pub item_count: u64,
    /// M9-D (t_36f3966c): erzeugende Liste (Brücke), NULL = standalone.
    pub source_list_id: Option<u64>,
    /// Name der erzeugenden Liste (aufgelöst beim Lesen).
    pub source_list_name: Option<String>,
}

/// Aggregierte Material-Zeile einer Liste (die Abhak-Ebene).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct AggMat {
    pub material: String,
    pub unit: String,
    /// Summe aller Eintrags-Bedarfe (quantity × copies).
    pub quantity: f64,
    /// MAX der Eintrags-Qualitäten — „höhere Qualität ist immer besser“.
    pub min_quality: u32,
    /// Bereits abgehakt (ganz ODER teilweise).
    pub done_qty: f64,
    /// M9-F (t_bb97da62): aus dem Inventar für dieses Material reservierte
    /// Menge (Reservierungen mit `list_id` = diese Liste, gematcht über den
    /// Inventar-Namen) — 0, wenn nichts reserviert.
    pub reserved_qty: f64,
    /// Mengengewichtete Ø-Qualität der reservierten Quelle — `None` ohne
    /// Reservierung.
    pub reserved_avg_quality: Option<u32>,
}

/// Vollständige Liste: Kopf + Einträge + Aggregation.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ListDetail {
    pub id: u64,
    pub kind: String,
    pub name: String,
    pub created_at: u64,
    pub items: Vec<ListItem>,
    pub aggregated: Vec<AggMat>,
}

/// Ergebnis von `inventory_add_for_list` (M9-E): die neue Inventar-Position
/// (inkl. Reservierungs-Verweis) + ob die Qualitäts-Anforderung der Liste
/// angehoben wurde („höhere Qualität ist immer besser").
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ReservedAdd {
    pub entry: InventoryEntry,
    pub requirement_raised: bool,
    /// Anforderung NACH der Anpassung (MAX aus Alt-Werten + gelieferter Q).
    pub requirement: u32,
}

fn validate_kind(kind: &str) -> Result<()> {
    if !kind_valid(kind) {
        return Err(CacheError::Query(i18n_err::err_key("lists.err_kind")));
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(CacheError::Query(i18n_err::err_key("lists.err_name")));
    }
    Ok(())
}

/// M9-G (t_131614e0) — Aggregation einer Liste für den Spiegel: (material,
/// unit) → (Menge, MAX-Qualität, ore_id). ore_id wie in `bridgeItems` (M9-D):
/// letzter Treffer in item/mats-Reihenfolge gewinnt (ORDER BY ... DESC, LIMIT 1).
type SyncAgg = Vec<(String, String, f64, i64, Option<String>)>;

fn craft_agg(conn: &Connection, list_id: i64) -> rusqlite::Result<SyncAgg> {
    let mut stmt = conn.prepare(
        "SELECT mim.material, mim.unit, SUM(mim.quantity * li.copies), MAX(mim.quality), \
                (SELECT m2.ore_id FROM list_item_mats m2 \
                 JOIN list_items li2 ON li2.id = m2.item_row \
                 WHERE li2.list_id = ?1 AND m2.material = mim.material AND m2.unit = mim.unit \
                 ORDER BY li2.id DESC, m2.id DESC LIMIT 1) \
         FROM list_items li JOIN list_item_mats mim ON mim.item_row = li.id \
         WHERE li.list_id = ?1 \
         GROUP BY mim.material, mim.unit \
         ORDER BY LOWER(mim.material) ASC",
    )?;
    let rows = stmt.query_map([list_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, f64>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, Option<String>>(4)?,
        ))
    })?;
    rows.collect()
}

/// Spiegelt den Stand der Crafting-Liste `source_id` in ALLE verknüpften
/// Mining-Listen (lists.source_list_id): Einträge werden neu aufgebaut
/// (Material/Menge/Qualität = Aggregation, M9-D-Brücke), der Abhak-Stand
/// bleibt für weitergeführte Materialien erhalten (auf die neue Menge
/// geklemmt) und fällt für entfernte Materialien weg. Wird nach jeder
/// Struktur-Änderung der Quell-Liste aufgerufen — die Verknüpfung ist
/// damit dauerhaft live, nicht nur beim Anlegen.
fn sync_linked_mining(conn: &mut Connection, source_id: u64) -> rusqlite::Result<()> {
    let agg = craft_agg(conn, source_id as i64)?;
    let linked: Vec<i64> = {
        let mut stmt = conn.prepare(
            "SELECT id FROM lists WHERE source_list_id = ?1 AND kind = 'mining' ORDER BY id",
        )?;
        let rows = stmt.query_map([source_id as i64], |r| r.get::<_, i64>(0))?;
        rows.collect::<rusqlite::Result<Vec<i64>>>()?
    };
    for m in linked {
        // Abhak-Stand VOR dem Neuaufbau lesen (er bleibt für Materialien,
        // die weitergeführt werden, erhalten — auf die neue Menge geklemmt).
        let done: Vec<(String, String, f64)> = {
            let mut stmt = conn
                .prepare("SELECT material, unit, done_qty FROM list_mat_done WHERE list_id = ?1")?;
            let rows = stmt.query_map([m], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, f64>(2)?,
                ))
            })?;
            rows.collect::<rusqlite::Result<Vec<(String, String, f64)>>>()?
        };
        conn.execute(
            "DELETE FROM list_item_mats WHERE item_row IN \
             (SELECT id FROM list_items WHERE list_id = ?1)",
            [m],
        )?;
        conn.execute("DELETE FROM list_items WHERE list_id = ?1", [m])?;
        conn.execute("DELETE FROM list_mat_done WHERE list_id = ?1", [m])?;
        for (material, unit, qty, quality, ore_id) in &agg {
            if *qty <= 0.0 {
                continue;
            }
            conn.execute(
                "INSERT INTO list_items (list_id, item_id, item_name, craft_time, copies, created_at) \
                 VALUES (?1, ?2, ?3, 0, ?4, ?5)",
                rusqlite::params![
                    m,
                    ore_id.clone().unwrap_or_else(|| material.clone()),
                    material,
                    qty,
                    super::now_unix() as i64
                ],
            )?;
            let row = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO list_item_mats (item_row, material, ore_id, unit, quantity, quality) \
                 VALUES (?1, ?2, ?3, ?4, 1, ?5)",
                rusqlite::params![row, material, ore_id, unit, quality],
            )?;
        }
        // done-Stand neu schreiben: nur Materialien, die es noch gibt,
        // done auf die (ggf. geschrumpfte) Sollmenge geklemmt.
        // M9-G R1-Fix (Review-Befund 1): der GEKLAMMTE Wert wird auch in die
        // Quell-Liste zurückgeschrieben — sonst divergieren verknüpfte
        // Ansichten (Mining done=25 geklemmt, Craft done=100 alt) für denselben
        // Zustand. Invariante: done ≤ Soll gilt in BEIDEN Listen.
        for (material, unit, dq) in done {
            if let Some((_, _, qty, _, _)) = agg
                .iter()
                .find(|(am, au, aq, _, _)| *am == material && *au == unit && *aq > 0.0)
            {
                let clamped = dq.min(*qty);
                conn.execute(
                    "INSERT INTO list_mat_done (list_id, material, unit, done_qty) \
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![m, material, unit, clamped],
                )?;
                if clamped < dq {
                    conn.execute(
                        "INSERT INTO list_mat_done (list_id, material, unit, done_qty) \
                         VALUES (?1, ?2, ?3, ?4) \
                         ON CONFLICT(list_id, material, unit) DO UPDATE SET done_qty = ?4",
                        rusqlite::params![source_id as i64, material, unit, clamped],
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn validate_item(new: &NewListItem) -> Result<()> {
    if new.item_id.trim().is_empty() || new.item_name.trim().is_empty() {
        return Err(CacheError::Query(i18n_err::err_key("lists.err_item")));
    }
    if !(new.copies.is_finite() && new.copies > 0.0) {
        return Err(CacheError::Query(i18n_err::err_key("lists.err_copies")));
    }
    if new.mats.is_empty() {
        return Err(CacheError::Query(i18n_err::err_key("lists.err_mats")));
    }
    for m in &new.mats {
        if m.material.trim().is_empty() || m.unit.trim().is_empty() {
            return Err(CacheError::Query(i18n_err::err_key("lists.err_item")));
        }
        if !(m.quantity.is_finite() && m.quantity >= 0.0) {
            return Err(CacheError::Query(i18n_err::err_key("lists.err_item")));
        }
        if m.quality > 1000 {
            return Err(CacheError::Query(i18n_err::err_key("lists.err_quality")));
        }
    }
    Ok(())
}

impl Cache {
    /// Alle Listen eines Modus (Kopfzeilen, neueste zuerst).
    pub fn lists_list(&self, kind: &str) -> Result<Vec<ListSummary>> {
        validate_kind(kind)?;
        self.with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT l.id, l.kind, l.name, l.created_at, \
                        (SELECT COUNT(*) FROM list_items i WHERE i.list_id = l.id) AS items, \
                        l.source_list_id, s.name AS source_list_name \
                 FROM lists l LEFT JOIN lists s ON s.id = l.source_list_id \
                 WHERE l.kind = ?1 ORDER BY l.created_at DESC, l.id DESC",
            )?;
            let rows = stmt.query_map([kind], |r| {
                Ok(ListSummary {
                    id: r.get::<_, i64>("id")? as u64,
                    kind: r.get("kind")?,
                    name: r.get("name")?,
                    created_at: r.get::<_, i64>("created_at")? as u64,
                    item_count: r.get::<_, i64>("items")? as u64,
                    source_list_id: r.get::<_, Option<i64>>("source_list_id")?.map(|v| v as u64),
                    source_list_name: r.get("source_list_name")?,
                })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
    }

    /// Neue Liste, liefert ihre id. `source_list_id` (M9-D, t_36f3966c)
    /// verknüpft eine Brücken-erzeugte Liste persistent mit ihrer erzeugenden
    /// Liste (Mining-Liste → Crafting-Liste) — die Inventar-Reservierungs-
    /// Hinweise und Badges zeigen dann die ERZEUGENDE Liste. Fehlende
    /// Source-Liste fliegt; beim Löschen der Source löst sich der Link.
    pub fn list_create(&self, kind: &str, name: &str, source_list_id: Option<u64>) -> Result<u64> {
        validate_kind(kind)?;
        validate_name(name)?;
        if let Some(src) = source_list_id {
            let exists: i64 = self.with(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM lists WHERE id = ?1",
                    [src as i64],
                    |r| r.get(0),
                )
            })?;
            if exists == 0 {
                return Err(CacheError::Query(i18n_err::err_keyp(
                    "lists.err_missing",
                    serde_json::json!({ "id": src }),
                )));
            }
        }
        self.with(move |conn| {
            conn.execute(
                "INSERT INTO lists (kind, name, created_at, source_list_id) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    kind,
                    name.trim(),
                    super::now_unix() as i64,
                    source_list_id.map(|v| v as i64)
                ],
            )?;
            Ok(conn.last_insert_rowid() as u64)
        })
    }

    /// Liste umbenennen. `Err` wenn die id nicht existiert.
    pub fn list_rename(&self, id: u64, name: &str) -> Result<()> {
        validate_name(name)?;
        let changed = self.with(|conn| {
            conn.execute(
                "UPDATE lists SET name = ?1 WHERE id = ?2",
                rusqlite::params![name.trim(), id as i64],
            )
        })?;
        if changed == 0 {
            return Err(CacheError::Query(i18n_err::err_keyp(
                "lists.err_missing",
                serde_json::json!({ "id": id }),
            )));
        }
        Ok(())
    }

    /// Liste inkl. Einträge, Snapshots und Abhak-Stand löschen. Brücken-Links
    /// auf diese Liste (M9-D) lösen sich mit (source_list_id → NULL) — die
    /// verlinkten Listen bleiben bestehen (dann standalone).
    pub fn list_delete(&self, id: u64) -> Result<()> {
        self.with(|conn| {
            conn.execute(
                "UPDATE lists SET source_list_id = NULL WHERE source_list_id = ?1",
                [id as i64],
            )?;
            conn.execute(
                "DELETE FROM list_item_mats WHERE item_row IN \
                 (SELECT id FROM list_items WHERE list_id = ?1)",
                [id as i64],
            )?;
            conn.execute("DELETE FROM list_items WHERE list_id = ?1", [id as i64])?;
            conn.execute("DELETE FROM list_mat_done WHERE list_id = ?1", [id as i64])?;
            // M9-F: nur die Verknüpfung lösen — die reservierten Inventar-
            // Positionen bleiben im Inventar (Reservierungen mitlöschen).
            conn.execute("DELETE FROM reservations WHERE list_id = ?1", [id as i64])?;
            conn.execute("DELETE FROM lists WHERE id = ?1", [id as i64])?;
            Ok(())
        })
    }

    /// M9-N (t_928d5896): Liste abschließen — der Listen-Level-„Abschließen“-
    /// Action. Nimmt alle reservierten Mengen der Liste AUS DEM INVENTAR:
    /// für jede Reservierung (`reservations WHERE list_id`) wird die Menge
    /// von der Inventar-Position abgezogen (defensiv auf 0 geklemmt — sollte
    /// nie unterschritten sein, aber Klemmen statt unter Null fallen),
    /// danach verschwinden die Listen-Reservierungen (wie `list_delete`).
    /// Freie Zwecke (`list_id` NULL) und `purpose`-Notizen auf denselben
    /// Positionen bleiben unangetastet — konsumiert wird NUR die Listen-
    /// Reservierung. Zum Schluss gilt die Liste als abgearbeitet: jedes
    /// aggregierte Material (Soll = SUM(quantity × copies) über die
    /// Snapshot-Zeilen) bekommt `done_qty` = volle Bedarfsmenge.
    pub fn list_complete(&self, id: u64) -> Result<()> {
        let exists: i64 = self.with(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM lists WHERE id = ?1",
                [id as i64],
                |r| r.get(0),
            )
        })?;
        if exists == 0 {
            return Err(CacheError::Query(i18n_err::err_keyp(
                "lists.err_missing",
                serde_json::json!({ "id": id }),
            )));
        }
        self.with(move |conn| {
            // 1) Konsumieren: je Reservierung die Menge von der Position
            //    abziehen. Mehrere Reservierungen auf einer Position ziehen
            //    mehrfach ab (SUM über die Loop-Iterationen).
            let rows: Vec<(i64, f64)> = {
                let mut stmt = conn.prepare(
                    "SELECT inventory_id, qty FROM reservations \
                     WHERE list_id = ?1 ORDER BY id",
                )?;
                let mapped = stmt.query_map([id as i64], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?))
                })?;
                mapped.collect::<rusqlite::Result<Vec<_>>>()?
            };
            for (inv_id, qty) in rows {
                conn.execute(
                    "UPDATE inventory SET quantity = MAX(quantity - ?1, 0.0), \
                     updated_at = ?2 WHERE id = ?3",
                    rusqlite::params![qty, super::now_unix() as i64, inv_id],
                )?;
            }
            // 2) Reservierungen der Liste lösen (Restbestand bleibt).
            conn.execute("DELETE FROM reservations WHERE list_id = ?1", [id as i64])?;
            // 3) done = volle Bedarfsmenge je (Material, Einheit) — dieselbe
            //    Aggregation wie `list_detail` (SUM(quantity × copies)).
            let mut stmt = conn.prepare(
                "SELECT mim.material, mim.unit, SUM(mim.quantity * li.copies) \
                 FROM list_item_mats mim JOIN list_items li ON li.id = mim.item_row \
                 WHERE li.list_id = ?1 GROUP BY mim.material, mim.unit",
            )?;
            let mapped = stmt.query_map([id as i64], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, f64>(2)?,
                ))
            })?;
            let reqs = mapped.collect::<rusqlite::Result<Vec<_>>>()?;
            drop(stmt);
            for (material, unit, req) in reqs {
                let req = req.max(0.0);
                conn.execute(
                    "INSERT INTO list_mat_done (list_id, material, unit, done_qty) \
                     VALUES (?1, ?2, ?3, ?4) \
                     ON CONFLICT(list_id, material, unit) DO UPDATE SET done_qty = ?4",
                    rusqlite::params![id as i64, material, unit, req],
                )?;
            }
            Ok(())
        })
    }

    /// Blueprint-Eintrag (mit Material-Snapshot) an eine Liste hängen.
    /// Duplikat-Schutz: derselbe item_id hängt NICHT doppelt an einer Liste.
    pub fn list_add_item(&self, list_id: u64, new: NewListItem) -> Result<u64> {
        validate_item(&new)?;
        // Liste existiert? (Existenz separat prüfen — die `with`-Closure
        // spricht rusqlite, semantische Fehler entstehen hier außen.)
        let exists: i64 = self.with(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM lists WHERE id = ?1",
                [list_id as i64],
                |r| r.get(0),
            )
        })?;
        if exists == 0 {
            return Err(CacheError::Query(i18n_err::err_keyp(
                "lists.err_missing",
                serde_json::json!({ "id": list_id }),
            )));
        }
        let dupe: i64 = self.with(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM list_items WHERE list_id = ?1 AND item_id = ?2",
                rusqlite::params![list_id as i64, new.item_id.trim()],
                |r| r.get(0),
            )
        })?;
        if dupe > 0 {
            return Err(CacheError::Query(i18n_err::err_key("lists.err_dupe")));
        }
        let row = self.with(move |conn| {
            conn.execute(
                "INSERT INTO list_items (list_id, item_id, item_name, craft_time, copies, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    list_id as i64,
                    new.item_id.trim(),
                    new.item_name.trim(),
                    new.craft_time as i64,
                    new.copies,
                    super::now_unix() as i64
                ],
            )?;
            let row = conn.last_insert_rowid();
            for m in &new.mats {
                conn.execute(
                    "INSERT INTO list_item_mats (item_row, material, ore_id, unit, quantity, quality) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![
                        row,
                        m.material.trim(),
                        m.ore_id,
                        m.unit.trim(),
                        m.quantity,
                        m.quality as i64
                    ],
                )?;
            }
            Ok(row as u64)
        })?;
        // M9-G: Struktur-Änderung an der Quell-Liste → Spiegel aktualisieren.
        self.with(|conn| sync_linked_mining(conn, list_id))?;
        Ok(row)
    }

    /// Eintrag (inkl. Snapshot) aus der Liste nehmen.
    pub fn list_remove_item(&self, item_row: u64) -> Result<()> {
        let changed = self.with(|conn| {
            let list_id: Option<i64> = conn
                .query_row(
                    "SELECT list_id FROM list_items WHERE id = ?1",
                    [item_row as i64],
                    |r| r.get(0),
                )
                .optional()?;
            let changed = conn.execute(
                "DELETE FROM list_item_mats WHERE item_row = ?1",
                [item_row as i64],
            )?;
            conn.execute("DELETE FROM list_items WHERE id = ?1", [item_row as i64])?;
            // M9-G: Entfernen aus der Quell-Liste → Spiegel aktualisieren.
            if let Some(src) = list_id {
                sync_linked_mining(conn, src as u64)?;
            }
            Ok(changed)
        })?;
        if changed == 0 {
            return Err(CacheError::Query(i18n_err::err_keyp(
                "lists.err_item_missing",
                serde_json::json!({ "id": item_row }),
            )));
        }
        Ok(())
    }

    /// copies eines Eintrags ändern (wie oft gecraftet wird).
    pub fn list_set_copies(&self, item_row: u64, copies: f64) -> Result<()> {
        if !(copies.is_finite() && copies > 0.0) {
            return Err(CacheError::Query(i18n_err::err_key("lists.err_copies")));
        }
        let changed = self.with(|conn| {
            let list_id: Option<i64> = conn
                .query_row(
                    "SELECT list_id FROM list_items WHERE id = ?1",
                    [item_row as i64],
                    |r| r.get(0),
                )
                .optional()?;
            let changed = conn.execute(
                "UPDATE list_items SET copies = ?1 WHERE id = ?2",
                rusqlite::params![copies, item_row as i64],
            )?;
            // M9-G: Mengen-Änderung an der Quell-Liste → Spiegel aktualisieren.
            if let Some(src) = list_id {
                sync_linked_mining(conn, src as u64)?;
            }
            Ok(changed)
        })?;
        if changed == 0 {
            return Err(CacheError::Query(i18n_err::err_keyp(
                "lists.err_item_missing",
                serde_json::json!({ "id": item_row }),
            )));
        }
        Ok(())
    }

    /// Abhak-Stand eines aggregierten Materials setzen. `done = null` löscht
    /// den Eintrag (Reset); sonst wird auf >= 0 geklemmt. „Ganz" setzt die
    /// UI done = Sollmenge, „teilweise" einen freien Wert.
    pub fn list_set_done(
        &self,
        list_id: u64,
        material: &str,
        unit: &str,
        done: Option<f64>,
    ) -> Result<()> {
        if material.trim().is_empty() || unit.trim().is_empty() {
            return Err(CacheError::Query(i18n_err::err_key("lists.err_item")));
        }
        self.with(move |conn| {
            match done {
                None => {
                    conn.execute(
                        "DELETE FROM list_mat_done WHERE list_id = ?1 AND material = ?2 AND unit = ?3",
                        rusqlite::params![list_id as i64, material.trim(), unit.trim()],
                    )?;
                }
                Some(q) => {
                    let q = q.max(0.0);
                    conn.execute(
                        "INSERT INTO list_mat_done (list_id, material, unit, done_qty) \
                         VALUES (?1, ?2, ?3, ?4) \
                         ON CONFLICT(list_id, material, unit) DO UPDATE SET done_qty = ?4",
                        rusqlite::params![list_id as i64, material.trim(), unit.trim(), q],
                    )?;
                }
            }
            // M9-G (t_131614e0), Welle-2 Punkt 2: Abhaken in einer VERKNÜPFTEN
            // Mining-Liste schlägt auf die erzeugende Crafting-Liste durch —
            // derselbe (material, unit)-Stand wird dort gesetzt/gelöscht.
            let (kind, source): (String, Option<i64>) = conn
                .query_row(
                    "SELECT kind, source_list_id FROM lists WHERE id = ?1",
                    [list_id as i64],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap_or_default();
            let sync_to: Option<i64> = if kind == "mining" { source } else { None };
            if let Some(src) = sync_to {
                match done {
                        None => {
                            conn.execute(
                                "DELETE FROM list_mat_done WHERE list_id = ?1 AND material = ?2 AND unit = ?3",
                                rusqlite::params![src, material.trim(), unit.trim()],
                            )?;
                        }
                        Some(q) => {
                            let q = q.max(0.0);
                            conn.execute(
                                "INSERT INTO list_mat_done (list_id, material, unit, done_qty) \
                                 VALUES (?1, ?2, ?3, ?4) \
                                 ON CONFLICT(list_id, material, unit) DO UPDATE SET done_qty = ?4",
                                rusqlite::params![src, material.trim(), unit.trim(), q],
                            )?;
                        }
                }
            }
            Ok(())
        })
    }

    /// M9-E (t_aae8d4c9) — Inventar-Anbindung: Material über eine Liste
    /// hinzufügen. Legt die Inventar-Position an (Ort + Qualität sind
    /// Pflichtfelder des Inventars), verknüpft sie mit der Liste
    /// („reserviert für Liste X") und passt die Qualitäts-Anforderung der
    /// Liste an, wenn die gelieferte Qualität HÖHER ist als die bisher
    /// geforderte (`list_item_mats.quality` der passenden (material, unit)-
    /// Zeilen wird auf die höhere Qualität gehoben — MAX-Regel, „höher ist
    /// immer besser"). Eine niedrigere Qualität ändert NICHTS an der
    /// Anforderung (sie ist eine Mindest-Qualität).
    ///
    /// M9-I (t_e7f0b3a6): Die angelegte Reservierung wird auf den freien
    /// Restlistenbedarf geklemmt (Harter Cap) — übersteht sie keinen Rest,
    /// bleibt die Position ohne Reservierungs-Verweis im Inventar.
    ///
    /// Reservierung und Anforderungs-Anpassung laufen in einem Rutsch auf
    /// derselben Verbindung; die Inventar-Zeile selbst entsteht vorher (dort
    /// sitzt die Validierung) — schlägt der Reservierungs-Teil fehl, bleibt
    /// eine ganz normale Inventar-Position ohne Verweis zurück.
    pub fn inventory_add_for_list(
        &self,
        entry: NewInventoryEntry,
        list_id: u64,
        material: &str,
        unit: &str,
    ) -> Result<ReservedAdd> {
        if material.trim().is_empty() || unit.trim().is_empty() {
            return Err(CacheError::Query(i18n_err::err_key("lists.err_item")));
        }
        // Liste existiert? Vor dem Inventar-Insert prüfen, damit ein
        // Tippfehler in der Listen-Id NICHT erst eine Position anlegt.
        let exists: i64 = self.with(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM lists WHERE id = ?1",
                [list_id as i64],
                |r| r.get(0),
            )
        })?;
        if exists == 0 {
            return Err(CacheError::Query(i18n_err::err_keyp(
                "lists.err_missing",
                serde_json::json!({ "id": list_id }),
            )));
        }
        let quality = entry.quality;
        let qty = entry.quantity;
        let mat = material.trim().to_string();
        let inv_id = self.inventory_add(entry)?;
        let (raised, requirement) = self.with(move |conn| {
            // M9-F: Reservierung über das neue Inventar-first-Primitiv —
            // Zweck = die Liste, Material in der note (wie bei der
            // v16-Übernahme). M9-I (t_e7f0b3a6): HARTER CAP — die Reservierung
            // wird auf den freien Restlistenbedarf geklemmt; bleibt keiner,
            // entsteht eine ganz normale Inventar-Position ohne Verweis.
            clamp_list_material_reservations(conn, list_id as i64, &mat)?;
            let req = list_material_requirement(conn, list_id as i64, &mat)?;
            let res_sum = list_material_reserved(conn, list_id as i64, &mat)?;
            let res_qty = (qty.min((req - res_sum).max(0.0)) * 1e6).round() / 1e6;
            if res_qty > 1e-9 {
                conn.execute(
                    "INSERT INTO reservations (inventory_id, list_id, purpose, qty, note, created_at) \
                     VALUES (?1, ?2, '', ?3, ?4, ?5)",
                    rusqlite::params![
                        inv_id as i64,
                        list_id as i64,
                        res_qty,
                        mat,
                        super::now_unix() as i64
                    ],
                )?;
            }
            // Bisherige Anforderung (MAX über die Snapshot-Zeilen).
            let old: i64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(quality), 0) FROM list_item_mats \
                     WHERE item_row IN (SELECT id FROM list_items WHERE list_id = ?1) \
                       AND material = ?2 AND unit = ?3",
                    rusqlite::params![list_id as i64, material.trim(), unit.trim()],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            // Höhere gelieferte Qualität hebt die Anforderung an.
            let changed = conn.execute(
                "UPDATE list_item_mats SET quality = ?1 \
                 WHERE quality < ?1 \
                   AND item_row IN (SELECT id FROM list_items WHERE list_id = ?2) \
                   AND material = ?3 AND unit = ?4",
                rusqlite::params![quality as i64, list_id as i64, material.trim(), unit.trim()],
            )?;
            // M9-G R1-Fix (Review-Befunde 2+3): Liste kind + Quelle lesen.
            let (kind, source): (String, Option<i64>) = conn
                .query_row(
                    "SELECT kind, source_list_id FROM lists WHERE id = ?1",
                    [list_id as i64],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap_or_default();
            // Fix 3: Anhebung über eine VERKNÜPFTE Mining-Liste hebt die
            // CRAFT-Quelle an (Single Source of Truth) — sonst wirft der
            // nächste Craft-Sync die Anhebung weg (500→300), während die
            // Q500-Reservierung bleibt. M9-E-Regel „höher ist immer besser"
            // bleibt damit dauerhaft.
            let raised_src = if kind == "mining" {
                match source {
                    Some(src) => {
                        let changed2 = conn.execute(
                            "UPDATE list_item_mats SET quality = ?1 \
                             WHERE quality < ?1 \
                               AND item_row IN (SELECT id FROM list_items WHERE list_id = ?2) \
                               AND material = ?3 AND unit = ?4",
                            rusqlite::params![quality as i64, src, material.trim(), unit.trim()],
                        )?;
                        changed2 > 0
                    }
                    None => false,
                }
            } else {
                false
            };
            // Fix 2: Qualitäts-Anhebung ändert das Material-Bild der Quell-
            // Liste → Spiegel sofort aktualisieren (Craft-Liste direkt,
            // verknüpfte Mining-Liste über ihre Quelle).
            let sync_src: i64 = if kind == "craft" {
                list_id as i64
            } else {
                source.unwrap_or(-1)
            };
            if sync_src > 0 {
                sync_linked_mining(conn, sync_src as u64)?;
            }
            Ok((changed > 0 || raised_src, (old.max(quality as i64)) as u32))
        })?;
        let entry = self
            .inventory_list()?
            .into_iter()
            .find(|e| e.id == inv_id)
            .ok_or_else(|| {
                CacheError::Query(i18n_err::err_keyp(
                    "inventory.err_position_missing",
                    serde_json::json!({ "id": inv_id }),
                ))
            })?;
        Ok(ReservedAdd {
            entry,
            requirement_raised: raised,
            requirement,
        })
    }

    /// Volle Liste: Kopf + Einträge (mit Snapshots) + Material-Aggregation.
    pub fn list_detail(&self, id: u64) -> Result<ListDetail> {
        // M9-I (t_e7f0b3a6), Welle-3 P6: Listen-Öffnen klemmt bestehende
        // Reservierungen hart auf den Listenbedarf — Mengen-Schrumpfungen
        // (copies/Entfernen) werden hier nachgezogen (User-Beschluss,
        // docs/LISTEN-MODE-PLAN.md Welle 3).
        self.with(|conn| clamp_list_reservations(conn, id as i64))?;
        let head: Option<(String, String, i64)> = self.with(|conn| {
            conn.query_row(
                "SELECT kind, name, created_at FROM lists WHERE id = ?1",
                [id as i64],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
        })?;
        let Some((kind, name, created_at)) = head else {
            return Err(CacheError::Query(i18n_err::err_keyp(
                "lists.err_missing",
                serde_json::json!({ "id": id }),
            )));
        };

        // Einträge + Material-Snapshots + done-Stand in einer Sperre lesen.
        let (items, mut done_map, reserved_map) = self.with(|conn| {
            let mut items: Vec<ListItem> = Vec::new();
            {
                let mut stmt = conn.prepare(
                    "SELECT id, item_id, item_name, craft_time, copies, created_at \
                     FROM list_items WHERE list_id = ?1 ORDER BY id ASC",
                )?;
                let mut rows = stmt.query([id as i64])?;
                while let Some(r) = rows.next()? {
                    let item = ListItem {
                        id: r.get::<_, i64>(0)? as u64,
                        item_id: r.get(1)?,
                        item_name: r.get(2)?,
                        craft_time: r.get::<_, i64>(3)? as u64,
                        copies: r.get(4)?,
                        created_at: r.get::<_, i64>(5)? as u64,
                        mats: Vec::new(),
                    };
                    items.push(item);
                }
            }
            {
                let mut stmt = conn.prepare(
                    "SELECT item_row, material, ore_id, unit, quantity, quality \
                     FROM list_item_mats ORDER BY id ASC",
                )?;
                let mut rows = stmt.query([])?;
                let mut by_item: std::collections::HashMap<i64, Vec<ListItemMat>> =
                    std::collections::HashMap::new();
                while let Some(r) = rows.next()? {
                    let row: i64 = r.get(0)?;
                    by_item.entry(row).or_default().push(ListItemMat {
                        material: r.get(1)?,
                        ore_id: r.get(2)?,
                        unit: r.get(3)?,
                        quantity: r.get(4)?,
                        quality: r.get::<_, i64>(5)? as u32,
                    });
                }
                for it in &mut items {
                    if let Some(mats) = by_item.remove(&(it.id as i64)) {
                        it.mats = mats;
                    }
                }
            }
            let mut done_map: std::collections::HashMap<(String, String), f64> =
                std::collections::HashMap::new();
            {
                let mut stmt = conn.prepare(
                    "SELECT material, unit, done_qty FROM list_mat_done WHERE list_id = ?1",
                )?;
                let mut rows = stmt.query([id as i64])?;
                while let Some(r) = rows.next()? {
                    let m: String = r.get(0)?;
                    let u: String = r.get(1)?;
                    done_map.insert((m, u), r.get(2)?);
                }
            }
            // M9-F (t_bb97da62): aus dem Inventar reservierte Mengen + mengen-
            // gewichtete Ø-Qualität je Material (Reservierungen dieser Liste,
            // gematcht über den Inventar-Namen — groß/klein egal).
            let mut reserved_map: std::collections::HashMap<String, (f64, Option<u32>)> =
                std::collections::HashMap::new();
            {
                let mut stmt = conn.prepare(
                    "SELECT LOWER(i.name) AS m, SUM(res.qty), \
                            SUM(res.qty * i.quality) / SUM(res.qty) \
                     FROM reservations res JOIN inventory i ON i.id = res.inventory_id \
                     WHERE res.list_id = ?1 GROUP BY LOWER(i.name)",
                )?;
                let mut rows = stmt.query([id as i64])?;
                while let Some(r) = rows.next()? {
                    let m: String = r.get(0)?;
                    let sum: f64 = r.get(1)?;
                    let avg: f64 = r.get(2)?;
                    reserved_map.insert(m, (sum, Some(avg.round() as u32)));
                }
            }
            Ok((items, done_map, reserved_map))
        })?;

        // Aggregation: (material, unit) → (Summe, MAX-Qualität). done
        // kommt aus list_mat_done. Sortierung: Materialname case-insens.
        let mut agg: std::collections::BTreeMap<(String, String), (f64, u32)> =
            std::collections::BTreeMap::new();
        for it in &items {
            for m in &it.mats {
                let e = agg
                    .entry((m.material.clone(), m.unit.clone()))
                    .or_insert((0.0, 0));
                e.0 += m.quantity * it.copies;
                e.1 = e.1.max(m.quality);
            }
        }
        let mut aggregated: Vec<AggMat> = agg
            .into_iter()
            .map(|((material, unit), (quantity, q))| {
                let (rq, ravg) = reserved_map
                    .get(&material.to_lowercase())
                    .copied()
                    .unwrap_or((0.0, None));
                AggMat {
                    done_qty: done_map
                        .remove(&(material.clone(), unit.clone()))
                        .unwrap_or(0.0),
                    min_quality: q,
                    quantity,
                    reserved_qty: rq,
                    reserved_avg_quality: ravg,
                    material,
                    unit,
                }
            })
            .collect();
        aggregated.sort_by_key(|a| a.material.to_lowercase());

        Ok(ListDetail {
            id,
            kind,
            name,
            created_at: created_at as u64,
            items,
            aggregated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reservations::NewReservation;

    fn bp(id: &str, name: &str, copies: f64, mats: Vec<(&str, &str, f64, u32)>) -> NewListItem {
        NewListItem {
            item_id: id.into(),
            item_name: name.into(),
            craft_time: 120,
            copies,
            mats: mats
                .into_iter()
                .map(|(m, u, q, quality)| NewListItemMat {
                    material: m.into(),
                    ore_id: None,
                    unit: u.into(),
                    quantity: q,
                    quality,
                })
                .collect(),
        }
    }

    #[test]
    fn m9g_sync_mirrors_craft_changes_into_linked_mining() {
        let c = Cache::open_in_memory().unwrap();
        let craft = c.list_create("craft", "Craft", None).unwrap();
        let mining = c.list_create("mining", "Mine", Some(craft)).unwrap();

        // Startzustand: 2 Blueprints → 2 Materialien in der Spiegel-Liste.
        let a = c
            .list_add_item(
                craft,
                bp("a", "A", 2.0, vec![("Hadanite", "gems", 100.0, 500)]),
            )
            .unwrap();
        c.list_add_item(
            craft,
            bp("b", "B", 1.0, vec![("Taranite", "SCU", 50.0, 300)]),
        )
        .unwrap();
        let d = c.list_detail(mining).unwrap();
        assert_eq!(d.items.len(), 2);
        let had = d
            .aggregated
            .iter()
            .find(|m| m.material == "Hadanite")
            .unwrap();
        assert_eq!(had.quantity, 200.0);
        assert_eq!(had.min_quality, 500);

        // Abhaken auf der Mining-Seite (durchschlägt später) — hier Stand setzen.
        c.list_set_done(mining, "Hadanite", "gems", Some(120.0))
            .unwrap();

        // copies ändern → Spiegel folgt sofort.
        c.list_set_copies(a, 3.0).unwrap();
        let d = c.list_detail(mining).unwrap();
        let had = d
            .aggregated
            .iter()
            .find(|m| m.material == "Hadanite")
            .unwrap();
        assert_eq!(had.quantity, 300.0);
        // done bleibt für weitergeführtes Material erhalten.
        assert_eq!(had.done_qty, 120.0);

        // Eintrag entfernen → Material verschwindet (inkl. done-Stand).
        c.list_remove_item(a).unwrap();
        let d = c.list_detail(mining).unwrap();
        assert_eq!(d.items.len(), 1);
        assert!(d.aggregated.iter().all(|m| m.material != "Hadanite"));
        let tar = d
            .aggregated
            .iter()
            .find(|m| m.material == "Taranite")
            .unwrap();
        assert_eq!(tar.quantity, 50.0);
        assert_eq!(tar.done_qty, 0.0);

        // done über der neuen Sollmenge wird beim Spiegeln geklemmt.
        c.list_set_done(mining, "Taranite", "SCU", Some(100.0))
            .unwrap();
        c.list_set_copies(c.list_detail(craft).unwrap().items[0].id, 1.0)
            .unwrap();
        // Taranite bleibt 50 — kein weiteres Blueprint-Item beteiligt; aber
        // ein NEUES Material erscheint automatisch:
        c.list_add_item(
            craft,
            bp("c", "C", 2.0, vec![("Quantainium", "SCU", 10.0, 200)]),
        )
        .unwrap();
        let d = c.list_detail(mining).unwrap();
        let q = d
            .aggregated
            .iter()
            .find(|m| m.material == "Quantainium")
            .unwrap();
        assert_eq!(q.quantity, 20.0);
        assert_eq!(q.min_quality, 200);
        // Taranite done wurde auf Soll (50) geklemmt.
        let tar = d
            .aggregated
            .iter()
            .find(|m| m.material == "Taranite")
            .unwrap();
        assert_eq!(tar.done_qty, 50.0);

        // Unverknüpfte Mining-Listen bleiben unberührt.
        let solo = c.list_create("mining", "Solo", None).unwrap();
        c.list_add_item(solo, bp("s", "S", 1.0, vec![("Inert", "SCU", 5.0, 100)]))
            .unwrap();
        c.list_set_copies(c.list_detail(craft).unwrap().items[0].id, 9.0)
            .unwrap();
        let d = c.list_detail(solo).unwrap();
        assert_eq!(d.items.len(), 1);
        assert_eq!(d.aggregated[0].material, "Inert");
    }

    #[test]
    fn m9g_mining_done_propagates_to_craft_source() {
        let c = Cache::open_in_memory().unwrap();
        let craft = c.list_create("craft", "Craft", None).unwrap();
        let mining = c.list_create("mining", "Mine", Some(craft)).unwrap();
        c.list_add_item(
            craft,
            bp("a", "A", 2.0, vec![("Hadanite", "gems", 100.0, 500)]),
        )
        .unwrap();

        // Ganz: Mining → Craft übernimmt done = Soll.
        c.list_set_done(mining, "Hadanite", "gems", Some(200.0))
            .unwrap();
        let cd = c.list_detail(craft).unwrap();
        assert_eq!(cd.aggregated[0].done_qty, 200.0);

        // Teilweise: Wert wird 1:1 durchgeschlagen.
        c.list_set_done(mining, "Hadanite", "gems", Some(70.0))
            .unwrap();
        let cd = c.list_detail(craft).unwrap();
        assert_eq!(cd.aggregated[0].done_qty, 70.0);

        // Reset: Craft-Seite wird mit geleert.
        c.list_set_done(mining, "Hadanite", "gems", None).unwrap();
        let cd = c.list_detail(craft).unwrap();
        assert_eq!(cd.aggregated[0].done_qty, 0.0);

        // Unverknüpfte Mining-Liste: KEIN Durchschlag.
        let solo = c.list_create("mining", "Solo", None).unwrap();
        c.list_set_done(solo, "Hadanite", "gems", Some(5.0))
            .unwrap();
        let cd = c.list_detail(craft).unwrap();
        assert_eq!(cd.aggregated[0].done_qty, 0.0);
    }

    // M9-G R1 (Review-Befund 1): done-Klemmung beim Spiegeln darf die
    // Quell-Liste nicht auf dem alten (höheren) Stand lassen — beide
    // Ansichten desselben Materials müssen identisch sein.
    #[test]
    fn m9g_done_clamp_writes_back_to_craft_source() {
        let c = Cache::open_in_memory().unwrap();
        let craft = c.list_create("craft", "Craft", None).unwrap();
        let mining = c.list_create("mining", "Mine", Some(craft)).unwrap();
        let item = c
            .list_add_item(
                craft,
                bp("a", "A", 1.0, vec![("Taranite", "SCU", 50.0, 300)]),
            )
            .unwrap();

        // done 100 > Soll 50: Durchschlag setzt BEIDE auf 100.
        c.list_set_done(mining, "Taranite", "SCU", Some(100.0))
            .unwrap();
        assert_eq!(c.list_detail(craft).unwrap().aggregated[0].done_qty, 100.0);

        // Mengen-Schrumpfung: Soll 25. Beide Seiten müssen auf 25 geklemmt
        // werden — nicht Mining 25 / Craft 100.
        c.list_set_copies(item, 0.5).unwrap();
        let md = c.list_detail(mining).unwrap().aggregated[0].clone();
        let cd = c.list_detail(craft).unwrap().aggregated[0].clone();
        assert_eq!(md.quantity, 25.0);
        assert_eq!(cd.quantity, 25.0);
        assert_eq!(md.done_qty, 25.0, "Mining-Seite geklemmt");
        assert_eq!(cd.done_qty, 25.0, "Craft-Seite ebenfalls geklemmt");
    }

    // M9-G R1 (Review-Befund 2): Qualitäts-Anhebung der CRAFT-Liste über
    // inventory_add_for_list spiegelt sich sofort in der verknüpften
    // Mining-Liste.
    #[test]
    fn m9g_craft_quality_raise_syncs_to_linked_mining() {
        let c = Cache::open_in_memory().unwrap();
        let craft = c.list_create("craft", "Craft", None).unwrap();
        let mining = c.list_create("mining", "Mine", Some(craft)).unwrap();
        c.list_add_item(
            craft,
            bp("a", "A", 1.0, vec![("Taranite", "SCU", 50.0, 300)]),
        )
        .unwrap();
        let inv = |q: u32| NewInventoryEntry {
            entity_kind: None,
            entity_id: None,
            name: "Taranite".into(),
            quality: q,
            location_id: None,
            location_name: "Levski".into(),
            quantity: 50.0,
            price: None,
        };

        // Craft-Qualität 300 → 500 über die Craft-Liste.
        c.inventory_add_for_list(inv(500), craft, "Taranite", "SCU")
            .unwrap();
        assert_eq!(c.list_detail(craft).unwrap().aggregated[0].min_quality, 500);
        assert_eq!(
            c.list_detail(mining).unwrap().aggregated[0].min_quality,
            500,
            "Spiegel folgt der Qualitäts-Anhebung"
        );
    }

    // M9-G R1 (Review-Befund 3): Qualitäts-Anhebung über die VERKNÜPFTE
    // Mining-Liste hebt die Craft-Quelle mit an — der nächste Craft-Sync
    // darf sie nicht auf die alte Aggregation zurücksetzen.
    #[test]
    fn m9g_mining_quality_raise_survives_next_craft_sync() {
        let c = Cache::open_in_memory().unwrap();
        let craft = c.list_create("craft", "Craft", None).unwrap();
        let mining = c.list_create("mining", "Mine", Some(craft)).unwrap();
        let item = c
            .list_add_item(
                craft,
                bp("a", "A", 1.0, vec![("Taranite", "SCU", 50.0, 300)]),
            )
            .unwrap();
        let inv = |q: u32| NewInventoryEntry {
            entity_kind: None,
            entity_id: None,
            name: "Taranite".into(),
            quality: q,
            location_id: None,
            location_name: "Levski".into(),
            quantity: 50.0,
            price: None,
        };

        // Anhebung 300 → 500 über die Mining-Liste (submitAddInv-Flow).
        let r = c
            .inventory_add_for_list(inv(500), mining, "Taranite", "SCU")
            .unwrap();
        assert!(r.requirement_raised);
        assert_eq!(r.requirement, 500);
        assert_eq!(
            c.list_detail(craft).unwrap().aggregated[0].min_quality,
            500,
            "Craft-Quelle wurde mit angehoben"
        );

        // Nächster Craft-Trigger: Anforderung bleibt 500, Reservierung (Q500)
        // bleibt konsistent.
        c.list_set_copies(item, 2.0).unwrap();
        let md = c.list_detail(mining).unwrap().aggregated[0].clone();
        let cd = c.list_detail(craft).unwrap().aggregated[0].clone();
        assert_eq!(md.quantity, 100.0);
        assert_eq!(cd.quantity, 100.0);
        assert_eq!(md.min_quality, 500);
        assert_eq!(cd.min_quality, 500);
    }

    /// M9-G R1-Fix (Review-Befund 1): Nach done > Soll + Mengen-Schrumpfung
    /// zeigen beide verknüpften Ansichten denselben (geklammerten) Stand.
    #[test]
    fn m9g_done_clamp_propagates_back_to_craft_source() {
        let c = Cache::open_in_memory().unwrap();
        let craft = c.list_create("craft", "Craft", None).unwrap();
        let mining = c.list_create("mining", "Mine", Some(craft)).unwrap();
        let a = c
            .list_add_item(
                craft,
                bp("b", "B", 1.0, vec![("Taranite", "SCU", 50.0, 300)]),
            )
            .unwrap();
        // done=100 > Soll 50 — Durchschlag setzt Craft auf 100.
        c.list_set_done(mining, "Taranite", "SCU", Some(100.0))
            .unwrap();
        assert_eq!(c.list_detail(craft).unwrap().aggregated[0].done_qty, 100.0);
        // Mengen-Schrumpfung: Soll 25 → Mining GEKLAMMT auf 25 …
        c.list_set_copies(a, 0.5).unwrap();
        let md = c.list_detail(mining).unwrap();
        assert_eq!(md.aggregated[0].quantity, 25.0);
        assert_eq!(md.aggregated[0].done_qty, 25.0);
        // … und Craft bekommt denselben geklemmten Wert (kein Divergieren).
        let cd = c.list_detail(craft).unwrap();
        assert_eq!(cd.aggregated[0].quantity, 25.0);
        assert_eq!(cd.aggregated[0].done_qty, 25.0);
    }

    /// M9-G R1-Fix (Review-Befund 2): Qualitäts-Anhebung an der CRAFT-Liste
    /// (M9-E-Flow) spiegelt sich sofort in die verknüpfte Mining-Liste.
    #[test]
    fn m9g_craft_quality_raise_syncs_into_linked_mining() {
        let c = Cache::open_in_memory().unwrap();
        let craft = c.list_create("craft", "Craft", None).unwrap();
        let mining = c.list_create("mining", "Mine", Some(craft)).unwrap();
        c.list_add_item(
            craft,
            bp("b", "B", 1.0, vec![("Taranite", "SCU", 50.0, 300)]),
        )
        .unwrap();
        let entry = NewInventoryEntry {
            entity_kind: None,
            entity_id: None,
            name: "Taranite".into(),
            quality: 500,
            location_id: None,
            location_name: "Levski".into(),
            quantity: 25.0,
            price: None,
        };
        c.inventory_add_for_list(entry, craft, "Taranite", "SCU")
            .unwrap();
        // Craft-Anforderung 500, verknüpfte Mining-Liste FOLGT (nicht 300).
        assert_eq!(c.list_detail(craft).unwrap().aggregated[0].min_quality, 500);
        assert_eq!(
            c.list_detail(mining).unwrap().aggregated[0].min_quality,
            500
        );
    }

    /// M9-G R1-Fix (Review-Befund 3): Qualitäts-Anhebung über die VERKNÜPFTE
    /// Mining-Liste hebt die Craft-Quelle an und überlebt den nächsten
    /// Craft-Sync (Reservierung Q500 bleibt, Anforderung fällt nicht zurück).
    #[test]
    fn m9g_mining_quality_raise_survives_craft_sync() {
        let c = Cache::open_in_memory().unwrap();
        let craft = c.list_create("craft", "Craft", None).unwrap();
        let mining = c.list_create("mining", "Mine", Some(craft)).unwrap();
        let a = c
            .list_add_item(
                craft,
                bp("b", "B", 1.0, vec![("Taranite", "SCU", 50.0, 300)]),
            )
            .unwrap();
        let entry = NewInventoryEntry {
            entity_kind: None,
            entity_id: None,
            name: "Taranite".into(),
            quality: 500,
            location_id: None,
            location_name: "Levski".into(),
            quantity: 25.0,
            price: None,
        };
        let r = c
            .inventory_add_for_list(entry, mining, "Taranite", "SCU")
            .unwrap();
        assert!(r.requirement_raised);
        assert_eq!(r.requirement, 500);
        // Nächster Craft-Trigger (Sync-Neuaufbau): Anforderung bleibt 500.
        c.list_set_copies(a, 2.0).unwrap();
        let cd = c.list_detail(craft).unwrap();
        assert_eq!(cd.aggregated[0].min_quality, 500);
        let md = c.list_detail(mining).unwrap();
        assert_eq!(md.aggregated[0].min_quality, 500);
        // Reservierung existiert weiterhin.
        assert!(
            c.inventory_list()
                .unwrap()
                .iter()
                .all(|e| e.reservations.iter().any(|res| res.list_id == Some(craft)))
        );
    }

    #[test]
    fn crud_roundtrip() {
        let c = Cache::open_in_memory().unwrap();
        assert!(c.lists_list("craft").unwrap().is_empty());
        assert!(c.list_create("nope", "x", None).is_err());
        assert!(c.list_create("craft", "  ", None).is_err());

        let id = c.list_create("craft", "FPS-Set", None).unwrap();
        let lists = c.lists_list("craft").unwrap();
        assert_eq!(lists.len(), 1);
        assert_eq!(lists[0].name, "FPS-Set");
        assert_eq!(lists[0].item_count, 0);
        // Mining-Listen erscheinen nicht im Craft-View.
        assert!(c.lists_list("mining").unwrap().is_empty());

        c.list_rename(id, "Schiffs-Set").unwrap();
        assert_eq!(c.lists_list("craft").unwrap()[0].name, "Schiffs-Set");

        let item = c
            .list_add_item(
                id,
                bp("r1", "Rifle", 2.0, vec![("Hadanite", "gems", 300.0, 500)]),
            )
            .unwrap();
        assert_eq!(c.lists_list("craft").unwrap()[0].item_count, 1);
        // Duplikat-Schutz
        assert!(
            c.list_add_item(
                id,
                bp("r1", "Rifle", 1.0, vec![("Hadanite", "gems", 1.0, 500)])
            )
            .is_err()
        );

        c.list_set_copies(item, 3.0).unwrap();
        let d = c.list_detail(id).unwrap();
        assert_eq!(d.items[0].copies, 3.0);
        assert_eq!(d.items[0].mats.len(), 1);
        assert_eq!(d.items[0].mats[0].quality, 500);

        c.list_remove_item(item).unwrap();
        assert!(c.list_detail(id).unwrap().items.is_empty());
        c.list_delete(id).unwrap();
        assert!(c.lists_list("craft").unwrap().is_empty());
        assert!(c.list_detail(id).is_err());
    }

    #[test]
    fn aggregation_sums_quantities_and_maxes_quality() {
        let c = Cache::open_in_memory().unwrap();
        let id = c.list_create("craft", "Set", None).unwrap();
        // Zwei Blueprints, gemeinsames Material Hadanite (gems), unterschiedliche
        // Qualitäten → MAX gewinnt; verschiedene Einheiten bleiben getrennt.
        c.list_add_item(
            id,
            bp(
                "a",
                "Alpha",
                2.0,
                vec![
                    ("Hadanite", "gems", 250.0, 500),
                    ("Quantum Core", "items", 1.0, 300),
                ],
            ),
        )
        .unwrap();
        c.list_add_item(
            id,
            bp(
                "b",
                "Beta",
                1.0,
                vec![
                    ("Hadanite", "gems", 300.0, 800),
                    ("Hadanite", "SCU", 0.3, 600),
                ],
            ),
        )
        .unwrap();
        let d = c.list_detail(id).unwrap();
        let agg = &d.aggregated;
        assert_eq!(agg.len(), 3);
        let hadanite_gems = agg
            .iter()
            .find(|m| m.material == "Hadanite" && m.unit == "gems")
            .unwrap();
        assert_eq!(hadanite_gems.quantity, 250.0 * 2.0 + 300.0); // × copies
        assert_eq!(hadanite_gems.min_quality, 800); // MAX, nicht Mittelwert
        assert_eq!(hadanite_gems.done_qty, 0.0);
        let hadanite_scu = agg
            .iter()
            .find(|m| m.material == "Hadanite" && m.unit == "SCU")
            .unwrap();
        assert_eq!(hadanite_scu.quantity, 0.3);
        assert_eq!(hadanite_scu.min_quality, 600);
        let qc = agg.iter().find(|m| m.material == "Quantum Core").unwrap();
        assert_eq!(qc.min_quality, 300);
        // Case-insensitive Sortierung nach Material.
        let names: Vec<&str> = agg.iter().map(|m| m.material.as_str()).collect();
        assert_eq!(names, ["Hadanite", "Hadanite", "Quantum Core"]);
    }

    #[test]
    fn done_tracking_ganz_teilweise_reset() {
        let c = Cache::open_in_memory().unwrap();
        let id = c.list_create("craft", "Set", None).unwrap();
        c.list_add_item(
            id,
            bp("a", "A", 1.0, vec![("Hadanite", "gems", 300.0, 500)]),
        )
        .unwrap();

        // Teilweise: 250 von 300.
        c.list_set_done(id, "Hadanite", "gems", Some(250.0))
            .unwrap();
        let d = c.list_detail(id).unwrap();
        assert_eq!(d.aggregated[0].done_qty, 250.0);

        // Ganz: auf Sollmenge.
        c.list_set_done(id, "Hadanite", "gems", Some(300.0))
            .unwrap();
        assert_eq!(c.list_detail(id).unwrap().aggregated[0].done_qty, 300.0);

        // Negativ wird geklemmt, kein Fehler.
        c.list_set_done(id, "Hadanite", "gems", Some(-5.0)).unwrap();
        assert_eq!(c.list_detail(id).unwrap().aggregated[0].done_qty, 0.0);

        // Reset (null löscht).
        c.list_set_done(id, "Hadanite", "gems", None).unwrap();
        assert_eq!(c.list_detail(id).unwrap().aggregated[0].done_qty, 0.0);

        // Done-Stand überlebt Eintrags-Änderungen und Listen-Neuaufbau.
        c.list_set_done(id, "Hadanite", "gems", Some(100.0))
            .unwrap();
        c.list_set_copies(c.list_detail(id).unwrap().items[0].id, 4.0)
            .unwrap();
        let d = c.list_detail(id).unwrap();
        assert_eq!(d.aggregated[0].quantity, 300.0 * 4.0);
        assert_eq!(d.aggregated[0].done_qty, 100.0);

        // Löschen der Liste räumt den done-Stand weg (kein Wiederauftauchen).
        c.list_delete(id).unwrap();
        let id2 = c.list_create("craft", "Set", None).unwrap();
        c.list_add_item(
            id2,
            bp("a", "A", 1.0, vec![("Hadanite", "gems", 300.0, 500)]),
        )
        .unwrap();
        assert_eq!(c.list_detail(id2).unwrap().aggregated[0].done_qty, 0.0);
    }

    #[test]
    fn validation_rejects_bad_payloads() {
        let c = Cache::open_in_memory().unwrap();
        let id = c.list_create("craft", "Set", None).unwrap();
        // leere mats
        assert!(c.list_add_item(id, bp("x", "X", 1.0, vec![])).is_err());
        // Qualität > 1000
        assert!(
            c.list_add_item(id, bp("x", "X", 1.0, vec![("H", "gems", 1.0, 1001)]))
                .is_err()
        );
        // copies <= 0 / nicht endlich
        assert!(
            c.list_add_item(id, bp("x", "X", 0.0, vec![("H", "gems", 1.0, 5)]))
                .is_err()
        );
        assert!(
            c.list_add_item(id, bp("x", "X", f64::NAN, vec![("H", "gems", 1.0, 5)]))
                .is_err()
        );
        // Liste existiert nicht
        assert!(
            c.list_add_item(999_999, bp("x", "X", 1.0, vec![("H", "gems", 1.0, 5)]))
                .is_err()
        );
        assert!(c.list_rename(999_999, "n").is_err());
    }

    #[test]
    fn inventory_add_for_list_reserves_and_raises_requirement() {
        let c = Cache::open_in_memory().unwrap();
        let id = c.list_create("craft", "Set", None).unwrap();
        c.list_add_item(
            id,
            bp(
                "a",
                "A",
                1.0,
                vec![
                    ("Hadanite", "gems", 300.0, 500),
                    ("Quantum Core", "items", 1.0, 800),
                ],
            ),
        )
        .unwrap();

        let inv = |q: u32, name: &str| NewInventoryEntry {
            entity_kind: None,
            entity_id: None,
            name: name.into(),
            quality: q,
            location_id: None,
            location_name: "Levski".into(),
            quantity: 250.0,
            price: None,
        };

        // Niedrigere als die GEFORNDERTE? Nein — 700 > 500: höhere gelieferte
        // Qualität hebt die Anforderung an („höher ist immer besser").
        let r = c
            .inventory_add_for_list(inv(700, "Hadanite"), id, "Hadanite", "gems")
            .unwrap();
        assert!(r.requirement_raised);
        assert_eq!(r.requirement, 700);
        assert_eq!(r.entry.reservations[0].list_id, Some(id));
        assert_eq!(r.entry.reservations[0].list_name, Some("Set".into()));
        assert_eq!(r.entry.quality, 700);
        // Anforderung der Snapshot-Zeilen auf 700 angehoben.
        let d = c.list_detail(id).unwrap();
        let had = d
            .aggregated
            .iter()
            .find(|m| m.material == "Hadanite")
            .unwrap();
        assert_eq!(had.min_quality, 700);
        assert_eq!(
            d.items[0]
                .mats
                .iter()
                .find(|m| m.material == "Hadanite")
                .unwrap()
                .quality,
            700
        );

        // Niedrigere oder gleiche Qualität ändert NICHTS an der Anforderung
        // (Mindest-Qualität — sie wird nicht gesenkt).
        let r0 = c
            .inventory_add_for_list(inv(600, "Hadanite"), id, "Hadanite", "gems")
            .unwrap();
        assert!(!r0.requirement_raised);
        assert_eq!(r0.requirement, 700);

        // Höhere Qualität HEBELT die Anforderung weiter an (nur Hadanite/gems —
        // Quantum Core bleibt bei 800).
        let r2 = c
            .inventory_add_for_list(inv(900, "Hadanite"), id, "Hadanite", "gems")
            .unwrap();
        assert!(r2.requirement_raised);
        assert_eq!(r2.requirement, 900);
        let d = c.list_detail(id).unwrap();
        assert_eq!(
            d.items[0]
                .mats
                .iter()
                .find(|m| m.material == "Hadanite")
                .unwrap()
                .quality,
            900
        );
        assert_eq!(
            d.items[0]
                .mats
                .iter()
                .find(|m| m.material == "Quantum Core")
                .unwrap()
                .quality,
            800
        );

        // M9-I: Harter Cap — Bedarf 300 (Menge, unabhängig von der Qualität):
        // erste Position reserviert 250, zweite nur den Rest (50), dritte
        // bleibt ohne Reservierung.
        let invs = c.inventory_list().unwrap();
        assert_eq!(invs.len(), 3);
        let by_q = |q: u32| invs.iter().find(|e| e.quality == q).unwrap();
        let r700 = &by_q(700).reservations;
        assert_eq!(r700.len(), 1);
        assert_eq!(r700[0].list_id, Some(id));
        assert_eq!(r700[0].list_name.as_deref(), Some("Set"));
        assert_eq!(r700[0].qty, 250.0);
        assert_eq!(by_q(600).reservations[0].qty, 50.0);
        assert!(
            by_q(900).reservations.is_empty(),
            "Cap aufgebraucht — keine Reservierung"
        );
        // Aggregation zeigt die geklemmte Summe (exakt der Bedarf).
        assert_eq!(
            c.list_detail(id)
                .unwrap()
                .aggregated
                .iter()
                .find(|m| m.material == "Hadanite")
                .unwrap()
                .reserved_qty,
            300.0
        );

        // Validierung: leeres Material / fehlende Liste legt KEINE Position an.
        assert!(
            c.inventory_add_for_list(inv(500, "X"), id, " ", "gems")
                .is_err()
        );
        let before = c.inventory_list().unwrap().len();
        assert!(
            c.inventory_add_for_list(inv(500, "X"), 999_999, "Hadanite", "gems")
                .is_err()
        );
        assert_eq!(c.inventory_list().unwrap().len(), before);
        // Qualität > 1000 fliegt (Inventar-Validierung greift auch hier).
        assert!(
            c.inventory_add_for_list(inv(1001, "X"), id, "Hadanite", "gems")
                .is_err()
        );

        // Liste löschen: Verknüpfung löst sich, Positionen bleiben.
        c.list_delete(id).unwrap();
        let invs = c.inventory_list().unwrap();
        assert_eq!(invs.len(), 3);
        assert!(invs.iter().all(|e| e.reservations.is_empty()));

        // Position löschen entfernt den Verweis (hier: nach Neuaufbau).
        let id2 = c.list_create("craft", "Set2", None).unwrap();
        c.list_add_item(
            id2,
            bp("a", "A", 1.0, vec![("Hadanite", "gems", 300.0, 500)]),
        )
        .unwrap();
        let r3 = c
            .inventory_add_for_list(inv(500, "Hadanite"), id2, "Hadanite", "gems")
            .unwrap();
        c.inventory_delete(r3.entry.id).unwrap();
        assert!(
            c.inventory_list()
                .unwrap()
                .iter()
                .all(|e| e.reservations.is_empty())
        );
    }

    #[test]
    fn bridge_link_persists_and_cleans_up_on_source_delete() {
        let c = Cache::open_in_memory().unwrap();
        let craft = c.list_create("craft", "FPS-Set", None).unwrap();
        // Brücke: Mining-Liste mit persistenter Verknüpfung auf die Craft-Liste.
        let mining = c.list_create("mining", "FPS-Set", Some(craft)).unwrap();
        let summaries = c.lists_list("mining").unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].source_list_id, Some(craft));
        assert_eq!(summaries[0].source_list_name, Some("FPS-Set".into()));
        // Standalone-Liste: keine Verknüpfung.
        let solo = c.lists_list("craft").unwrap();
        assert_eq!(solo[0].source_list_id, None);
        assert_eq!(solo[0].source_list_name, None);
        // Fehlende Source-Liste fliegt (kein Phantom-Link).
        assert!(c.list_create("mining", "x", Some(999_999)).is_err());
        // Löschen der Source löst den Link (verlinkte Liste bleibt, standalone).
        c.list_delete(craft).unwrap();
        let summaries = c.lists_list("mining").unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, mining);
        assert_eq!(summaries[0].source_list_id, None);
        assert_eq!(summaries[0].source_list_name, None);
    }

    #[test]
    fn inventory_reservation_hint_points_to_source_craft_list() {
        let c = Cache::open_in_memory().unwrap();
        let craft = c.list_create("craft", "FPS-Set", None).unwrap();
        let mining = c
            .list_create("mining", "FPS-Set (Mining)", Some(craft))
            .unwrap();
        c.list_add_item(
            mining,
            bp(
                "a",
                "Hadanite",
                300.0,
                vec![("Hadanite", "gems", 300.0, 500)],
            ),
        )
        .unwrap();
        // Reservierung über die MINING-Liste — der Hinweis zeigt die ERZEUGENDE
        // Crafting-Liste (User-Spec: „immer auf die Craftingliste verweisen").
        let entry = NewInventoryEntry {
            entity_kind: None,
            entity_id: None,
            name: "Hadanite".into(),
            quality: 500,
            location_id: None,
            location_name: "Levski".into(),
            quantity: 300.0,
            price: None,
        };
        let r = c
            .inventory_add_for_list(entry, mining, "Hadanite", "gems")
            .unwrap();
        assert_eq!(r.entry.reservations[0].list_id, Some(craft));
        assert_eq!(r.entry.reservations[0].list_name, Some("FPS-Set".into()));
        // Reservierung an einer standalone (nicht verlinkten) Liste bleibt
        // unverändert bei der Liste selbst (M9-E-Verhalten).
        let solo = c.list_create("mining", "Solo", None).unwrap();
        c.list_add_item(
            solo,
            bp(
                "b",
                "Quantainium",
                10.0,
                vec![("Quantainium", "gems", 10.0, 400)],
            ),
        )
        .unwrap();
        let r2 = c
            .inventory_add_for_list(
                NewInventoryEntry {
                    entity_kind: None,
                    entity_id: None,
                    name: "Quantainium".into(),
                    quality: 400,
                    location_id: None,
                    location_name: "Levski".into(),
                    quantity: 10.0,
                    price: None,
                },
                solo,
                "Quantainium",
                "gems",
            )
            .unwrap();
        assert_eq!(r2.entry.reservations[0].list_id, Some(solo));
        assert_eq!(r2.entry.reservations[0].list_name, Some("Solo".into()));
    }

    #[test]
    fn lists_survive_reopen() {
        let dir = std::env::temp_dir().join(format!(
            "arch-lists-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("cache.sqlite3");
        let db = Cache::open(&path).unwrap();
        let id = db.list_create("craft", "Restart-Test", None).unwrap();
        db.list_add_item(
            id,
            bp("a", "A", 1.0, vec![("Hadanite", "gems", 300.0, 700)]),
        )
        .unwrap();
        db.list_set_done(id, "Hadanite", "gems", Some(150.0))
            .unwrap();
        drop(db);

        let db = Cache::open(&path).unwrap();
        let d = db.list_detail(id).unwrap();
        assert_eq!(d.name, "Restart-Test");
        assert_eq!(d.items.len(), 1);
        assert_eq!(d.items[0].mats[0].quality, 700);
        assert_eq!(d.aggregated[0].done_qty, 150.0);
        std::fs::remove_dir_all(dir).ok();
    }

    // M9-N (t_928d5896): Listen-Abschluss konsumiert Reservierungen.

    #[test]
    fn list_delete_removes_reservations_but_keeps_inventory() {
        let c = Cache::open_in_memory().unwrap();
        let id = c.list_create("craft", "Set", None).unwrap();
        c.list_add_item(
            id,
            bp("a", "A", 1.0, vec![("Hadanite", "gems", 300.0, 500)]),
        )
        .unwrap();
        let r = c
            .inventory_add_for_list(
                NewInventoryEntry {
                    entity_kind: None,
                    entity_id: None,
                    name: "Hadanite".into(),
                    quality: 500,
                    location_id: None,
                    location_name: "Levski".into(),
                    quantity: 300.0,
                    price: None,
                },
                id,
                "Hadanite",
                "gems",
            )
            .unwrap();
        assert_eq!(r.entry.reservations[0].qty, 300.0);

        // P4a-Beleg: list_delete löscht die Reservierungen MIT, aber die
        // Inventar-Positionen bleiben (Menge unangetastet).
        c.list_delete(id).unwrap();
        let invs = c.inventory_list().unwrap();
        assert_eq!(invs.len(), 1);
        assert_eq!(invs[0].quantity, 300.0);
        assert!(invs[0].reservations.is_empty());
    }

    #[test]
    fn list_complete_consumes_reservations_and_completes_list() {
        let c = Cache::open_in_memory().unwrap();
        let id = c.list_create("craft", "Set", None).unwrap();
        c.list_add_item(
            id,
            bp(
                "a",
                "A",
                1.0,
                vec![
                    ("Hadanite", "gems", 300.0, 500),
                    ("Quantum Core", "items", 2.0, 500),
                ],
            ),
        )
        .unwrap();
        // Zwei Reservierungen: Hadanite über inventory_add_for_list (300),
        // Quantum Core über reservation_add auf einer vorhandenen Position.
        let had = c
            .inventory_add_for_list(
                NewInventoryEntry {
                    entity_kind: None,
                    entity_id: None,
                    name: "Hadanite".into(),
                    quality: 500,
                    location_id: None,
                    location_name: "Levski".into(),
                    quantity: 350.0,
                    price: None,
                },
                id,
                "Hadanite",
                "gems",
            )
            .unwrap();
        let qc = c
            .inventory_add(NewInventoryEntry {
                entity_kind: None,
                entity_id: None,
                name: "Quantum Core".into(),
                quality: 500,
                location_id: None,
                location_name: "Levski".into(),
                quantity: 10.0,
                price: None,
            })
            .unwrap();
        let qc_res = c
            .reservation_add(NewReservation {
                inventory_id: qc,
                list_id: Some(id),
                purpose: String::new(),
                qty: 2.0,
                note: String::new(),
            })
            .unwrap();
        assert!(qc_res > 0);

        // Freizweck-Reservierung auf derselben Hadanite-Position MUSS beim
        // Abschluss überleben (nur die Listen-Reservierung wird konsumiert).
        let free = c
            .reservation_add(NewReservation {
                inventory_id: had.entry.id,
                list_id: None,
                purpose: "Verkauf".into(),
                qty: 20.0,
                note: String::new(),
            })
            .unwrap();
        assert!(free > 0);

        // Teilabhak-Stand davor — Abschluss überschreibt ihn auf Soll.
        c.list_set_done(id, "Hadanite", "gems", Some(50.0)).unwrap();

        c.list_complete(id).unwrap();

        let invs = c.inventory_list().unwrap();
        let had_entry = invs.iter().find(|e| e.name == "Hadanite").unwrap();
        let qc_entry = invs.iter().find(|e| e.name == "Quantum Core").unwrap();
        // 350 − 300 (Listen-Reservierung) = 50; die Freizweck-Reservierung
        // (20) bleibt stehen und hat NICHT konsumiert.
        assert_eq!(had_entry.quantity, 50.0);
        assert_eq!(qc_entry.quantity, 8.0);

        let by_inv = |name: &str| {
            invs.iter()
                .find(|e| e.name == name)
                .unwrap()
                .reservations
                .clone()
        };
        // Freizweck bleibt, Listen-Reservierungen sind weg.
        let had_res = by_inv("Hadanite");
        let free_res = had_res.iter().find(|r| r.list_id.is_none()).unwrap();
        assert_eq!(free_res.id, free);
        assert_eq!(free_res.qty, 20.0);
        assert!(had_res.iter().all(|r| r.list_id != Some(id)));
        assert!(by_inv("Quantum Core").is_empty());

        // done = volle Bedarfsmenge je aggregiertem Material.
        let d = c.list_detail(id).unwrap();
        let had_agg = d
            .aggregated
            .iter()
            .find(|m| m.material == "Hadanite")
            .unwrap();
        let qc_agg = d
            .aggregated
            .iter()
            .find(|m| m.material == "Quantum Core")
            .unwrap();
        assert_eq!(had_agg.done_qty, 300.0);
        assert_eq!(had_agg.quantity, 300.0);
        assert_eq!(qc_agg.done_qty, 2.0);
        assert_eq!(qc_agg.quantity, 2.0);
        assert_eq!(had_agg.reserved_qty, 0.0);
    }

    #[test]
    fn list_complete_clamps_negative_and_errors_on_missing() {
        let c = Cache::open_in_memory().unwrap();
        // Nicht existierende Liste fliegt.
        assert!(c.list_complete(999_999).is_err());

        // Defensiv-Klemmung: Positionsmenge < reservierte Menge darf die
        // Menge nicht unter 0 fallen lassen (direkt über die DB erzeugt —
        // die API würde das nie zulassen).
        let id = c.list_create("mining", "Klein", None).unwrap();
        c.list_add_item(id, bp("a", "A", 1.0, vec![("Taranite", "SCU", 5.0, 500)]))
            .unwrap();
        let inv = c
            .inventory_add(NewInventoryEntry {
                entity_kind: None,
                entity_id: None,
                name: "Taranite".into(),
                quality: 500,
                location_id: None,
                location_name: "Levski".into(),
                quantity: 5.0,
                price: None,
            })
            .unwrap();
        c.reservation_add(NewReservation {
            inventory_id: inv,
            list_id: Some(id),
            purpose: String::new(),
            qty: 5.0,
            note: String::new(),
        })
        .unwrap();
        // Position künstlich schrumpfen lassen (Menge 5 → 3) — die API
        // würde das verweigern (err_qty_floor), deshalb direkt auf der DB.
        c.with(|conn| {
            conn.execute(
                "UPDATE inventory SET quantity = 3.0 WHERE id = ?1",
                [inv as i64],
            )?;
            Ok(())
        })
        .unwrap();
        c.list_complete(id).unwrap();
        let invs = c.inventory_list().unwrap();
        assert_eq!(invs[0].quantity, 0.0);
        assert!(invs[0].reservations.is_empty());
    }
}
