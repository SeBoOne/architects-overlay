//! # cache::reservations
//!
//! Inventar-first-Reservierung (M9-F, t_bb97da62, Welle 2): Reservierungen
//! sind ein eigenes Primitiv AUS dem Inventar — jede Reservierung zeigt auf
//! eine Inventar-Position (`inventory_id`), trägt eine Menge (`qty`,
//! partiell erlaubt) und einen Zweck: entweder eine Crafting-/Mining-Liste
//! (`list_id`) oder einen freien Zweck-Text (`purpose`: „Projekt X“,
//! „Verkauf“, ...). Mehrere Reservierungen je Position sind erlaubt; die
//! SUMME darf die Positionsmenge nicht übersteigen („gesamt / reserviert /
//! frei“ im Inventar).
//!
//! Modell-Umkehr gegenüber v14 (`list_reservations`): dort „erwies“ die
//! Position die Reservierung 1:1 (eine Position = eine Liste). Bestehende
//! Zeilen wurden bei der v16-Migration übernommen (qty = Positionsmenge,
//! Material → note). Lokal-first wie das Inventar: KEIN Sync, KEIN API.

use super::{Cache, CacheError, Result};
use rusqlite::OptionalExtension;

/// Eine Reservierung aus dem Inventar.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Reservation {
    #[serde(serialize_with = "provider::serialize_id_as_string")]
    pub id: u64,
    #[serde(serialize_with = "ser_inv_id_as_string")]
    pub inventory_id: u64,
    /// Liste (Listenzweck) — `None` = freier Zweck.
    #[serde(serialize_with = "ser_opt_list_id_as_string")]
    pub list_id: Option<u64>,
    /// Aufgelöster Listenname (beim Lesen).
    pub list_name: Option<String>,
    /// Freier Zweck-Text (mit `list_id` kombinierbar, z. B. Notiz).
    pub purpose: String,
    pub qty: f64,
    pub note: String,
    pub created_at: u64,
}

/// Payload für `reservation_add` — `inventory_id` kommt als STRING (JS-
/// Präzisionskonvention, t_925a577a), bare numbers akzeptiert.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct NewReservation {
    #[serde(deserialize_with = "provider::deserialize_id_flexible")]
    pub inventory_id: u64,
    #[serde(deserialize_with = "provider::deserialize_opt_id_flexible")]
    pub list_id: Option<u64>,
    pub purpose: String,
    pub qty: f64,
    pub note: String,
}

fn ser_inv_id_as_string<S: serde::Serializer>(
    v: &u64,
    s: S,
) -> std::result::Result<S::Ok, S::Error> {
    s.serialize_str(&v.to_string())
}

fn ser_opt_list_id_as_string<S: serde::Serializer>(
    v: &Option<u64>,
    s: S,
) -> std::result::Result<S::Ok, S::Error> {
    match v {
        Some(id) => s.serialize_str(&id.to_string()),
        None => s.serialize_none(),
    }
}

fn row_to_reservation(r: &rusqlite::Row<'_>) -> rusqlite::Result<Reservation> {
    let list_id: Option<i64> = r.get("list_id")?;
    Ok(Reservation {
        id: r.get::<_, i64>("id")? as u64,
        inventory_id: r.get::<_, i64>("inventory_id")? as u64,
        list_id: list_id.map(|v| v as u64),
        list_name: r.get("list_name")?,
        purpose: r.get("purpose")?,
        qty: r.get("qty")?,
        note: r.get("note")?,
        created_at: r.get::<_, i64>("created_at")? as u64,
    })
}

const RESERVATION_SELECT: &str = "SELECT res.id, res.inventory_id, \
     COALESCE(s.id, l.id) AS list_id, COALESCE(s.name, l.name) AS list_name, \
     res.purpose, res.qty, res.note, res.created_at \
     FROM reservations res \
     LEFT JOIN lists l ON l.id = res.list_id \
     LEFT JOIN lists s ON s.id = l.source_list_id";

impl Cache {
    /// Freie Menge einer Position = quantity − SUM(reserviert). `Err`,
    /// wenn die Position nicht existiert.
    pub fn inventory_free_qty(&self, inventory_id: u64) -> Result<f64> {
        let row: Option<(f64, Option<f64>)> = self.with(|conn| {
            conn.query_row(
                "SELECT i.quantity, (SELECT SUM(res.qty) FROM reservations res \
                 WHERE res.inventory_id = i.id) \
                 FROM inventory i WHERE i.id = ?1",
                [inventory_id as i64],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
        })?;
        match row {
            None => Err(CacheError::Query(i18n_err::err_keyp(
                "inventory.err_position_missing",
                serde_json::json!({ "id": inventory_id }),
            ))),
            Some((qty, reserved)) => Ok((qty - reserved.unwrap_or(0.0)).max(0.0)),
        }
    }

    /// Alle Reservierungen (neueste zuerst) — für Tests/Analyse.
    pub fn reservations_list(&self) -> Result<Vec<Reservation>> {
        self.with(|conn| {
            let mut stmt = conn.prepare(&format!(
                "{RESERVATION_SELECT} ORDER BY res.created_at DESC, res.id DESC"
            ))?;
            let rows = stmt.query_map([], row_to_reservation)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
    }

    /// Reservierung anlegen. Prüft: Position existiert, Zweck vorhanden
    /// (`purpose` ODER `list_id`), Menge > 0 und ≤ freie Menge (partiell
    /// erlaubt). Liefert die id.
    pub fn reservation_add(&self, new: NewReservation) -> Result<u64> {
        if !(new.qty.is_finite() && new.qty > 0.0) {
            return Err(CacheError::Query(i18n_err::err_key("inventory.err_qty")));
        }
        if new.purpose.trim().is_empty() && new.list_id.is_none() {
            return Err(CacheError::Query(i18n_err::err_key(
                "reservations.err_purpose",
            )));
        }
        let list_id = new.list_id;
        let purpose = new.purpose.trim().to_string();
        let note = new.note.trim().to_string();
        let qty = new.qty;
        let inv = new.inventory_id;
        // Existenz + freie Menge separat abfragen (die `with`-Closure
        // spricht rusqlite; semantische Fehler entstehen hier außen).
        let (quantity, reserved, list_ok): (f64, f64, bool) = self.with(|conn| {
            let quantity: f64 = conn
                .query_row(
                    "SELECT quantity FROM inventory WHERE id = ?1",
                    [inv as i64],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
            let reserved: f64 = conn.query_row(
                "SELECT COALESCE(SUM(qty), 0) FROM reservations WHERE inventory_id = ?1",
                [inv as i64],
                |r| r.get(0),
            )?;
            let list_ok = match list_id {
                Some(lid) => {
                    let exists: i64 = conn.query_row(
                        "SELECT COUNT(*) FROM lists WHERE id = ?1",
                        [lid as i64],
                        |r| r.get(0),
                    )?;
                    exists > 0
                }
                None => true,
            };
            Ok((quantity, reserved, list_ok))
        })?;
        if !list_ok {
            return Err(CacheError::Query(i18n_err::err_keyp(
                "lists.err_missing",
                serde_json::json!({ "id": list_id }),
            )));
        }
        if qty > (quantity - reserved).max(0.0) + 1e-9 {
            return Err(CacheError::Query(i18n_err::err_keyp(
                "reservations.err_no_free",
                serde_json::json!({ "free": (quantity - reserved).max(0.0) }),
            )));
        }
        self.with(move |conn| {
            conn.execute(
                "INSERT INTO reservations (inventory_id, list_id, purpose, qty, note, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    inv as i64,
                    list_id.map(|v| v as i64),
                    purpose,
                    qty,
                    note,
                    super::now_unix() as i64
                ],
            )?;
            Ok(conn.last_insert_rowid() as u64)
        })
    }

    /// Reservierung aufheben. `Err` wenn die id nicht existiert.
    pub fn reservation_delete(&self, id: u64) -> Result<()> {
        let changed = self.with(move |conn| {
            conn.execute("DELETE FROM reservations WHERE id = ?1", [id as i64])
        })?;
        if changed == 0 {
            return Err(CacheError::Query(i18n_err::err_keyp(
                "reservations.err_missing",
                serde_json::json!({ "id": id }),
            )));
        }
        Ok(())
    }

    /// Reservierungen einer Inventar-Position (für die Zeilen-Anzeige „wofür“).
    pub fn reservations_for_inventory(&self, inventory_id: u64) -> Result<Vec<Reservation>> {
        self.with(|conn| {
            let mut stmt = conn.prepare(&format!(
                "{RESERVATION_SELECT} WHERE res.inventory_id = ?1 \
                 ORDER BY res.created_at ASC, res.id ASC"
            ))?;
            let rows = stmt.query_map([inventory_id as i64], row_to_reservation)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
    }

    /// Reservierungen einer Liste (Zweck auflösen — Listenanzeige/Tests).
    pub fn reservations_for_list(&self, list_id: u64) -> Result<Vec<Reservation>> {
        self.with(|conn| {
            let mut stmt = conn.prepare(&format!(
                "{RESERVATION_SELECT} WHERE res.list_id = ?1 \
                 ORDER BY res.created_at ASC, res.id ASC"
            ))?;
            let rows = stmt.query_map([list_id as i64], row_to_reservation)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
    }

    /// Reservierte Menge + mengengewichtete Ø-Qualität, die eine Liste über
    /// ihre Reservierungen gegen ein (Material-)Muster gebunden hat. Gematcht
    /// wird über den Inventar-Namen (die Reservierung ist quellenbezogen —
    /// die Quelle trägt Material + Qualität). Leerer Vektor ⇒ keine Treffer.
    pub fn list_reservation_quality(
        &self,
        list_id: u64,
        material: &str,
    ) -> Result<(f64, Option<u32>)> {
        self.with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT res.qty, i.quality FROM reservations res \
                 JOIN inventory i ON i.id = res.inventory_id \
                 WHERE res.list_id = ?1 AND LOWER(i.name) = LOWER(?2)",
            )?;
            let rows = stmt.query_map(rusqlite::params![list_id as i64, material.trim()], |r| {
                let q: f64 = r.get(0)?;
                let qual: i64 = r.get(1)?;
                Ok((q, qual))
            })?;
            let mut sum = 0.0f64;
            let mut wq = 0.0f64;
            for r in rows {
                let (q, qual) = r?;
                sum += q;
                wq += q * qual as f64;
            }
            let avg = if sum > 0.0 {
                Some((wq / sum).round() as u32)
            } else {
                None
            };
            Ok((sum, avg))
        })
    }
}

/// M9-J (t_14cc5d4e) — Ergebnis-Zeile je Listen-Material von
/// `list_reserve_all`.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ReserveAllMat {
    pub material: String,
    pub unit: String,
    /// Bedarf = Sollmenge − abgehakt (done).
    pub required: f64,
    /// Diesem Lauf neu reservierte Menge.
    pub reserved: f64,
    /// Mengengewichtete Ø-Qualität der neuen Mischung.
    pub avg_quality: Option<u32>,
    /// Bedarf vollständig abgedeckt.
    pub fully_covered: bool,
}

/// Ergebnis von `list_reserve_all`.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ReserveAllResult {
    pub list_id: u64,
    pub mats: Vec<ReserveAllMat>,
    /// Anzahl neu angelegter Reservierungs-Zeilen.
    pub created: u64,
    /// Anzahl ersetzter (gelöschter) Alt-Reservierungen der Liste.
    pub replaced: u64,
}

const EPS: f64 = 1e-9;

/// Pure Greedy-Auswahl best→worst (M9-J): reserviert aus den Kandidaten
/// `(freie Menge, Qualität)` bis `required` abgedeckt ist. Die mengen-
/// gewichtete Ø-Qualität der Mischung darf `min_quality` NIE unterschreiten:
/// Zeilen unterhalb der Anforderung werden nur in dem Umfang aufgenommen,
/// der die Ø-Qualität hält (`Ø ≥ Anforderung`), sonst übersprungen. Summe
/// der Picks ≤ required (Cap). Per Material unabhängig — ein Material
/// blockiert kein anderes.
///
/// Rückgabe: Picks `(Zeilen-Index, Menge)` in best→worst-Reihenfolge
/// (Index in die EINGABE-Reihenfolge, bei Gleichstand stabil).
pub fn greedy_reserve_mix(
    required: f64,
    min_quality: u32,
    rows: &[(f64, u32)],
) -> Vec<(usize, f64)> {
    let mut picks: Vec<(usize, f64)> = Vec::new();
    if !(required.is_finite() && required > EPS) {
        return picks;
    }
    // best → worst; bei Gleichstand stabil (Reihenfolge der Eingabe).
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by(|&a, &b| rows[b].1.cmp(&rows[a].1));
    let mut acc_qty = 0.0f64;
    let mut acc_wq = 0.0f64;
    for &i in &order {
        let (free, quality) = rows[i];
        if acc_qty >= required - EPS {
            break;
        }
        if !(free.is_finite() && free > EPS) {
            continue;
        }
        let want = required - acc_qty;
        let mut take = free.min(want);
        if (quality as f64) < min_quality as f64 && acc_qty + take > EPS {
            // Ø-Check: (acc_wq + take·q) / (acc_qty + take) ≥ min_quality.
            // Maximal zulässige Zusatzmenge aus dieser (schlechteren) Quelle:
            let denom = min_quality as f64 - quality as f64;
            let x_max = (acc_wq - min_quality as f64 * acc_qty) / denom;
            take = take.min(x_max.max(0.0));
            if take <= EPS {
                // Diese Quelle (und alle folgenden — nur schlechter) darf
                // die Ø-Qualität nicht halten → abbrechen.
                break;
            }
        }
        acc_qty += take;
        acc_wq += take * quality as f64;
        picks.push((i, take));
    }
    picks
}

fn row_free(conn: &rusqlite::Connection, inventory_id: i64) -> rusqlite::Result<f64> {
    conn.query_row(
        "SELECT i.quantity - COALESCE((SELECT SUM(res.qty) FROM reservations res \
         WHERE res.inventory_id = i.id), 0) FROM inventory i WHERE i.id = ?1",
        [inventory_id],
        |r| r.get::<_, f64>(0),
    )
}

impl Cache {
    /// M9-J (t_14cc5d4e): „Alles reservieren“ für eine Liste. Reserviert mit
    /// einem Lauf für ALLE Materialien der Liste aus dem freien Inventar
    /// (Greedy best→worst, Ø-Qualitätsregel je Material, Cap Summe ≤ Bedarf).
    ///
    /// Idempotenter Re-Click: komplette Neuberechnung — alle bestehenden
    /// Reservierungen dieser Liste (`list_id`) werden gelöscht und neu
    /// gesetzt; manuell nachgeänder Mengen werden überschrieben, zwischen-
    /// zeitlich besseres/mehr Material wird aufgenommen. Materialien sind
    /// unabhängig voneinander (ein Material blockiert kein anderes).
    pub fn list_reserve_all(&self, list_id: u64) -> Result<ReserveAllResult> {
        // Liste existiert?
        let exists: Option<i64> = self.with(|conn| {
            conn.query_row("SELECT 1 FROM lists WHERE id = ?1", [list_id as i64], |r| {
                r.get(0)
            })
            .optional()
        })?;
        if exists.is_none() {
            return Err(CacheError::Query(i18n_err::err_keyp(
                "lists.err_missing",
                serde_json::json!({ "id": list_id }),
            )));
        }

        // Bedarf je (Material, Unit) + Anforderung (MAX) — dieselbe Aggrega-
        // tion wie `list_detail`, inkl. Abzug des Abhak-Stands (done).
        let mats: Vec<(String, String, f64, u32)> = self.with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT mim.material, mim.unit, SUM(mim.quantity * li.copies), MAX(mim.quality) \
                 FROM list_items li JOIN list_item_mats mim ON mim.item_row = li.id \
                 WHERE li.list_id = ?1 \
                 GROUP BY mim.material, mim.unit ORDER BY LOWER(mim.material) ASC",
            )?;
            let rows = stmt.query_map([list_id as i64], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, f64>(2)?,
                    r.get::<_, i64>(3)? as u32,
                ))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })?;

        // done-Stand lesen (Bedarf-Abzug). Die freien Kandidaten werden erst
        // NACH dem Löschen der Alt-Reservierungen gelesen (unten in derselben
        // Sperre) — sonst würden die eigenen Alt-Reservierungen den Bestand
        // fälschlich blockieren.
        let done_map: std::collections::HashMap<(String, String), f64> = self.with(|conn| {
            let mut stmt =
                conn.prepare("SELECT list_id, material, unit, done_qty FROM list_mat_done")?;
            let mut rows = stmt.query([])?;
            let mut map = std::collections::HashMap::new();
            while let Some(r) = rows.next()? {
                let lid: i64 = r.get(0)?;
                if lid == list_id as i64 {
                    map.insert(
                        (r.get::<_, String>(1)?, r.get::<_, String>(2)?),
                        r.get::<_, f64>(3)?,
                    );
                }
            }
            Ok(map)
        })?;

        // Alt-Reservierungen der Liste zuerst lesen (Zähler) und löschen,
        // dann pro Material Greedy — alles in EINER Sperre (atomar).
        let mut result = ReserveAllResult {
            list_id,
            mats: Vec::new(),
            created: 0,
            replaced: 0,
        };
        self.with(move |conn| -> rusqlite::Result<ReserveAllResult> {
            result.replaced = conn.query_row(
                "SELECT COUNT(*) FROM reservations WHERE list_id = ?1",
                [list_id as i64],
                |r| r.get::<_, i64>(0),
            )? as u64;
            conn.execute(
                "DELETE FROM reservations WHERE list_id = ?1",
                [list_id as i64],
            )?;
            // Freier Bestand NACH dem Löschen der Alt-Reservierungen (die
            // Liste hat ihren Bestand damit wieder freigegeben); Reservierun-
            // gen FREMDER Zwecke zählen weiter ab.
            let mut inventory: Vec<(i64, String, f64, i64)> = Vec::new();
            {
                let mut stmt = conn.prepare(
                    "SELECT i.id, i.name, i.quantity, i.quality, \
                            COALESCE((SELECT SUM(res.qty) FROM reservations res \
                                      WHERE res.inventory_id = i.id), 0) AS reserved \
                     FROM inventory i WHERE i.quantity > 0 ORDER BY i.id ASC",
                )?;
                let rows = stmt.query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, f64>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, f64>(4)?,
                    ))
                })?;
                for r in rows {
                    let (id, name, qty, quality, reserved) = r?;
                    let free = (qty - reserved).max(0.0);
                    if free > EPS {
                        inventory.push((id, name.to_lowercase(), free, quality));
                    }
                }
            }
            for (material, unit, quantity, min_quality) in &mats {
                let done = done_map
                    .get(&(material.clone(), unit.clone()))
                    .copied()
                    .unwrap_or(0.0);
                let required = (quantity - done).max(0.0);
                // Kandidaten: Inventar-Positionen mit diesem Material-Namen
                // (Match wie in list_detail/list_reservation_quality — groß/
                // klein egal). Freie Menge JETZT — die gelöschten Alt-Reser-
                // vierungen der Liste haben die Position ggf. wieder frei-
                // gegeben; Reservierungen FREMDER Zwecke zählen weiter ab.
                let mut cand_rows: Vec<(f64, u32)> = Vec::new();
                let mut cand_ids: Vec<i64> = Vec::new();
                for &(id, ref name, _, quality) in inventory.iter() {
                    if name == &material.to_lowercase() {
                        cand_ids.push(id);
                        cand_rows.push((row_free(conn, id).unwrap_or(0.0), quality as u32));
                    }
                }
                let picks = greedy_reserve_mix(required, *min_quality, &cand_rows);
                for &(row, qty) in &picks {
                    conn.execute(
                        "INSERT INTO reservations \
                         (inventory_id, list_id, purpose, qty, note, created_at) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        rusqlite::params![
                            cand_ids[row],
                            list_id as i64,
                            "", // Zweck = die Liste
                            qty,
                            material.trim(),
                            super::now_unix() as i64
                        ],
                    )?;
                    result.created += 1;
                }
                let reserved: f64 = picks.iter().map(|&(_, q)| q).sum();
                let wq: f64 = picks
                    .iter()
                    .map(|&(row, q)| q * cand_rows[row].1 as f64)
                    .sum();
                let avg = if reserved > EPS {
                    Some((wq / reserved).round() as u32)
                } else {
                    None
                };
                result.mats.push(ReserveAllMat {
                    material: material.clone(),
                    unit: unit.clone(),
                    required,
                    reserved,
                    avg_quality: avg,
                    fully_covered: required > EPS && reserved >= required - EPS,
                });
            }
            Ok(result)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::NewInventoryEntry;
    use crate::lists::{NewListItem, NewListItemMat};

    fn inv(name: &str, quality: u32, qty: f64) -> NewInventoryEntry {
        NewInventoryEntry {
            entity_kind: Some("commodity".into()),
            entity_id: Some(7),
            name: name.into(),
            quality,
            location_id: None,
            location_name: "Depot".into(),
            quantity: qty,
            price: None,
        }
    }

    // ---------- M9-J (t_14cc5d4e): list_reserve_all / greedy_reserve_mix ----------

    #[test]
    fn greedy_best_to_worst_with_cap() {
        // best→worst, Cap auf Bedarf
        let picks = greedy_reserve_mix(10.0, 0, &[(4.0, 900), (10.0, 500), (10.0, 300)]);
        assert_eq!(picks.len(), 2);
        assert_eq!(picks[0], (0, 4.0));
        assert_eq!(picks[1], (1, 6.0));
        let total: f64 = picks.iter().map(|&(_, q)| q).sum();
        assert!(total <= 10.0 + 1e-9);
        // Bedarf 0 → nichts
        assert!(greedy_reserve_mix(0.0, 500, &[(10.0, 900)]).is_empty());
        // Partielle Aufnahme mitten in der Quelle
        let picks = greedy_reserve_mix(5.0, 0, &[(2.5, 800), (9.0, 600)]);
        assert_eq!(picks.len(), 2);
        assert_eq!(picks[1], (1, 2.5));
    }

    #[test]
    fn greedy_avg_quality_blocks_low_quality_material() {
        // Nur schlechtere Qualität vorhanden → Material bleibt UNRESERVIERT
        assert!(greedy_reserve_mix(10.0, 600, &[(10.0, 500)]).is_empty());
        // Mischung, die Ø hält: 5×900 + 5×500 → Ø 700 ≥ 600 → voll
        let picks = greedy_reserve_mix(10.0, 600, &[(5.0, 900), (10.0, 500)]);
        let total: f64 = picks.iter().map(|&(_, q)| q).sum();
        assert!((total - 10.0).abs() < 1e-9);
        // Ø auf die zulässige Mischung BEGRENZT: 2×900 + x×500 mit
        // (1800 + 500x)/(2+x) ≥ 600 → x ≤ 6 → nur 8 statt Bedarf 10
        let picks = greedy_reserve_mix(10.0, 600, &[(2.0, 900), (20.0, 500)]);
        let total: f64 = picks.iter().map(|&(_, q)| q).sum();
        assert!((total - 8.0).abs() < 1e-9, "got {total}");
        // Ø-Check exakt: gewichtet
        let wsum: f64 = picks
            .iter()
            .map(|&(i, q)| q * if i == 0 { 900.0 } else { 500.0 })
            .sum();
        assert!((wsum / total - 600.0).abs() < 1e-9);
        // Blockiert komplett: Best-Qualität == Anforderung, danach nichts
        // mehr zulässig (x_max = 0) → abbrechen, nur die 3 reserviert.
        let picks = greedy_reserve_mix(5.0, 700, &[(3.0, 700), (10.0, 600)]);
        let total: f64 = picks.iter().map(|&(_, q)| q).sum();
        assert!((total - 3.0).abs() < 1e-9, "got {total}");
        // Best-Qualität UNTER der Anforderung → gar nichts
        assert!(greedy_reserve_mix(5.0, 700, &[(3.0, 650), (10.0, 400)]).is_empty());
    }

    #[test]
    fn list_reserve_all_greedy_avg_and_idempotent_reclick() {
        let c = Cache::open_in_memory().unwrap();
        let l = c.list_create("craft", "Rasierer-Set", None).unwrap();
        c.list_add_item(
            l,
            NewListItem {
                item_id: "bp1".into(),
                item_name: "Wall".into(),
                craft_time: 0,
                copies: 1.0,
                mats: vec![
                    NewListItemMat {
                        material: "Gold".into(),
                        ore_id: None,
                        unit: "SCU".into(),
                        quantity: 10.0,
                        quality: 600,
                    },
                    NewListItemMat {
                        material: "Silver".into(),
                        ore_id: None,
                        unit: "SCU".into(),
                        quantity: 5.0,
                        quality: 400,
                    },
                ],
            },
        )
        .unwrap();
        // Inventar: Gold in 3 Qualitäten, Silver nur zu schlecht.
        let g900 = c.inventory_add(inv("Gold", 900, 4.0)).unwrap();
        let g500 = c.inventory_add(inv("Gold", 500, 10.0)).unwrap();
        let g300 = c.inventory_add(inv("Gold", 300, 10.0)).unwrap();
        let s200 = c.inventory_add(inv("Silver", 200, 20.0)).unwrap();
        let _ = (g900, g500, g300, s200);

        let r = c.list_reserve_all(l).unwrap();
        assert_eq!(r.mats.len(), 2);
        // Gold: 4×Q900 + 6×Q500 → Ø 660 ≥ 600, voll abgedeckt.
        let gold = r.mats.iter().find(|m| m.material == "Gold").unwrap();
        assert_eq!(gold.reserved, 10.0);
        assert_eq!(gold.avg_quality, Some(660));
        assert!(gold.fully_covered);
        // Silver: nur Q200 < Anforderung 400 → unreserviert.
        let silver = r.mats.iter().find(|m| m.material == "Silver").unwrap();
        assert_eq!(silver.reserved, 0.0);
        assert_eq!(silver.avg_quality, None);
        assert!(!silver.fully_covered);
        assert_eq!(r.created, 2);
        // Zeilen: Gold900 = 4, Gold500 = 6.
        let res = c.reservations_for_list(l).unwrap();
        assert_eq!(res.len(), 2);
        assert!(res.iter().all(|x| x.list_id == Some(l)));

        // Idempotenter Re-Click: gleiche Mischung, Alt-Reservierungen ersetzt.
        let r2 = c.list_reserve_all(l).unwrap();
        assert_eq!(r2.created, 2);
        assert_eq!(r2.replaced, 2);
        let res2 = c.reservations_for_list(l).unwrap();
        assert_eq!(res2.len(), 2);
        let (sum, avg) = c.list_reservation_quality(l, "Gold").unwrap();
        assert_eq!(sum, 10.0);
        assert_eq!(avg, Some(660));

        // Inventar-Zuwachs (besseres Material) → Re-Click nimmt es auf.
        let g1000 = c.inventory_add(inv("Gold", 1000, 10.0)).unwrap();
        let _ = g1000;
        let r3 = c.list_reserve_all(l).unwrap();
        let gold3 = r3.mats.iter().find(|m| m.material == "Gold").unwrap();
        assert_eq!(gold3.reserved, 10.0);
        assert_eq!(gold3.avg_quality, Some(1000));
        assert_eq!(r3.replaced, 2);

        // Manuell geänderte Mengen werden von der Neuberechnung ÜBERSCHRIEBEN.
        let manual = c
            .reservation_add(NewReservation {
                inventory_id: c
                    .inventory_list()
                    .unwrap()
                    .iter()
                    .find(|e| e.name == "Gold" && e.quality == 300)
                    .unwrap()
                    .id,
                list_id: Some(l),
                purpose: String::new(),
                qty: 9.0,
                note: "Gold".into(),
            })
            .unwrap();
        let _ = manual;
        let r4 = c.list_reserve_all(l).unwrap();
        let gold4 = r4.mats.iter().find(|m| m.material == "Gold").unwrap();
        assert_eq!(gold4.reserved, 10.0);
        assert_eq!(gold4.avg_quality, Some(1000));
        // Q300-Position wieder frei (manuelle Reservierung weg).
        let free300 = c
            .inventory_free_qty(
                c.inventory_list()
                    .unwrap()
                    .iter()
                    .find(|e| e.name == "Gold" && e.quality == 300)
                    .unwrap()
                    .id,
            )
            .unwrap();
        assert_eq!(free300, 10.0);

        // Cap: Abhaken reduziert den Bedarf; Summe ≤ Listenbedarf.
        c.list_set_done(l, "Gold", "SCU", Some(4.0)).unwrap();
        let r5 = c.list_reserve_all(l).unwrap();
        let gold5 = r5.mats.iter().find(|m| m.material == "Gold").unwrap();
        assert_eq!(gold5.required, 6.0);
        assert_eq!(gold5.reserved, 6.0);
        assert_eq!(gold5.avg_quality, Some(1000));
        // Liste existiert nicht → Err.
        assert!(c.list_reserve_all(999_999).is_err());
    }

    #[test]
    fn list_reserve_all_respects_foreign_purpose_reservations() {
        let c = Cache::open_in_memory().unwrap();
        let l = c.list_create("craft", "L1", None).unwrap();
        c.list_add_item(
            l,
            NewListItem {
                item_id: "bp1".into(),
                item_name: "Wall".into(),
                craft_time: 0,
                copies: 1.0,
                mats: vec![NewListItemMat {
                    material: "Gold".into(),
                    ore_id: None,
                    unit: "SCU".into(),
                    quantity: 10.0,
                    quality: 0,
                }],
            },
        )
        .unwrap();
        let g = c.inventory_add(inv("Gold", 800, 10.0)).unwrap();
        // Fremder Zweck blockiert 7 → der Liste bleiben nur 3.
        c.reservation_add(NewReservation {
            inventory_id: g,
            list_id: None,
            purpose: "Verkauf".into(),
            qty: 7.0,
            note: String::new(),
        })
        .unwrap();
        let r = c.list_reserve_all(l).unwrap();
        assert_eq!(r.mats[0].required, 10.0);
        assert_eq!(r.mats[0].reserved, 3.0);
        assert!(!r.mats[0].fully_covered);
        // Re-Click: gleiche Mischung (kein Selbst-Überbuchen).
        let r2 = c.list_reserve_all(l).unwrap();
        assert_eq!(r2.mats[0].reserved, 3.0);
        assert_eq!(r2.replaced, 1);
    }

    #[test]
    fn free_qty_add_delete_and_limits() {
        let c = Cache::open_in_memory().unwrap();
        let id = c.inventory_add(inv("Gold", 900, 10.0)).unwrap();
        assert_eq!(c.inventory_free_qty(id).unwrap(), 10.0);

        // Kein Zweck → Err
        assert!(
            c.reservation_add(NewReservation {
                inventory_id: id,
                list_id: None,
                purpose: "  ".into(),
                qty: 2.0,
                note: String::new()
            })
            .is_err()
        );

        // Freier Zweck, partiell
        let r1 = c
            .reservation_add(NewReservation {
                inventory_id: id,
                list_id: None,
                purpose: "Verkauf".into(),
                qty: 2.5,
                note: String::new(),
            })
            .unwrap();
        assert_eq!(c.inventory_free_qty(id).unwrap(), 7.5);

        // Mehrere je Position (Listen-Zweck)
        let l = c.list_create("craft", "FPS-Set", None).unwrap();
        let r2 = c
            .reservation_add(NewReservation {
                inventory_id: id,
                list_id: Some(l),
                purpose: String::new(),
                qty: 3.0,
                note: "Gold".into(),
            })
            .unwrap();
        assert_eq!(c.inventory_free_qty(id).unwrap(), 4.5);

        // Überbuchung fliegt
        assert!(
            c.reservation_add(NewReservation {
                inventory_id: id,
                list_id: None,
                purpose: "x".into(),
                qty: 5.0,
                note: String::new()
            })
            .is_err()
        );

        // Gespeicherte Zeilen tragen Zweck + Listenname
        let all = c.reservations_for_inventory(id).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].purpose, "Verkauf");
        assert_eq!(all[1].list_name.as_deref(), Some("FPS-Set"));

        // Position löschen räumt die Reservierungen mit ab
        c.inventory_delete(id).unwrap();
        assert!(c.reservations_list().unwrap().is_empty());
        // Delete einer nicht existierenden Reservierung
        assert!(c.reservation_delete(r1).is_err());
        assert!(c.reservation_delete(r2).is_err());
        assert!(c.reservation_delete(999_999).is_err());
    }

    #[test]
    fn list_purpose_requires_existing_list() {
        let c = Cache::open_in_memory().unwrap();
        let id = c.inventory_add(inv("Gold", 900, 10.0)).unwrap();
        assert!(
            c.reservation_add(NewReservation {
                inventory_id: id,
                list_id: Some(12345),
                purpose: String::new(),
                qty: 1.0,
                note: String::new()
            })
            .is_err()
        );
    }

    /// v16-Migration: eine bestehende v14-Reservierung (Position → Liste,
    /// volle Menge) landet im neuen Primitiv — Listenname aufgelöst, Menge
    /// = Positionsmenge, Material in der note.
    #[test]
    fn v14_reservations_migrate_with_full_quantity() {
        let dir = std::env::temp_dir().join(format!(
            "arch-resv-mig-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("c.sqlite3");
        // v14-Stand aufbauen: alte Tabelle + Zeile (die init-Migration existiert
        // im geladenen Build schon — also Legacy-Zeile NACH init einfügen und
        // dann Reopen, damit die Migration die Zeile übernimmt).
        {
            let c = Cache::open(&path).unwrap();
            let inv2 = c.inventory_add(inv("Hadanite", 620, 3.0)).unwrap();
            let l2 = c.list_create("craft", "Schiffs-Set", None).unwrap();
            c.with(|conn| {
                conn.execute(
                    "CREATE TABLE IF NOT EXISTS list_reservations (\
                     inventory_id INTEGER PRIMARY KEY, list_id INTEGER NOT NULL, \
                     material TEXT NOT NULL, unit TEXT NOT NULL)",
                    [],
                )?;
                conn.execute(
                    "INSERT INTO list_reservations (inventory_id, list_id, material, unit) \
                     VALUES (?1, ?2, 'Hadanite', 'SCU')",
                    rusqlite::params![inv2 as i64, l2 as i64],
                )
            })
            .unwrap();
            drop(c);
            let c3 = Cache::open(&path).unwrap();
            let res = c3.reservations_for_inventory(inv2).unwrap();
            assert_eq!(res.len(), 1, "v14-Zeile übernommen");
            assert_eq!(res[0].list_id, Some(l2));
            assert_eq!(res[0].list_name.as_deref(), Some("Schiffs-Set"));
            assert_eq!(res[0].qty, 3.0);
            assert_eq!(res[0].note, "Hadanite");
            assert_eq!(c3.inventory_free_qty(inv2).unwrap(), 0.0);
            // Mengengewichtete Ø-Qualität der Liste
            let (sum, avg) = c3.list_reservation_quality(l2, "Hadanite").unwrap();
            assert_eq!(sum, 3.0);
            assert_eq!(avg, Some(620));
            // Reopen-Idempotenz: Migration läuft erneut, KEINE Duplikate.
            drop(c3);
            let c4 = Cache::open(&path).unwrap();
            assert_eq!(c4.reservations_for_inventory(inv2).unwrap().len(), 1);
        }
        std::fs::remove_dir_all(dir).ok();
    }
}
