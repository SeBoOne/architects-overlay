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

use super::{Cache, CacheError, Result};
use rusqlite::OptionalExtension;

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
}

/// Aggregierte Material-Zeile einer Liste (die Abhak-Ebene).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct AggMat {
    pub material: String,
    pub unit: String,
    /// Summe aller Eintrags-Bedarfe (quantity × copies).
    pub quantity: f64,
    /// MAX der Eintrags-Qualitäten — „höhere Qualität ist immer besser".
    pub min_quality: u32,
    /// Bereits abgehakt (ganz ODER teilweise).
    pub done_qty: f64,
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
                        (SELECT COUNT(*) FROM list_items i WHERE i.list_id = l.id) AS items \
                 FROM lists l WHERE l.kind = ?1 ORDER BY l.created_at DESC, l.id DESC",
            )?;
            let rows = stmt.query_map([kind], |r| {
                Ok(ListSummary {
                    id: r.get::<_, i64>("id")? as u64,
                    kind: r.get("kind")?,
                    name: r.get("name")?,
                    created_at: r.get::<_, i64>("created_at")? as u64,
                    item_count: r.get::<_, i64>("items")? as u64,
                })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
    }

    /// Neue Liste, liefert ihre id.
    pub fn list_create(&self, kind: &str, name: &str) -> Result<u64> {
        validate_kind(kind)?;
        validate_name(name)?;
        self.with(|conn| {
            conn.execute(
                "INSERT INTO lists (kind, name, created_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![kind, name.trim(), super::now_unix() as i64],
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

    /// Liste inkl. Einträge, Snapshots und Abhak-Stand löschen.
    pub fn list_delete(&self, id: u64) -> Result<()> {
        self.with(|conn| {
            conn.execute(
                "DELETE FROM list_item_mats WHERE item_row IN \
                 (SELECT id FROM list_items WHERE list_id = ?1)",
                [id as i64],
            )?;
            conn.execute("DELETE FROM list_items WHERE list_id = ?1", [id as i64])?;
            conn.execute("DELETE FROM list_mat_done WHERE list_id = ?1", [id as i64])?;
            conn.execute("DELETE FROM lists WHERE id = ?1", [id as i64])?;
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
        self.with(move |conn| {
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
        })
    }

    /// Eintrag (inkl. Snapshot) aus der Liste nehmen.
    pub fn list_remove_item(&self, item_row: u64) -> Result<()> {
        let changed = self.with(|conn| {
            conn.execute(
                "DELETE FROM list_item_mats WHERE item_row = ?1",
                [item_row as i64],
            )?;
            conn.execute("DELETE FROM list_items WHERE id = ?1", [item_row as i64])
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
            conn.execute(
                "UPDATE list_items SET copies = ?1 WHERE id = ?2",
                rusqlite::params![copies, item_row as i64],
            )
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
            Ok(())
        })
    }

    /// Volle Liste: Kopf + Einträge (mit Snapshots) + Material-Aggregation.
    pub fn list_detail(&self, id: u64) -> Result<ListDetail> {
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
        let (items, mut done_map) = self.with(|conn| {
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
            Ok((items, done_map))
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
            .map(|((material, unit), (quantity, q))| AggMat {
                done_qty: done_map
                    .remove(&(material.clone(), unit.clone()))
                    .unwrap_or(0.0),
                min_quality: q,
                quantity,
                material,
                unit,
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
    fn crud_roundtrip() {
        let c = Cache::open_in_memory().unwrap();
        assert!(c.lists_list("craft").unwrap().is_empty());
        assert!(c.list_create("nope", "x").is_err());
        assert!(c.list_create("craft", "  ").is_err());

        let id = c.list_create("craft", "FPS-Set").unwrap();
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
        let id = c.list_create("craft", "Set").unwrap();
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
        let id = c.list_create("craft", "Set").unwrap();
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
        let id2 = c.list_create("craft", "Set").unwrap();
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
        let id = c.list_create("craft", "Set").unwrap();
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
        let id = db.list_create("craft", "Restart-Test").unwrap();
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
}
