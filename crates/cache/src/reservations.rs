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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::NewInventoryEntry;

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
