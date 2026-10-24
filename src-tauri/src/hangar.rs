//! Hangar-Modus (Karte t_317f19c5): private Schiffe/Fahrzeuge/Module.
//!
//! User-owned Datensätze in der Cache-DB (Tabelle `hangar`, Schema v4) —
//! Pledge- vs. Ingame-Käufe, Versicherung (LTI/Standard/…), Module mit
//! optionalem Basisschiff-Bezug.
//!
//! Import-Formate:
//! - **Hangar XPLORer JSON** (Primärformat): Array von Objekten mit
//!   `ship_code`, `lti`, `warbond`, `pledge_id/name/date/cost`, `ccud`,
//!   `entity_type`. Tolerant gegen unbekannte Felder, `unidentified`-Einträge
//!   und fehlende Optionalfelder.
//! - **CCUgame CSV**: Header `name,pledgeId,insurance,msrp,pledge,date,…,
//!   customName,account,packageName,tags`. `insurance` trägt `LTI`,
//!   Monate-Zahlen (`3`, `120`) oder leer.
//!
//! Merge-Strategie: Datensätze mit gleicher `pledge_id` werden aktualisiert
//! (nicht dupliziert); ohne pledge_id zählt das (ship_code, name)-Paar.
//!
//! Export: eigenes sauberes JSON-Format (alle Felder, insurance vollständig).
//!
//! Kein Org-Tool/API-Anbindung in dieser Karte (out of scope).

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::trade::TradeState;
use cache::Cache;

// ---------------------------------------------------------------------------
// Datenmodell (spiegelt die `hangar`-Tabelle)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HangarItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    /// Modellname, z. B. "600i Explorer".
    pub name: String,
    /// `pledge` | `ingame`.
    #[serde(default = "default_origin")]
    pub origin: String,
    /// Versicherung: "LTI", "3 Monate", "120 Monate", … — NULL bei ingame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insurance: Option<String>,
    /// Individueller Name (CCUgame `customName` / XPLorer `ship_name`).
    #[serde(default)]
    pub custom_name: String,
    /// `ship` | `vehicle` | `module`.
    #[serde(default = "default_entity_type")]
    pub entity_type: String,
    /// Nur Module: Name des Basisschiffs.
    #[serde(default)]
    pub base_ship: String,
    #[serde(default)]
    pub ship_code: String,
    #[serde(default)]
    pub manufacturer: String,
    /// USD (aus XPLorer `pledge_cost` / CCUgame `pledge`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purchase_price: Option<f64>,
    /// ISO `YYYY-MM-DD`, wenn parsebar; sonst Originaltext.
    #[serde(default)]
    pub purchase_date: String,
    #[serde(default)]
    pub pledge_id: String,
    #[serde(default)]
    pub pledge_name: String,
    #[serde(default)]
    pub warbond: bool,
    #[serde(default)]
    pub ccud: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

fn default_origin() -> String {
    "pledge".to_string()
}

fn default_entity_type() -> String {
    "ship".to_string()
}

#[derive(Clone, Debug, Serialize)]
pub struct HangarImportResult {
    pub imported: usize,
    pub updated: usize,
    pub skipped: usize,
    pub format: String,
}

// ---------------------------------------------------------------------------
// Import-Parser
// ---------------------------------------------------------------------------

/// Hangar XPLORer JSON (Primärformat). Akzeptiert ein JSON-Array; einzelne
/// Einträge, die weder `name` noch `ship_name` tragen, werden übersprungen.
pub fn parse_xplorer_json(text: &str) -> Result<(Vec<HangarItem>, usize), String> {
    let arr: Vec<serde_json::Value> =
        serde_json::from_str(text.trim()).map_err(|e| format!("Ungültiges XPLORer-JSON: {e}"))?;
    let mut out = Vec::with_capacity(arr.len());
    let mut skipped = 0usize;
    for v in arr.iter() {
        // Modellname: `name` ist bei XPLorer die Werksbezeichnung; `ship_name`
        // kann ein Custom-Namen tragen (z. B. "Riftwalker" auf der 600i).
        let orig_name =
            str_of(v, "orig_name").unwrap_or_else(|| str_of(v, "name").unwrap_or_default());
        let ship_name = str_of(v, "ship_name").unwrap_or_default();
        let custom_name = if !ship_name.is_empty() && ship_name != orig_name {
            ship_name.clone()
        } else {
            String::new()
        };
        let name = if !orig_name.is_empty() {
            orig_name
        } else if !ship_name.is_empty() {
            ship_name
        } else {
            // Weder name noch ship_name → unbrauchbarer Eintrag (z. B. das
            // "unidentified"-Report-Objekt des Plugins ohne Titel).
            skipped += 1;
            continue;
        };
        let lti = bool_of(v, "lti");
        let entity_type = match str_of(v, "entity_type").as_deref() {
            Some("vehicle") => "vehicle",
            Some("module") => "module",
            _ => "ship",
        };
        out.push(HangarItem {
            id: None,
            name,
            origin: "pledge".to_string(),
            insurance: if lti { Some("LTI".to_string()) } else { None },
            custom_name,
            entity_type: entity_type.to_string(),
            base_ship: String::new(),
            ship_code: str_of(v, "ship_code").unwrap_or_default(),
            manufacturer: str_of(v, "manufacturer_name")
                .or_else(|| str_of(v, "manufacturer_code"))
                .unwrap_or_default(),
            purchase_price: parse_usd(str_of(v, "pledge_cost").as_deref()),
            purchase_date: parse_xplorer_date(str_of(v, "pledge_date").as_deref()),
            pledge_id: str_of(v, "pledge_id").unwrap_or_default(),
            pledge_name: str_of(v, "pledge_name").unwrap_or_default(),
            warbond: bool_of(v, "warbond"),
            ccud: bool_of(v, "ccud"),
            created_at: None,
            updated_at: None,
        });
    }
    Ok((out, skipped))
}

/// CCUgame CSV (tolerant): typisierte Header-Zeile (`name,pledgeId,insurance,…`),
/// Felder in doppelten Anführungszeichen, leere Felder erlaubt.
pub fn parse_ccugame_csv(text: &str) -> Result<(Vec<HangarItem>, usize), String> {
    let mut skipped = 0usize;
    let mut rows = parse_csv_rows(text)?;
    let header = if rows.is_empty() {
        return Ok((Vec::new(), 0));
    } else {
        rows.remove(0)
    };
    let idx = |col: &str| header.iter().position(|h| h.eq_ignore_ascii_case(col));

    // Pflichtspalten
    let i_name = idx("name").ok_or("CCUgame-CSV: Spalte 'name' fehlt")?;
    let i_pledge = idx("pledgeId");
    let i_ins = idx("insurance");
    let i_msrp = idx("msrp");
    let i_pledge_cost = idx("pledge");
    let i_date = idx("date");
    let i_custom = idx("customName");
    let _i_account = idx("account");
    let i_package = idx("packageName");

    let mut out = Vec::new();
    for row in rows.iter() {
        let cell = |j: Option<usize>| {
            j.and_then(|j| row.get(j))
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        };
        let name = cell(Some(i_name));
        if name.is_empty() {
            skipped += 1;
            continue;
        }
        let insurance = normalize_insurance(&cell(i_ins));
        out.push(HangarItem {
            id: None,
            name,
            origin: "pledge".to_string(),
            insurance,
            custom_name: cell(i_custom),
            entity_type: "ship".to_string(),
            base_ship: String::new(),
            ship_code: String::new(),
            manufacturer: String::new(),
            purchase_price: cell(i_pledge_cost)
                .parse::<f64>()
                .ok()
                .or_else(|| parse_usd(Some(&cell(i_pledge_cost)))),
            purchase_date: cell(i_date),
            pledge_id: cell(i_pledge),
            pledge_name: cell(i_package),
            warbond: false,
            ccud: false,
            created_at: None,
            updated_at: None,
        });
        let _ = i_msrp; // MSRP zeigt den Listenpreis, kein eigenes Feld nötig — bewusst verworfen.
    }
    Ok((out, skipped))
}

/// Normalisiert das CCUgame-`insurance`-Feld: "LTI" bleibt, Monatszahlen
/// (3, 6, 120, …) werden zu "n Monate", leer/unknown → None.
fn normalize_insurance(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.eq_ignore_ascii_case("LTI") {
        return Some("LTI".to_string());
    }
    if let Ok(months) = raw.parse::<u32>() {
        return Some(format!("{months} Monate"));
    }
    Some(raw.to_string())
}

/// "$180.00 USD" / "$0.00" → 180.0.
fn parse_usd(v: Option<&str>) -> Option<f64> {
    let v = v?.trim();
    let digits: String = v
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',' || *c == '-')
        .collect();
    let digits = digits.replace(',', "");
    digits.parse::<f64>().ok()
}

/// "July 12, 2024" → "2024-07-12"; nicht parsebar → Originaltext zurück.
fn parse_xplorer_date(v: Option<&str>) -> String {
    let Some(v) = v else { return String::new() };
    let v = v.trim();
    if v.is_empty() {
        return String::new();
    }
    // Format: "<Month> <d>, <yyyy>"
    let parts: Vec<&str> = v
        .split(|c| c == ' ' || c == ',')
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() != 3 {
        return v.to_string();
    }
    let month = match parts[0].to_lowercase().as_str() {
        "january" | "jan" => 1,
        "february" | "feb" => 2,
        "march" | "mar" => 3,
        "april" | "apr" => 4,
        "may" => 5,
        "june" | "jun" => 6,
        "july" | "jul" => 7,
        "august" | "aug" => 8,
        "september" | "sep" => 9,
        "october" | "oct" => 10,
        "november" | "nov" => 11,
        "december" | "dec" => 12,
        _ => return v.to_string(),
    };
    let day: u32 = match parts[1].parse() {
        Ok(d) if (1..=31).contains(&d) => d,
        _ => return v.to_string(),
    };
    let year: i32 = match parts[2].parse() {
        Ok(y) if (1900..=3000).contains(&y) => y,
        _ => return v.to_string(),
    };
    format!("{year:04}-{month:02}-{day:02}")
}

/// Toleranter quoted-CSV-Splitter: 'a,"b,c",d' → ["a", "b,c", "d"].
/// Behandelt Anführungszeichen, Escapes ("") und Zeilenumbrüche innerhalb
/// eines Feldes; kaputt gequotete Zellen (XPLorer-CSV) fallen auf den
/// Rohbestand zurück, statt zu crashen.
fn parse_csv_rows(text: &str) -> Result<Vec<Vec<String>>, String> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if !in_quotes => in_quotes = true,
            '"' if in_quotes => {
                if chars.peek() == Some(&'"') {
                    chars.next(); // escaped quote → literal "
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            }
            ',' if !in_quotes => {
                row.push(field.trim().to_string());
                field.clear();
            }
            '\r' => {}
            '\n' if !in_quotes => {
                row.push(field.trim().to_string());
                field.clear();
                if row.iter().any(|f| !f.is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
            }
            _ => field.push(c),
        }
    }
    // Letzte Zeile ohne abschließenden Newline.
    if !field.is_empty() || row.iter().any(|f| !f.is_empty()) {
        row.push(field.trim().to_string());
        rows.push(row);
    }
    Ok(rows)
}

fn str_of(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
}

fn bool_of(v: &serde_json::Value, key: &str) -> bool {
    v.get(key).and_then(|x| x.as_bool()).unwrap_or(false)
}

// ---------------------------------------------------------------------------
// DB-Zugriff (Cache-Tabelle `hangar`)
// ---------------------------------------------------------------------------

fn row_to_item(r: &rusqlite::Row<'_>) -> rusqlite::Result<HangarItem> {
    Ok(HangarItem {
        id: Some(r.get(0)?),
        name: r.get(1)?,
        origin: r.get(2)?,
        insurance: r.get(3)?,
        custom_name: r.get(4)?,
        entity_type: r.get(5)?,
        base_ship: r.get(6)?,
        ship_code: r.get(7)?,
        manufacturer: r.get(8)?,
        purchase_price: r.get(9)?,
        purchase_date: r.get(10)?,
        pledge_id: r.get(11)?,
        pledge_name: r.get(12)?,
        warbond: r.get::<_, i64>(13)? != 0,
        ccud: r.get::<_, i64>(14)? != 0,
        created_at: Some(r.get(15)?),
        updated_at: Some(r.get(16)?),
    })
}

const HANGAR_COLS: &str = "id, name, origin, insurance, custom_name, entity_type, base_ship, \
     ship_code, manufacturer, purchase_price, purchase_date, pledge_id, pledge_name, \
     warbond, ccud, created_at, updated_at";

/// Merge-Kandidat für einen Import-/CRUD-Eintrag suchen.
/// Wichtig: Pack-/Bundle-Einträge teilen sich eine pledge_id, enthalten aber
/// mehrere Objekte (z. B. Golem Duo Pack: DRAK_Golem + DRAK_Golem_OX). Der
/// Merge-Key muss das Objekt innerhalb des Pledges unterscheiden:
///   1. (pledge_id, ship_code)  — wenn ship_code vorliegt
///   2. (pledge_id, name)       — CSV hat keinen ship_code
///   3. (ship_code, name)       — ohne pledge_id
///   4. (name, custom_name)     — ohne beides
fn find_merge_candidate(
    tx: &rusqlite::Transaction<'_>,
    item: &HangarItem,
) -> rusqlite::Result<Option<i64>> {
    if !item.pledge_id.is_empty() && !item.ship_code.is_empty() {
        let hit = tx
            .query_row(
                "SELECT id FROM hangar WHERE pledge_id = ?1 AND ship_code = ?2 LIMIT 1",
                rusqlite::params![item.pledge_id, item.ship_code],
                |r| r.get(0),
            )
            .ok();
        if hit.is_some() {
            return Ok(hit);
        }
    }
    if !item.pledge_id.is_empty() {
        let hit = tx
            .query_row(
                "SELECT id FROM hangar WHERE pledge_id = ?1 AND name = ?2 LIMIT 1",
                rusqlite::params![item.pledge_id, item.name],
                |r| r.get(0),
            )
            .ok();
        if hit.is_some() {
            return Ok(hit);
        }
    }
    if !item.ship_code.is_empty() {
        return Ok(tx
            .query_row(
                "SELECT id FROM hangar WHERE ship_code = ?1 AND name = ?2 LIMIT 1",
                rusqlite::params![item.ship_code, item.name],
                |r| r.get(0),
            )
            .ok());
    }
    if !item.pledge_id.is_empty() {
        // Fallback für ship_code-lose Zeilen (CCUgame-CSV), deren Namen von
        // XPLorer abweichen (z. B. "600i Explorer" vs. "600i Exploration
        // Module"): Merge auf die pledge_id — ABER nur wenn genau EINE Zeile
        // diesen Pledge hat. Mehrzeilige Packs (Golem Duo) kollabieren so nie.
        let count: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM hangar WHERE pledge_id = ?1",
                [item.pledge_id.as_str()],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if count == 1 {
            return Ok(tx
                .query_row(
                    "SELECT id FROM hangar WHERE pledge_id = ?1 LIMIT 1",
                    [item.pledge_id.as_str()],
                    |r| r.get(0),
                )
                .ok());
        }
        return Ok(None);
    }
    Ok(tx
        .query_row(
            "SELECT id FROM hangar WHERE ship_code = '' AND name = ?1 AND custom_name = ?2 LIMIT 1",
            rusqlite::params![item.name, item.custom_name],
            |r| r.get(0),
        )
        .ok())
}

/// Import-Merge: bestehende Zeile nur mit NICHT-leeren Feldern des Imports
/// anreichern. Sonst würde ein ärmerer Import (CCUgame-CSV hat kein
/// ship_code/manufacturer/warbond/ccud) den reicheren Datensatz (XPLorer-JSON)
/// degradieren — das Ergebnis darf nicht von der Import-Reihenfolge abhängen.
fn merge_import(existing: &HangarItem, item: &HangarItem) -> HangarItem {
    let nz = |inc: &str, cur: &str| -> String {
        if inc.trim().is_empty() {
            cur.to_string()
        } else {
            inc.trim().to_string()
        }
    };
    HangarItem {
        id: existing.id,
        name: nz(&item.name, &existing.name),
        // origin/entity_type: Importquellen liefern teils hartcodierte Defaults
        // (CSV setzt immer "ship") — bestehenden Wert nicht verschlechtern.
        origin: if existing.origin.is_empty() {
            item.origin.clone()
        } else {
            existing.origin.clone()
        },
        insurance: item
            .insurance
            .clone()
            .or_else(|| existing.insurance.clone()),
        custom_name: nz(&item.custom_name, &existing.custom_name),
        entity_type: if existing.entity_type.is_empty() || item.entity_type != "ship" {
            item.entity_type.clone()
        } else {
            existing.entity_type.clone()
        },
        base_ship: nz(&item.base_ship, &existing.base_ship),
        ship_code: nz(&item.ship_code, &existing.ship_code),
        manufacturer: nz(&item.manufacturer, &existing.manufacturer),
        purchase_price: item.purchase_price.or(existing.purchase_price),
        purchase_date: nz(&item.purchase_date, &existing.purchase_date),
        pledge_id: nz(&item.pledge_id, &existing.pledge_id),
        pledge_name: nz(&item.pledge_name, &existing.pledge_name),
        warbond: item.warbond || existing.warbond,
        ccud: item.ccud || existing.ccud,
        created_at: existing.created_at,
        updated_at: existing.updated_at,
    }
}

fn upsert(
    conn: &mut rusqlite::Connection,
    item: &HangarItem,
    now: i64,
) -> rusqlite::Result<(i64, bool)> {
    let tx = conn.transaction()?;
    // Manuelles Update (hangar_update) kommt MIT id — dann gezielt nach id
    // schreiben, kein Merge-Matching (könnte eine fremde Zeile treffen).
    let existing: Option<i64> = match item.id {
        Some(id) => Some(id),
        None => find_merge_candidate(&tx, item)?,
    };
    let id = match existing {
        Some(id) => {
            // Import (item.id == None): bestehende Zeile laden und nur mit
            // nicht-leeren Feldern anreichern (kein Degradieren durch ärmere
            // Importquellen). Manuelles Update (item.id == Some) schreibt
            // bewusst alles, was das UI liefert.
            let effective: HangarItem = if item.id.is_none() {
                let cur: HangarItem = tx.query_row(
                    &format!("SELECT {HANGAR_COLS} FROM hangar WHERE id = ?1"),
                    [id],
                    row_to_item,
                )?;
                merge_import(&cur, item)
            } else {
                item.clone()
            };
            let insurance = effective.insurance.as_deref();
            let price = effective.purchase_price;
            tx.execute(
                "UPDATE hangar SET name=?1, origin=?2, insurance=?3, custom_name=?4, \
                 entity_type=?5, base_ship=?6, ship_code=?7, manufacturer=?8, \
                 purchase_price=?9, purchase_date=?10, pledge_id=?11, pledge_name=?12, \
                 warbond=?13, ccud=?14, updated_at=?15 WHERE id=?16",
                rusqlite::params![
                    effective.name,
                    effective.origin,
                    insurance,
                    effective.custom_name,
                    effective.entity_type,
                    effective.base_ship,
                    effective.ship_code,
                    effective.manufacturer,
                    price,
                    effective.purchase_date,
                    effective.pledge_id,
                    effective.pledge_name,
                    effective.warbond as i64,
                    effective.ccud as i64,
                    now,
                    id
                ],
            )?;
            (id, false)
        }
        None => {
            let insurance = item.insurance.as_deref();
            let price = item.purchase_price;
            tx.execute(
                "INSERT INTO hangar (name, origin, insurance, custom_name, entity_type, \
                 base_ship, ship_code, manufacturer, purchase_price, purchase_date, \
                 pledge_id, pledge_name, warbond, ccud, created_at, updated_at) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
                rusqlite::params![
                    item.name,
                    item.origin,
                    insurance,
                    item.custom_name,
                    item.entity_type,
                    item.base_ship,
                    item.ship_code,
                    item.manufacturer,
                    price,
                    item.purchase_date,
                    item.pledge_id,
                    item.pledge_name,
                    item.warbond as i64,
                    item.ccud as i64,
                    now,
                    now
                ],
            )?;
            (tx.last_insert_rowid(), true)
        }
    };
    tx.commit()?;
    Ok(id)
}

// ---------------------------------------------------------------------------
// Tauri-Kommandos
// ---------------------------------------------------------------------------

/// Alle Hangar-Einträge (Schiffe/Fahrzeuge/Module), nach Typ und Name sortiert.
#[tauri::command]
pub fn hangar_list(state: State<'_, Arc<TradeState>>) -> Result<Vec<HangarItem>, String> {
    let cache = &state.inner().cache;
    cache
        .with(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {HANGAR_COLS} FROM hangar ORDER BY entity_type, name"
            ))?;
            let rows = stmt.query_map([], row_to_item)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(|e| format!("hangar list: {e}"))
}

/// Eintrag manuell anlegen (für Ingame-Käufe, die in keinem Export auftauchen).
#[tauri::command]
pub fn hangar_add(
    state: State<'_, Arc<TradeState>>,
    item: HangarItem,
) -> Result<HangarItem, String> {
    if item.name.trim().is_empty() {
        return Err("Name darf nicht leer sein".to_string());
    }
    let item = HangarItem {
        name: item.name.trim().to_string(),
        ..item
    };
    let now = unix_now() as i64;
    let cache = &state.inner().cache;
    let (id, _inserted) = cache
        .with(|conn| upsert(conn, &item, now))
        .map_err(|e| format!("hangar add: {e}"))?;
    hangar_get_by_id(cache, id)
}

/// Eintrag bearbeiten (Versicherung nachtragen, Custom-Name, Basisschiff …).
#[tauri::command]
pub fn hangar_update(
    state: State<'_, Arc<TradeState>>,
    item: HangarItem,
) -> Result<HangarItem, String> {
    let id = item.id.ok_or("hangar_update ohne id")?;
    let cache = &state.inner().cache;
    let now = unix_now() as i64;
    // Existiert der Datensatz? Dann Feldweise übernehmen (id bleibt).
    let current = hangar_get_by_id(cache, id)?;
    let merged = HangarItem {
        id: Some(id),
        created_at: current.created_at,
        updated_at: Some(now),
        ..item
    };
    let (updated_id, _inserted) = cache
        .with(|conn| upsert(conn, &merged, now))
        .map_err(|e| format!("hangar update: {e}"))?;
    let _ = updated_id;
    hangar_get_by_id(cache, id)
}

/// Eintrag löschen.
#[tauri::command]
pub fn hangar_delete(state: State<'_, Arc<TradeState>>, id: i64) -> Result<(), String> {
    let cache = &state.inner().cache;
    cache
        .with(|conn| {
            conn.execute("DELETE FROM hangar WHERE id = ?1", [id])
                .map(|_| ())
        })
        .map_err(|e| format!("hangar delete: {e}"))
}

/// Import aus XPLorer-JSON oder CCUgame-CSV (`format: "xplorer" | "ccugame"`).
/// Merge: gleiche pledge_id / (ship_code, name) aktualisiert statt dupliziert.
#[tauri::command]
pub fn hangar_import(
    state: State<'_, Arc<TradeState>>,
    format: String,
    text: String,
) -> Result<HangarImportResult, String> {
    let (items, parse_skipped) = match format.as_str() {
        "xplorer" => parse_xplorer_json(&text)?,
        "ccugame" => parse_ccugame_csv(&text)?,
        other => return Err(format!("Unbekanntes Import-Format: {other}")),
    };
    let now = unix_now() as i64;
    let cache = &state.inner().cache;
    let mut imported = 0usize;
    let mut updated = 0usize;
    for item in &items {
        // Upsert entscheidet anhand der Merge-Regel und meldet insert/update.
        let (_id, inserted) = cache
            .with(|conn| upsert(conn, item, now))
            .map_err(|e| format!("hangar import: {e}"))?;
        if inserted {
            imported += 1;
        } else {
            updated += 1;
        }
    }
    Ok(HangarImportResult {
        imported,
        updated,
        skipped: parse_skipped,
        format,
    })
}

/// Eigenes sauberes Export-JSON (alle Felder inkl. vollständigem insurance).
#[tauri::command]
pub fn hangar_export(state: State<'_, Arc<TradeState>>) -> Result<String, String> {
    let items = hangar_list(state)?;
    serde_json::to_string_pretty(&items).map_err(|e| format!("hangar export: {e}"))
}

fn hangar_get_by_id(cache: &Cache, id: i64) -> Result<HangarItem, String> {
    cache
        .with(|conn| {
            conn.query_row(
                &format!("SELECT {HANGAR_COLS} FROM hangar WHERE id = ?1"),
                [id],
                row_to_item,
            )
        })
        .map_err(|e| format!("hangar get: {e}"))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// Tests (Inline-Samples aus den echten Beispieldateien von Sebo)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const XPLORER_SAMPLE: &str = r#"[
  {
    "ship_code": "ORIG_600i",
    "ship_name": "Riftwalker",
    "manufacturer_code": "ORIG",
    "manufacturer_name": "Origin Jumpworks",
    "lti": true,
    "name": "600i Exploration Module",
    "orig_name": "600i Exploration Module",
    "warbond": false,
    "entity_type": "ship",
    "pledge_id": "71394952",
    "pledge_name": "Nox - Foundation Festival 2024 Referral Bonus",
    "pledge_date": "July 12, 2024",
    "pledge_cost": "$180.00 USD",
    "ccud": false
  },
  {
    "unidentified": "Please report this ship",
    "ship_code": "DRAK_Dragonfly",
    "manufacturer_name": "Drake Interplanetary",
    "manufacturer_code": "DRAK",
    "lti": false,
    "name": "Dragonfly Star Kitten Edition",
    "orig_name": "Dragonfly Star Kitten Edition",
    "warbond": false,
    "entity_type": "ship",
    "pledge_id": "91692952",
    "pledge_name": "IAE 2955 Referral Bonus",
    "pledge_date": "November 20, 2025",
    "pledge_cost": "$0.00 USD",
    "ccud": false
  }
]"#;

    const CCUGAME_SAMPLE: &str = concat!(
        "name,pledgeId,insurance,msrp,pledge,date,isGiftable,isMeltable,isGifted,place,customName,account,packageName,tags\n",
        "\"600i Explorer\",71394952,\"LTI\",475,180,\"2024-07-12\",false,false,false,\"hangar\",\"Riftwalker\",\"[AAWA] SeBoOne\",\"Nox - Foundation Festival 2024 Referral Bonus\",\"\"\n",
        "\"Aurora Mk I SE\",104070748,\"3\",95,75,\"2026-03-24\",false,false,false,\"hangar\",,\"[AAWA] SeBoOne\",\"Package - Squadron 42 Pledge\",\"\"\n",
        "\"Melted Ship\",,\"\",0,0,\"\",false,false,false,\"hangar\",,\"\",\"\",\"\"\n"
    );

    #[test]
    fn xplorer_json_parses_ships_and_custom_names() {
        let (items, skipped) = parse_xplorer_json(XPLORER_SAMPLE).unwrap();
        assert_eq!(skipped, 0);
        assert_eq!(items.len(), 2);
        let rift = &items[0];
        // ship_name "Riftwalker" != orig_name → custom_name
        assert_eq!(rift.name, "600i Exploration Module");
        assert_eq!(rift.custom_name, "Riftwalker");
        assert_eq!(rift.insurance.as_deref(), Some("LTI"));
        assert_eq!(rift.purchase_price, Some(180.0));
        assert_eq!(rift.purchase_date, "2024-07-12");
        assert_eq!(rift.pledge_id, "71394952");
        assert_eq!(rift.manufacturer, "Origin Jumpworks");
        // Nicht-LTI → kein insurance-Feld
        assert!(items[1].insurance.is_none());
        assert_eq!(items[1].purchase_price, Some(0.0));
    }

    #[test]
    fn xplorer_json_skips_nameless_entries() {
        let text = r#"[{"unidentified": "report this", "lti": true}]"#;
        let (items, skipped) = parse_xplorer_json(text).unwrap();
        assert!(items.is_empty());
        assert_eq!(skipped, 1);
    }

    #[test]
    fn ccugame_csv_parses_insurance_variants() {
        let (items, skipped) = parse_ccugame_csv(CCUGAME_SAMPLE).unwrap();
        assert_eq!(skipped, 0);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].insurance.as_deref(), Some("LTI"));
        assert_eq!(items[0].custom_name, "Riftwalker");
        assert_eq!(items[0].purchase_price, Some(180.0));
        assert_eq!(items[1].insurance.as_deref(), Some("3 Monate"));
        assert_eq!(items[1].custom_name, "");
        // Leere Versicherung → None, leerer pledgeId → keine Merge-Key
        assert_eq!(items[2].insurance, None);
        assert_eq!(items[2].pledge_id, "");
    }

    #[test]
    fn ccugame_csv_rejects_missing_name_column() {
        assert!(parse_ccugame_csv("a,b\n1,2").is_err());
    }

    #[test]
    fn normalize_insurance_maps_values() {
        assert_eq!(normalize_insurance("LTI").as_deref(), Some("LTI"));
        assert_eq!(normalize_insurance("lti").as_deref(), Some("LTI"));
        assert_eq!(normalize_insurance("3").as_deref(), Some("3 Monate"));
        assert_eq!(normalize_insurance("120").as_deref(), Some("120 Monate"));
        assert_eq!(normalize_insurance(""), None);
        assert_eq!(normalize_insurance("Standard"), Some("Standard".into()));
    }

    #[test]
    fn parse_usd_handles_currency_strings() {
        assert_eq!(parse_usd(Some("$180.00 USD")), Some(180.0));
        assert_eq!(parse_usd(Some("$0.00")), Some(0.0));
        assert_eq!(parse_usd(Some("75")), Some(75.0));
        assert_eq!(parse_usd(Some("n/a")), None);
        assert_eq!(parse_usd(None), None);
    }

    #[test]
    fn parse_xplorer_date_iso() {
        assert_eq!(parse_xplorer_date(Some("July 12, 2024")), "2024-07-12");
        assert_eq!(parse_xplorer_date(Some("November 20, 2025")), "2025-11-20");
        assert_eq!(parse_xplorer_date(Some("2024-07-12")), "2024-07-12");
        assert_eq!(parse_xplorer_date(Some("garbage")), "garbage");
        assert_eq!(parse_xplorer_date(Some("")), "");
        assert_eq!(parse_xplorer_date(None), "");
    }

    #[test]
    fn csv_parser_survives_quoted_and_embedded() {
        let rows = parse_csv_rows("a,\"b,c\",d\n\"x\",\"y\",\n").unwrap();
        assert_eq!(rows, vec![vec!["a", "b,c", "d"], vec!["x", "y", ""]]);
    }

    #[test]
    fn csv_parser_survives_broken_quotes() {
        // Kaputt gequotet (XPLorer-CSV-Fall): unbalanced quote soll nicht crashen.
        let rows = parse_csv_rows("\"open,keep\n").unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn hangar_table_crud_roundtrip() {
        let cache = cache::Cache::open_in_memory().unwrap();
        let now = 1_792_500_000i64;
        let item = HangarItem {
            id: None,
            name: "600i Explorer".into(),
            origin: "pledge".into(),
            insurance: Some("LTI".into()),
            custom_name: "Riftwalker".into(),
            entity_type: "ship".into(),
            base_ship: String::new(),
            ship_code: "ORIG_600i".into(),
            manufacturer: "Origin Jumpworks".into(),
            purchase_price: Some(180.0),
            purchase_date: "2024-07-12".into(),
            pledge_id: "71394952".into(),
            pledge_name: "Nox".into(),
            warbond: false,
            ccud: false,
            created_at: None,
            updated_at: None,
        };
        let (id, inserted) = cache
            .with(|c| upsert(c, &item, now))
            .map_err(|e| e.to_string())
            .unwrap();
        assert!(inserted);
        // Same pledge_id + ship_code → update, not duplicate
        let mut updated = item.clone();
        updated.insurance = Some("Standard".into());
        let (id2, inserted2) = cache
            .with(|c| upsert(c, &updated, now + 10))
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(id, id2);
        assert!(!inserted2);
        let count: i64 = cache
            .with(|c| c.query_row("SELECT COUNT(*) FROM hangar", [], |r| r.get(0)))
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(count, 1);
        // Delete
        cache
            .with(|c| c.execute("DELETE FROM hangar WHERE id = ?1", [id]))
            .map_err(|e| e.to_string())
            .unwrap();
        let count: i64 = cache
            .with(|c| c.query_row("SELECT COUNT(*) FROM hangar", [], |r| r.get(0)))
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn pack_with_shared_pledge_id_keeps_both_ships() {
        // Realer Fall (Golem Duo Pack, pledge_id 93874564): zwei Schiffe in
        // EINEM Pledge — der Merge-Key (pledge_id, ship_code/name) darf sie
        // nicht kollabieren lassen (Regression zu Review-Blocker 1).
        let cache = cache::Cache::open_in_memory().unwrap();
        let now = 1_792_500_000i64;
        let mk = |name: &str, code: &str| HangarItem {
            id: None,
            name: name.into(),
            origin: "pledge".into(),
            insurance: Some("LTI".into()),
            custom_name: String::new(),
            entity_type: "ship".into(),
            base_ship: String::new(),
            ship_code: code.into(),
            manufacturer: "Drake".into(),
            purchase_price: Some(0.0),
            purchase_date: String::new(),
            pledge_id: "93874564".into(),
            pledge_name: "Packs - Golem Duo Pack plus Sunstruck Paint".into(),
            warbond: false,
            ccud: false,
            created_at: None,
            updated_at: None,
        };
        cache
            .with(|c| upsert(c, &mk("Golem", "DRAK_Golem"), now))
            .map_err(|e| e.to_string())
            .unwrap();
        cache
            .with(|c| upsert(c, &mk("Golem OX", "DRAK_Golem_OX"), now))
            .map_err(|e| e.to_string())
            .unwrap();
        // Re-Import beider Einträge (XPLorer-Repeat) → weiterhin 2 Zeilen.
        cache
            .with(|c| upsert(c, &mk("Golem", "DRAK_Golem"), now))
            .map_err(|e| e.to_string())
            .unwrap();
        cache
            .with(|c| upsert(c, &mk("Golem OX", "DRAK_Golem_OX"), now))
            .map_err(|e| e.to_string())
            .unwrap();
        let count: i64 = cache
            .with(|c| c.query_row("SELECT COUNT(*) FROM hangar", [], |r| r.get(0)))
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(count, 2);
        // CCUgame-Variante (kein ship_code, Namen identisch) merge auf dieselben Zeilen.
        let csvlike = |name: &str| HangarItem {
            ship_code: String::new(),
            manufacturer: String::new(),
            name: name.into(),
            ..mk(name, "")
        };
        cache
            .with(|c| upsert(c, &csvlike("Golem"), now))
            .map_err(|e| e.to_string())
            .unwrap();
        cache
            .with(|c| upsert(c, &csvlike("Golem OX"), now))
            .map_err(|e| e.to_string())
            .unwrap();
        let count: i64 = cache
            .with(|c| c.query_row("SELECT COUNT(*) FROM hangar", [], |r| r.get(0)))
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn ccugame_import_after_xplorer_does_not_degrade() {
        // Regression zu Review-Blocker 2: ärmerer Import (CSV ohne ship_code/
        // manufacturer/warbond) darf die reichere XPLorer-Zeile nicht leeren.
        let cache = cache::Cache::open_in_memory().unwrap();
        let now = 1_792_500_000i64;
        let rich = HangarItem {
            id: None,
            name: "600i Exploration Module".into(),
            origin: "pledge".into(),
            insurance: Some("LTI".into()),
            custom_name: "Riftwalker".into(),
            entity_type: "ship".into(),
            base_ship: String::new(),
            ship_code: "ORIG_600i".into(),
            manufacturer: "Origin Jumpworks".into(),
            purchase_price: Some(180.0),
            purchase_date: "2024-07-12".into(),
            pledge_id: "71394952".into(),
            pledge_name: "600i Exploration".into(),
            warbond: false,
            ccud: true,
            created_at: None,
            updated_at: None,
        };
        let (id, _) = cache
            .with(|c| upsert(c, &rich, now))
            .map_err(|e| e.to_string())
            .unwrap();
        let poor = HangarItem {
            ship_code: String::new(),
            manufacturer: String::new(),
            custom_name: String::new(),
            purchase_price: None,
            purchase_date: String::new(),
            warbond: false,
            ccud: false,
            ..rich.clone()
        };
        let (id2, _) = cache
            .with(|c| upsert(c, &poor, now + 10))
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(id, id2);
        let merged: HangarItem = cache
            .with(|c| {
                c.query_row(
                    &format!("SELECT {HANGAR_COLS} FROM hangar WHERE id = ?1"),
                    [id],
                    row_to_item,
                )
            })
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(merged.ship_code, "ORIG_600i");
        assert_eq!(merged.manufacturer, "Origin Jumpworks");
        assert_eq!(merged.custom_name, "Riftwalker");
        assert_eq!(merged.purchase_price, Some(180.0));
        assert_eq!(merged.purchase_date, "2024-07-12");
        assert!(merged.ccud);
    }

    #[test]
    fn real_export_files_parse_and_merge() {
        // Smoke gegen die realen Beispieldateien (/tmp/HangarExport, von Sebo).
        // Skippt, wenn die Dateien fehlen (CI/anderer Rechner).
        let dir = std::path::Path::new("/tmp/HangarExport");
        let json_path = dir.join("Hangar XPLORer.json");
        let csv_path = dir.join("CCUgame.csv");
        if !json_path.exists() || !csv_path.exists() {
            eprintln!("real_export_files_parse_and_merge: /tmp/HangarExport fehlt — skipped");
            return;
        }
        let json_text = std::fs::read_to_string(&json_path).unwrap();
        let csv_text = std::fs::read_to_string(&csv_path).unwrap();
        let (xplorer, sk1) = parse_xplorer_json(&json_text).unwrap();
        assert_eq!(sk1, 0, "XPLORer-Realdatei: keine Einträge verworfen");
        let (ccugame, sk2) = parse_ccugame_csv(&csv_text).unwrap();
        assert_eq!(sk2, 0, "CCUgame-Realdatei: keine Zeilen verworfen");

        let cache = cache::Cache::open_in_memory().unwrap();
        let now = 1_792_500_000i64;
        for it in &xplorer {
            cache
                .with(|c| upsert(c, it, now))
                .map_err(|e| e.to_string())
                .unwrap();
        }
        let after_x: i64 = cache
            .with(|c| c.query_row("SELECT COUNT(*) FROM hangar", [], |r| r.get(0)))
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(
            after_x as usize,
            xplorer.len(),
            "XPLORer-Import: keine Kollabierung"
        );
        for it in &ccugame {
            cache
                .with(|c| upsert(c, it, now))
                .map_err(|e| e.to_string())
                .unwrap();
        }
        let after_csv: i64 = cache
            .with(|c| c.query_row("SELECT COUNT(*) FROM hangar", [], |r| r.get(0)))
            .map_err(|e| e.to_string())
            .unwrap();
        // Re-Import darf weder kollabieren noch Zeilen löschen.
        assert!(
            after_csv >= after_x,
            "CCUgame-Nachimport darf keine Zeilen entfernen"
        );
        // Kein Feld-Leeren durch den ärmeren CSV-Import:
        let empty_codes: i64 = cache
            .with(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM hangar WHERE pledge_id != '' AND ship_code = ''",
                    [],
                    |r| r.get(0),
                )
            })
            .map_err(|e| e.to_string())
            .unwrap();
        // Zeilen, die XPLorer einen ship_code gegeben hat, müssen ihn behalten.
        let xplorer_with_code = xplorer.iter().filter(|i| !i.ship_code.is_empty()).count() as i64;
        assert!(
            empty_codes <= after_x - xplorer_with_code,
            "CCUgame-Nachimport hat ship_codes geleert: {empty_codes}"
        );
        // Der Golem-Duo-Pack bleibt zweizeilig.
        let golem_rows: i64 = cache
            .with(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM hangar WHERE pledge_id = '93874564'",
                    [],
                    |r| r.get(0),
                )
            })
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(golem_rows, 2, "Golem Duo Pack: 2 Schiffe, nicht 1");
    }
    #[test]
    fn dump_real_import_for_smoke() {
        // Gate: ARCH_HANGAR_DUMP=<pfad> — importiert die realen
        // /tmp/HangarExport-Dateien in eine In-Memory-Cache-DB und schreibt
        // das hangar_list-Ergebnis als JSON (für den UI-Smoke, docs/scripts).
        let Ok(out) = std::env::var("ARCH_HANGAR_DUMP") else {
            eprintln!("dump_real_import_for_smoke: ARCH_HANGAR_DUMP nicht gesetzt — skipped");
            return;
        };
        let dir = std::path::Path::new("/tmp/HangarExport");
        let json_text = std::fs::read_to_string(dir.join("Hangar XPLORer.json")).unwrap();
        let csv_text = std::fs::read_to_string(dir.join("CCUgame.csv")).unwrap();
        let (xplorer, _) = parse_xplorer_json(&json_text).unwrap();
        let (ccugame, _) = parse_ccugame_csv(&csv_text).unwrap();
        let cache = cache::Cache::open_in_memory().unwrap();
        let now = 1_792_500_000i64;
        for it in xplorer.iter().chain(ccugame.iter()) {
            cache
                .with(|c| upsert(c, it, now))
                .map_err(|e| e.to_string())
                .unwrap();
        }
        let items: Vec<HangarItem> = cache
            .with(|c| {
                let mut stmt = c.prepare(&format!(
                    "SELECT {HANGAR_COLS} FROM hangar ORDER BY entity_type, name"
                ))?;
                let rows = stmt.query_map([], row_to_item)?;
                rows.collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(|e| e.to_string())
            .unwrap();
        std::fs::write(&out, serde_json::to_string_pretty(&items).unwrap()).unwrap();
        println!(
            "dump_real_import_for_smoke: {} Einträge -> {out}",
            items.len()
        );
    }
}
