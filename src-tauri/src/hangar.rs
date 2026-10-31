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
//! Merge-Strategie: Importe werden pledge-gruppenweise gematcht —
//! (pledge_id, ship_code), dann (pledge_id, Name). Nur wenn Pledge im
//! Import-Batch UND in der DB jeweils genau EIN Objekt enthält, darf über
//! die pledge_id gemerged werden (toleriert Namensvarianten zwischen den
//! Formaten, z. B. „600i Explorer" vs. „600i Exploration Module"). Packs mit
//! mehreren Objekten je pledge_id (z. B. Golem Duo Pack) kollabieren so nie.
//! Ohne pledge_id zählt (ship_code, name) bzw. (name, custom_name).
//! Modellnamen folgen einer Quellen-Priorität: manual > xplorer > ccugame.
//!
//! Export: eigenes sauberes JSON-Format (alle Felder, insurance vollständig).
//!
//! Kein Org-Tool/API-Anbindung in dieser Karte (out of scope).

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
    /// Import-Herkunft des letzten Schreibvorgangs: "xplorer" | "ccugame" |
    /// "manual". Steuert die Namens-Priorität beim Merge. Intern — nicht
    /// Teil des Export-JSON (`serde(skip)` hält das Export-Format stabil).
    #[serde(skip, default)]
    pub source: String,
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
        // R2 (t_2158474c, Punkt 7): ship_code/pledge_id bleiben als INTERNE
        // Merge-Keys erhalten (Import-Idempotenz, Pack-Disambiguierung — die
        // Match-Strategie unten hängt an beiden), aber pledge_name/warbond/ccud
        // werden NICHT mehr geschrieben (Import-Artefakte, „nicht wichtig").
        // Die UI zeigt keinen dieser Felder mehr.
        out.push(HangarItem {
            id: None,
            // Quelle wird zentral in hangar_import gestempelt.
            source: String::new(),
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
            // TB (t_78a4a5e5, Punkt 3): Hangar-Preise werden nicht mehr
            // geführt — Importe ignorieren `pledge_cost` (Spalte bleibt in
            // der DB, wird nicht mehr befüllt/gezeigt).
            purchase_price: None,
            purchase_date: parse_xplorer_date(str_of(v, "pledge_date").as_deref()),
            pledge_id: str_of(v, "pledge_id").unwrap_or_default(),
            pledge_name: String::new(),
            warbond: false,
            ccud: false,
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
    let i_date = idx("date");
    let i_custom = idx("customName");
    let _i_account = idx("account");
    let _i_package = idx("packageName");
    let _i_msrp = idx("msrp"); // TB (t_78a4a5e5, Punkt 3): bewusst verworfen.
    let _i_pledge_cost = idx("pledge"); // TB (t_78a4a5e5, Punkt 3): bewusst verworfen.

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
        // R2 (t_2158474c, Punkt 7): packageName/pledge_name nicht mehr schreiben
        // (Import-Artefakt, „nicht wichtig“) — pledge_id bleibt interner Merge-Key.
        out.push(HangarItem {
            id: None,
            // Quelle wird zentral in hangar_import gestempelt.
            source: String::new(),
            name,
            origin: "pledge".to_string(),
            insurance,
            custom_name: cell(i_custom),
            entity_type: "ship".to_string(),
            base_ship: String::new(),
            ship_code: String::new(),
            manufacturer: String::new(),
            // TB (t_78a4a5e5, Punkt 3): Import ignoriert msrp/pledge-Preise.
            purchase_price: None,
            purchase_date: cell(i_date),
            pledge_id: cell(i_pledge),
            pledge_name: String::new(),
            warbond: false,
            ccud: false,
            created_at: None,
            updated_at: None,
        });
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

// TB (t_78a4a5e5, Punkt 3): `parse_usd` entfernt — Hangar-Preise werden
// nicht mehr geführt (Import verwirft msrp/pledge_cost).

/// "July 12, 2024" → "2024-07-12"; nicht parsebar → Originaltext zurück.
fn parse_xplorer_date(v: Option<&str>) -> String {
    let Some(v) = v else { return String::new() };
    let v = v.trim();
    if v.is_empty() {
        return String::new();
    }
    // Format: "<Month> <d>, <yyyy>"
    let parts: Vec<&str> = v.split([' ', ',']).filter(|p| !p.is_empty()).collect();
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
        source: r.get(15)?,
        created_at: Some(r.get(16)?),
        updated_at: Some(r.get(17)?),
    })
}

const HANGAR_COLS: &str = "id, name, origin, insurance, custom_name, entity_type, base_ship, \
     ship_code, manufacturer, purchase_price, purchase_date, pledge_id, pledge_name, \
     warbond, ccud, source, created_at, updated_at";

/// Normalisierte Namensbasis für Match-Vergleiche: lowercase,
/// Whitespace zusammengefasst — "Golem  OX" und "golem ox" sind gleich.
fn norm_name(s: &str) -> String {
    s.split_whitespace()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

fn name_matches(a: &str, b: &str) -> bool {
    let na = norm_name(a);
    !na.is_empty() && na == norm_name(b)
}

/// Quellen-Priorität für den Modellnamen: manuelle Eingaben schlagen XPLorer,
/// XPLorer (Primärformat) schlägt CCUgame. Leer (Altdaten) = niedrigste.
fn source_priority(source: &str) -> u8 {
    match source {
        "manual" => 3,
        "xplorer" => 2,
        "ccugame" => 1,
        _ => 0,
    }
}

/// Match-Kandidat für einen IMPORT-Eintrag innerhalb eines Batch-Laufs.
///
/// `db` ist der aktuelle Stand (bestehende Zeilen + in diesem Import bereits
/// geschriebene), `incoming_group` die Anzahl der Einträge MIT GLEICHER
/// pledge_id im GESAMTEN Import-Batch.
///
/// Wichtig: Pack-/Bundle-Einträge teilen sich eine pledge_id, enthalten aber
/// mehrere Objekte (z. B. Golem Duo Pack: DRAK_Golem + DRAK_Golem_OX). Der
/// Merge-Key muss das Objekt innerhalb des Pledges unterscheiden:
///   1. (pledge_id, ship_code)  — wenn ship_code vorliegt
///   2. (pledge_id, name)       — normalisiert; CSV hat keinen ship_code
///   3. pledge_id NUR bei 1-zu-1 (genau ein Objekt im Batch UND in der DB) —
///      toleriert Namensvarianten zwischen den Formaten („600i Explorer" vs.
///      „600i Exploration Module"); Mehrzeilige Packs kollabieren so nie.
///      (Review-Blocker: der alte COUNT==1-Fallback prüfte nur den DB-State
///      mitten im laufenden Import und verschluckte das zweite Pack-Schiff.)
///   4. (ship_code, name)       — ohne pledge_id
///   5. (name, custom_name)     — ohne beides
fn find_import_candidate(
    db: &[HangarItem],
    item: &HangarItem,
    incoming_group: usize,
) -> Option<usize> {
    if !item.pledge_id.is_empty() {
        let same_pledge: Vec<usize> = db
            .iter()
            .enumerate()
            .filter(|(_, r)| r.pledge_id == item.pledge_id)
            .map(|(i, _)| i)
            .collect();
        if !item.ship_code.is_empty()
            && let Some(i) = same_pledge
                .iter()
                .copied()
                .find(|&i| db[i].ship_code == item.ship_code)
        {
            return Some(i);
        }
        if let Some(i) = same_pledge
            .iter()
            .copied()
            .find(|&i| name_matches(&db[i].name, &item.name))
        {
            return Some(i);
        }
        if same_pledge.len() == 1 && incoming_group == 1 {
            return Some(same_pledge[0]);
        }
        return None;
    }
    if !item.ship_code.is_empty() {
        return db.iter().position(|r| {
            !r.ship_code.is_empty()
                && r.ship_code == item.ship_code
                && name_matches(&r.name, &item.name)
        });
    }
    db.iter().position(|r| {
        r.ship_code.is_empty()
            && name_matches(&r.name, &item.name)
            && r.custom_name == item.custom_name
    })
}

/// Match-Kandidat für einen MANUELLEN Eintrag (hangar_add ohne id): strikt —
/// nur eindeutige Feldkombinationen, KEIN pledge-only-Fallback. Ein manuell
/// angelegtes Objekt soll nie still eine fremde Zeile überschreiben; im
/// Zweifel entsteht eine neue (sichtbare, löschbare) Zeile.
fn find_strict_candidate(
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
    // Namens-Priorität (Review-Finding: der CCUgame-Rückimport überschrieb
    // den XPLorer-Modellnamen): eine Quelle darf den Namen nur schreiben,
    // wenn ihre Priorität mindestens so hoch ist wie die der bestehenden.
    // Gleiche Quelle darf aktualisieren (frischer Export derselben Herkunft).
    let incoming_name = item.name.trim();
    let name = if !incoming_name.is_empty()
        && source_priority(&item.source) >= source_priority(&existing.source)
    {
        incoming_name.to_string()
    } else if !existing.name.trim().is_empty() {
        existing.name.clone()
    } else {
        incoming_name.to_string()
    };
    let source = if name == incoming_name && !item.source.is_empty() {
        item.source.clone()
    } else {
        existing.source.clone()
    };
    HangarItem {
        id: existing.id,
        name,
        source,
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

fn write_update(
    tx: &rusqlite::Transaction<'_>,
    id: i64,
    item: &HangarItem,
    now: i64,
) -> rusqlite::Result<()> {
    let insurance = item.insurance.as_deref();
    let price = item.purchase_price;
    tx.execute(
        "UPDATE hangar SET name=?1, origin=?2, insurance=?3, custom_name=?4, \
         entity_type=?5, base_ship=?6, ship_code=?7, manufacturer=?8, \
         purchase_price=?9, purchase_date=?10, pledge_id=?11, pledge_name=?12, \
         warbond=?13, ccud=?14, source=?15, updated_at=?16 WHERE id=?17",
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
            item.source,
            now,
            id
        ],
    )?;
    Ok(())
}

fn write_insert(
    tx: &rusqlite::Transaction<'_>,
    item: &HangarItem,
    now: i64,
) -> rusqlite::Result<i64> {
    let insurance = item.insurance.as_deref();
    let price = item.purchase_price;
    tx.execute(
        "INSERT INTO hangar (name, origin, insurance, custom_name, entity_type, \
         base_ship, ship_code, manufacturer, purchase_price, purchase_date, \
         pledge_id, pledge_name, warbond, ccud, source, created_at, updated_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
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
            item.source,
            now,
            now
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

/// Upsert für MANUELLE CRUD-Pfade (hangar_add/hangar_update): striktes
/// Match (kein pledge-only-Fallback). Manuelles Update (hangar_update)
/// kommt MIT id — dann gezielt nach id schreiben, kein Merge-Matching
/// (könnte eine fremde Zeile treffen).
fn upsert(
    conn: &mut rusqlite::Connection,
    item: &HangarItem,
    now: i64,
) -> rusqlite::Result<(i64, bool)> {
    let tx = conn.transaction()?;
    let existing: Option<i64> = match item.id {
        Some(id) => Some(id),
        None => find_strict_candidate(&tx, item)?,
    };
    let (id, inserted) = match existing {
        Some(id) => {
            // Manuelles Anlegen (item.id == None) mit Treffer: bestehende
            // Zeile laden und nur mit nicht-leeren Feldern anreichern.
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
            write_update(&tx, id, &effective, now)?;
            (id, false)
        }
        None => (write_insert(&tx, item, now)?, true),
    };
    tx.commit()?;
    Ok((id, inserted))
}

/// Batch-Import: matcht den GESAMTEN Import-Batch pledge-gruppenweise gegen
/// die DB (inklusive der in diesem Lauf bereits geschriebenen Zeilen).
/// Ersetzt den zeilenweisen upsert für Importe — dessen pledge-only-Fallback
/// prüfte den DB-State MITTEN im laufenden Import (COUNT==1, sobald der
/// erste Pack-Eintrag stand) und kollabierte/kappte Pack-Schiffe je nach
/// Import-Reihenfolge (Review-R2-Blocker).
fn import_items(
    conn: &mut rusqlite::Connection,
    items: &[HangarItem],
    now: i64,
) -> rusqlite::Result<(usize, usize)> {
    use std::collections::HashMap;
    // Gruppengröße je pledge_id im Import-Batch (für die 1-zu-1-Regel).
    let mut pledge_counts: HashMap<&str, usize> = HashMap::new();
    for it in items {
        if !it.pledge_id.is_empty() {
            *pledge_counts.entry(it.pledge_id.as_str()).or_default() += 1;
        }
    }
    let tx = conn.transaction()?;
    let mut db: Vec<HangarItem> = {
        let mut stmt = tx.prepare(&format!("SELECT {HANGAR_COLS} FROM hangar"))?;
        let rows = stmt.query_map([], row_to_item)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut imported = 0usize;
    let mut updated = 0usize;
    for item in items {
        let incoming_group = if item.pledge_id.is_empty() {
            1
        } else {
            pledge_counts
                .get(item.pledge_id.as_str())
                .copied()
                .unwrap_or(1)
        };
        match find_import_candidate(&db, item, incoming_group) {
            Some(idx) => {
                let id = db[idx].id.expect("DB-Zeile hat id");
                let merged = merge_import(&db[idx], item);
                write_update(&tx, id, &merged, now)?;
                db[idx] = merged;
                updated += 1;
            }
            None => {
                let id = write_insert(&tx, item, now)?;
                let mut row = item.clone();
                row.id = Some(id);
                db.push(row);
                imported += 1;
            }
        }
    }
    tx.commit()?;
    Ok((imported, updated))
}

// ---------------------------------------------------------------------------
// Tauri-Kommandos
// ---------------------------------------------------------------------------

/// Alle Hangar-Einträge (Schiffe/Fahrzeuge/Module), nach Typ und Name sortiert.
#[tauri::command]
pub fn hangar_list(state: State<'_, TradeState>) -> Result<Vec<HangarItem>, String> {
    let cache = &state.inner().cache;
    cache
        .with(|conn| {
            migrate_unlinked_modules(conn)?;
            let mut stmt = conn.prepare(&format!(
                "SELECT {HANGAR_COLS} FROM hangar ORDER BY entity_type, name"
            ))?;
            let rows = stmt.query_map([], row_to_item)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(|e| format!("hangar list: {e}"))
}

/// TB (t_78a4a5e5, Punkt 2) — Migration: Modul-Zeilen wandern in die
/// Ship-Details. Datenseitig bleibt das zeilenbasierte Modell
/// (entity_type='module', base_ship) — die Migration VERKNÜPFT nur noch
/// bezugslose Module, wo möglich:
///
/// Ein Modul ohne `base_ship`, dessen `pledge_id` zu genau EINEM Schiff/
/// Fahrzeug (nicht-module Zeile) passt, bekommt dessen Modellnamen als
/// base_ship (gleiche Pledge-Gruppe = gehört zusammen, z. B. das
/// XPLorer-Modul-Objekt neben dem Basisschiff im selben Pledge).
///
/// Idempotent (nur base_ship='' betroffen) und verknüpfungsfreundlich:
/// wirklich Bezugsloses bleibt als „Modul (unverbunden)“ erhalten (R2-
/// Vereinbarung) — die UI zeigt es in der Liste und bietet dort die
/// manuelle Zuordnung an.
fn migrate_unlinked_modules(conn: &mut rusqlite::Connection) -> rusqlite::Result<usize> {
    let pairs: Vec<(i64, String)> = {
        let mut stmt = conn.prepare(
            "SELECT m.id, s.name FROM hangar m \
             JOIN hangar s ON s.pledge_id = m.pledge_id AND s.entity_type != 'module' \
             WHERE m.entity_type = 'module' AND TRIM(m.base_ship) = '' \
               AND TRIM(m.pledge_id) != '' AND TRIM(s.name) != '' \
             GROUP BY m.id HAVING COUNT(DISTINCT s.id) = 1",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    if pairs.is_empty() {
        return Ok(0);
    }
    let tx = conn.transaction()?;
    let now = unix_now() as i64;
    for (id, base) in &pairs {
        tx.execute(
            "UPDATE hangar SET base_ship = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![base, now, id],
        )?;
    }
    tx.commit()?;
    Ok(pairs.len())
}

/// R2 (t_2158474c, Punkt 8): Module sind KEINE eigenständigen Einträge mehr —
/// sie werden nur noch in den Ship-Details eines Schiffs/Fahrzeugs angelegt
/// (base_ship ist dann gesetzt). Manuelle Anlage ohne Basisschiff-Bezug wird
/// abgelehnt; bestehende zeilenbasierte Module (incl. „unverbunden“) bleiben
/// über hangar_update editierbar/zuordenbar.
fn validate_manual_item(item: &HangarItem) -> Result<(), String> {
    if item.entity_type == "module" && item.id.is_none() && item.base_ship.trim().is_empty() {
        return Err(
            "Module werden nicht mehr separat angelegt — im Schiff-Detail unter „Module“ hinzufügen"
                .to_string(),
        );
    }
    Ok(())
}

/// Eintrag manuell anlegen (für Ingame-Käufe, die in keinem Export auftauchen).
#[tauri::command]
pub fn hangar_add(state: State<'_, TradeState>, item: HangarItem) -> Result<HangarItem, String> {
    if item.name.trim().is_empty() {
        return Err("Name darf nicht leer sein".to_string());
    }
    validate_manual_item(&item)?;
    let item = HangarItem {
        name: item.name.trim().to_string(),
        // Manuelle Eingabe — höchste Namens-Priorität.
        source: "manual".to_string(),
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
pub fn hangar_update(state: State<'_, TradeState>, item: HangarItem) -> Result<HangarItem, String> {
    let id = item.id.ok_or("hangar_update ohne id")?;
    let cache = &state.inner().cache;
    let now = unix_now() as i64;
    // Existiert der Datensatz? Dann Feldweise übernehmen (id bleibt).
    let current = hangar_get_by_id(cache, id)?;
    let merged = HangarItem {
        id: Some(id),
        // Manuelle Bearbeitung — höchste Namens-Priorität (schützt manuelle
        // Umbenennungen vor dem nächsten Import).
        source: "manual".to_string(),
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
pub fn hangar_delete(state: State<'_, TradeState>, id: i64) -> Result<(), String> {
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
    state: State<'_, TradeState>,
    format: String,
    text: String,
) -> Result<HangarImportResult, String> {
    let (items, parse_skipped) = match format.as_str() {
        "xplorer" => parse_xplorer_json(&text)?,
        "ccugame" => parse_ccugame_csv(&text)?,
        other => return Err(format!("Unbekanntes Import-Format: {other}")),
    };
    // Quellen-Stempel für die Namens-Priorität (manual > xplorer > ccugame).
    let items: Vec<HangarItem> = items
        .into_iter()
        .map(|mut it| {
            it.source = format.clone();
            it
        })
        .collect();
    let now = unix_now() as i64;
    let cache = &state.inner().cache;
    // Batch-Import: pledge-gruppensicheres Matching in EINEM Lauf.
    let (imported, updated) = cache
        .with(|conn| import_items(conn, &items, now))
        .map_err(|e| format!("hangar import: {e}"))?;
    Ok(HangarImportResult {
        imported,
        updated,
        skipped: parse_skipped,
        format,
    })
}

/// Eigenes sauberes Export-JSON (alle Felder inkl. vollständigem insurance).
#[tauri::command]
pub fn hangar_export(state: State<'_, TradeState>) -> Result<String, String> {
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
        // TB (t_78a4a5e5, Punkt 3): Import schreibt keine Preise mehr.
        assert_eq!(rift.purchase_price, None);
        assert_eq!(rift.purchase_date, "2024-07-12");
        assert_eq!(rift.pledge_id, "71394952");
        assert_eq!(rift.manufacturer, "Origin Jumpworks");
        // Nicht-LTI → kein insurance-Feld
        assert!(items[1].insurance.is_none());
        assert_eq!(items[1].purchase_price, None);
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
        // TB (t_78a4a5e5, Punkt 3): CSV-Import schreibt keine Preise mehr.
        assert_eq!(items[0].purchase_price, None);
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
    fn import_parsers_ignore_prices() {
        // TB (t_78a4a5e5, Punkt 3): Importe verwerfen Preisangaben (XPLorer
        // `pledge_cost`, CCUgame `msrp`/`pledge`) — die DB-Spalte bleibt,
        // wird aber nie mehr befüllt.
        let (x, _) = parse_xplorer_json(XPLORER_SAMPLE).unwrap();
        assert!(
            x.iter().all(|i| i.purchase_price.is_none()),
            "XPLorer: keine Preise"
        );
        let (c, _) = parse_ccugame_csv(CCUGAME_SAMPLE).unwrap();
        assert!(
            c.iter().all(|i| i.purchase_price.is_none()),
            "CSV: keine Preise"
        );
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
            source: String::new(),
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
            source: String::new(),
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
            source: String::new(),
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

        // Quellen-Stempel wie im echten hangar_import.
        let stamp = |mut items: Vec<HangarItem>, source: &str| {
            for it in &mut items {
                it.source = source.to_string();
            }
            items
        };
        let xplorer = stamp(xplorer, "xplorer");
        let ccugame = stamp(ccugame, "ccugame");
        let now = 1_792_500_000i64;

        let count = |cache: &cache::Cache| -> usize {
            cache
                .with(|c| c.query_row("SELECT COUNT(*) FROM hangar", [], |r| r.get::<_, i64>(0)))
                .map_err(|e| e.to_string())
                .unwrap() as usize
        };
        let golem_rows = |cache: &cache::Cache| -> Vec<String> {
            cache
                .with(|c| {
                    let mut stmt = c.prepare(
                        "SELECT name FROM hangar WHERE pledge_id = '93874564' ORDER BY name",
                    )?;
                    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
                    rows.collect::<rusqlite::Result<Vec<_>>>()
                })
                .map_err(|e| e.to_string())
                .unwrap()
        };
        let no_dupes = |cache: &cache::Cache| -> bool {
            let dupes: i64 = cache
                .with(|c| {
                    c.query_row(
                        "SELECT COUNT(*) FROM (SELECT pledge_id, LOWER(TRIM(name)) AS n, \
                         COUNT(*) AS k FROM hangar WHERE pledge_id != '' \
                         GROUP BY pledge_id, n HAVING k > 1)",
                        [],
                        |r| r.get(0),
                    )
                })
                .map_err(|e| e.to_string())
                .unwrap();
            dupes == 0
        };
        let name_of_pledge = |cache: &cache::Cache, pledge: &str| -> String {
            cache
                .with(|c| {
                    c.query_row(
                        "SELECT name FROM hangar WHERE pledge_id = ?1",
                        [pledge],
                        |r| r.get(0),
                    )
                })
                .map_err(|e| e.to_string())
                .unwrap()
        };

        // Akzeptanzcheck 1: CCUgame-CSV ALLEIN in leere DB — alle Zeilen,
        // Golem Duo Pack zweizeilig („Golem" darf nicht verschluckt werden).
        let cache1 = cache::Cache::open_in_memory().unwrap();
        cache1
            .with(|c| import_items(c, &ccugame, now))
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(
            count(&cache1),
            ccugame.len(),
            "CCUgame-Only: alle CSV-Zeilen"
        );
        assert_eq!(
            golem_rows(&cache1),
            vec!["Golem".to_string(), "Golem OX".to_string()],
            "CCUgame-Only: Golem Duo Pack bleibt zweizeilig"
        );

        // Akzeptanzcheck 2: CCUgame dann XPLORer — konvergiert ohne
        // Duplikate; ship_code/manufacturer werden angereichert; der
        // XPLorer-Modellname gewinnt (Quellen-Priorität).
        let cache2 = cache::Cache::open_in_memory().unwrap();
        cache2
            .with(|c| import_items(c, &ccugame, now))
            .map_err(|e| e.to_string())
            .unwrap();
        cache2
            .with(|c| import_items(c, &xplorer, now))
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(count(&cache2), ccugame.len(), "CSV→JSON: keine Duplikate");
        assert!(
            no_dupes(&cache2),
            "CSV→JSON: keine doppelten (pledge, name)-Paare"
        );
        let empty_codes: i64 = cache2
            .with(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM hangar WHERE ship_code = ''",
                    [],
                    |r| r.get(0),
                )
            })
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(
            empty_codes, 0,
            "CSV→JSON: ship_code/manufacturer vollständig angereichert"
        );
        assert_eq!(
            name_of_pledge(&cache2, "71394952"),
            "600i Exploration Module",
            "CSV→JSON: XPLorer-Modellname gewinnt"
        );

        // Akzeptanzcheck 3: XPLORer dann CCUgame — bleibt vollzählig, der
        // XPLorer-Modellname wird vom ärmeren CSV-Import NICHT überschrieben.
        let cache3 = cache::Cache::open_in_memory().unwrap();
        cache3
            .with(|c| import_items(c, &xplorer, now))
            .map_err(|e| e.to_string())
            .unwrap();
        cache3
            .with(|c| import_items(c, &ccugame, now))
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(count(&cache3), xplorer.len(), "JSON→CSV: keine Duplikate");
        assert!(
            no_dupes(&cache3),
            "JSON→CSV: keine doppelten (pledge, name)-Paare"
        );
        assert_eq!(
            golem_rows(&cache3).len(),
            2,
            "JSON→CSV: Pack bleibt zweizeilig"
        );
        assert_eq!(
            name_of_pledge(&cache3, "71394952"),
            "600i Exploration Module",
            "JSON→CSV: CCUgame überschreibt den XPLorer-Modellnamen nicht"
        );
    }
    #[test]
    fn ccugame_only_import_keeps_all_pack_ships() {
        // Review-R2-Akzeptanzcheck 1 (synthetisch, läuft OHNE Realdateien):
        // ein Pack mit 2 Schiffen auf EINER pledge_id, CSV-like (kein
        // ship_code) — der alte COUNT==1-Fallback kollabierte hier zu 1 Zeile.
        let cache = cache::Cache::open_in_memory().unwrap();
        let now = 1_792_500_000i64;
        let mk = |name: &str, pledge: &str| HangarItem {
            id: None,
            source: "ccugame".into(),
            name: name.into(),
            origin: "pledge".into(),
            insurance: Some("LTI".into()),
            custom_name: String::new(),
            entity_type: "ship".into(),
            base_ship: String::new(),
            ship_code: String::new(),
            manufacturer: String::new(),
            purchase_price: Some(130.0),
            purchase_date: "2025-12-02".into(),
            pledge_id: pledge.into(),
            pledge_name: "Packs - Golem Duo Pack".into(),
            warbond: false,
            ccud: false,
            created_at: None,
            updated_at: None,
        };
        let items = vec![
            mk("600i Explorer", "71394952"),
            mk("Golem", "93874564"),
            mk("Golem OX", "93874564"),
        ];
        let (ins, upd) = cache
            .with(|c| import_items(c, &items, now))
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!((ins, upd), (3, 0), "CSV-only: alle 3 Zeilen neu angelegt");
        let rows: Vec<String> = cache
            .with(|c| {
                let mut stmt = c.prepare(
                    "SELECT name FROM hangar WHERE pledge_id = '93874564' ORDER BY name",
                )?;
                let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
                rows.collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(
            rows,
            vec!["Golem".to_string(), "Golem OX".to_string()],
            "Pack bleibt zweizeilig"
        );
    }

    #[test]
    fn ccugame_then_xplorer_converges_without_duplicates() {
        // Review-R2-Akzeptanzcheck 2 (synthetisch): erst CSV (Namen weichen
        // von XPLorer ab, kein ship_code), dann JSON — es dürfen weder
        // Duplikate entstehen noch Objekte verloren gehen.
        let cache = cache::Cache::open_in_memory().unwrap();
        let now = 1_792_500_000i64;
        let csv_item = |name: &str, pledge: &str| HangarItem {
            id: None,
            source: "ccugame".into(),
            name: name.into(),
            origin: "pledge".into(),
            insurance: Some("LTI".into()),
            custom_name: String::new(),
            entity_type: "ship".into(),
            base_ship: String::new(),
            ship_code: String::new(),
            manufacturer: String::new(),
            purchase_price: Some(130.0),
            purchase_date: "2025-12-02".into(),
            pledge_id: pledge.into(),
            pledge_name: "Pack".into(),
            warbond: false,
            ccud: false,
            created_at: None,
            updated_at: None,
        };
        let json_item = |name: &str, code: &str, pledge: &str| HangarItem {
            source: "xplorer".into(),
            ship_code: code.into(),
            manufacturer: "Drake".into(),
            purchase_price: Some(150.0),
            ..csv_item(name, pledge)
        };
        let csv = vec![
            csv_item("600i Explorer", "71394952"),
            csv_item("Golem", "93874564"),
            csv_item("Golem OX", "93874564"),
        ];
        let json = vec![
            json_item("600i Exploration Module", "ORIG_600i", "71394952"),
            json_item("Golem", "DRAK_Golem", "93874564"),
            json_item("Golem OX", "DRAK_Golem_OX", "93874564"),
        ];
        cache
            .with(|c| import_items(c, &csv, now))
            .map_err(|e| e.to_string())
            .unwrap();
        cache
            .with(|c| import_items(c, &json, now))
            .map_err(|e| e.to_string())
            .unwrap();
        let rows: Vec<(String, String)> = cache
            .with(|c| {
                let mut stmt =
                    c.prepare("SELECT name, ship_code FROM hangar ORDER BY pledge_id, name")?;
                let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
                rows.collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(
            rows,
            vec![
                ("600i Exploration Module".into(), "ORIG_600i".into()),
                ("Golem".into(), "DRAK_Golem".into()),
                ("Golem OX".into(), "DRAK_Golem_OX".into()),
            ],
            "konvergiert: 3 Zeilen, angereichert, XPLorer-Namen"
        );
    }

    #[test]
    fn manual_name_survives_weaker_import() {
        // Manuell gesetzter Name (source=manual) darf von keinem Import
        // überschrieben werden; XPLorer schlägt CCUgame (Quellen-Priorität).
        let cache = cache::Cache::open_in_memory().unwrap();
        let now = 1_792_500_000i64;
        let base = HangarItem {
            id: None,
            name: "Meine 600i".into(),
            origin: "pledge".into(),
            insurance: None,
            custom_name: String::new(),
            entity_type: "ship".into(),
            base_ship: String::new(),
            ship_code: String::new(),
            manufacturer: String::new(),
            purchase_price: None,
            purchase_date: String::new(),
            pledge_id: "71394952".into(),
            pledge_name: String::new(),
            warbond: false,
            ccud: false,
            source: "manual".into(),
            created_at: None,
            updated_at: None,
        };
        let (id, inserted) = cache
            .with(|c| upsert(c, &base, now))
            .map_err(|e| e.to_string())
            .unwrap();
        assert!(inserted);
        let mut ccugame = base.clone();
        ccugame.id = None;
        ccugame.source = "ccugame".into();
        ccugame.name = "600i Explorer".into();
        ccugame.insurance = Some("3 Monate".into());
        cache
            .with(|c| import_items(c, std::slice::from_ref(&ccugame), now))
            .map_err(|e| e.to_string())
            .unwrap();
        let row: HangarItem = cache
            .with(|c| {
                c.query_row(
                    &format!("SELECT {HANGAR_COLS} FROM hangar WHERE id = ?1"),
                    [id],
                    row_to_item,
                )
            })
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(
            row.name, "Meine 600i",
            "manuellem Namen wird nie überschrieben"
        );
        assert_eq!(
            row.insurance.as_deref(),
            Some("3 Monate"),
            "Anreicherung funktioniert"
        );
        assert_eq!(row.source, "manual", "Quelle bleibt manual");
    }

    #[test]
    fn import_no_longer_writes_pledge_artifacts() {
        // R2 Punkt 7: XPLorer/CCUgame schreiben pledge_name/warbond/ccud nicht
        // mehr (Import-Artefakte, „nicht wichtig“). pledge_id bleibt als
        // interner Merge-Key erhalten (Import-Idempotenz).
        let (items, _) = parse_xplorer_json(XPLORER_SAMPLE).unwrap();
        for it in &items {
            assert_eq!(
                it.pledge_name, "",
                "pledge_name wird nicht mehr geschrieben"
            );
            assert!(!it.warbond, "warbond wird nicht mehr geschrieben");
            assert!(!it.ccud, "ccud wird nicht mehr geschrieben");
        }
        assert_eq!(items[0].pledge_id, "71394952", "pledge_id bleibt Merge-Key");
        let (csv, _) = parse_ccugame_csv(CCUGAME_SAMPLE).unwrap();
        for it in &csv {
            assert_eq!(
                it.pledge_name, "",
                "CSV: pledge_name wird nicht mehr geschrieben"
            );
            assert!(!it.warbond && !it.ccud);
        }
    }

    #[test]
    fn migration_links_unlinked_modules_by_pledge() {
        // TB (t_78a4a5e5, Punkt 2) Migration: Modul ohne base_ship, dessen
        // pledge_id zu genau EINEM Schiff passt → base_ship gesetzt.
        // Bezugslos (kein pledge-Match) bleibt unverbunden. Idempotent.
        let cache = cache::Cache::open_in_memory().unwrap();
        let now = 1_792_500_000i64;
        let ship = |name: &str, pledge: &str| HangarItem {
            id: None,
            source: "xplorer".into(),
            name: name.into(),
            origin: "pledge".into(),
            insurance: Some("LTI".into()),
            custom_name: String::new(),
            entity_type: "ship".into(),
            base_ship: String::new(),
            ship_code: String::new(),
            manufacturer: "Origin".into(),
            purchase_price: None,
            purchase_date: String::new(),
            pledge_id: pledge.into(),
            pledge_name: String::new(),
            warbond: false,
            ccud: false,
            created_at: None,
            updated_at: None,
        };
        let module = |name: &str, pledge: &str, base: &str| HangarItem {
            entity_type: "module".into(),
            base_ship: base.into(),
            ..ship(name, pledge)
        };
        let mk = |i: &HangarItem| -> HangarItem {
            let mut it = i.clone();
            it.id = None;
            it
        };
        // Schiff A mit Pledge P1; zwei Module: M1 (P1, bezugslos → verknüpfbar),
        // M2 (P2, kein Schiff → bleibt unverbunden), M3 (bereits verlinkt).
        let ships = [ship("600i Explorer", "P1")];
        let mods = [
            module("600i Exploration Module", "P1", ""),
            module("Fremd-Modul", "P2", ""),
            module("Bekanntes Modul", "P3", "600i Explorer"),
        ];
        for it in ships.iter().chain(mods.iter()) {
            cache
                .with(|c| upsert(c, &mk(it), now))
                .map_err(|e| e.to_string())
                .unwrap();
        }
        let linked = cache
            .with(migrate_unlinked_modules)
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(linked, 1, "genau ein bezugsloses Modul verknüpft");
        let base_of = |name: &str| -> String {
            cache
                .with(|c| {
                    c.query_row(
                        "SELECT base_ship FROM hangar WHERE name = ?1 AND entity_type = 'module'",
                        [name],
                        |r| r.get(0),
                    )
                })
                .map_err(|e| e.to_string())
                .unwrap()
        };
        assert_eq!(
            base_of("600i Exploration Module"),
            "600i Explorer",
            "per pledge verknüpft"
        );
        assert_eq!(
            base_of("Fremd-Modul"),
            "",
            "ohne pledge-Match bleibt unverbunden"
        );
        assert_eq!(
            base_of("Bekanntes Modul"),
            "600i Explorer",
            "Bestands-Link unangetastet"
        );
        // Idempotenz: zweiter Lauf verändert nichts mehr.
        let again = cache
            .with(migrate_unlinked_modules)
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(again, 0, "zweiter Lauf: kein weiteres Update");
        // Mehrere Schiffe in EINER Pledge-Gruppe: keine automatische Wahl —
        // das Modul bleibt unverbunden (Mehrdeutigkeit nicht erraten).
        cache
            .with(|c| upsert(c, &mk(&ship("Golem", "P4")), now))
            .map_err(|e| e.to_string())
            .unwrap();
        cache
            .with(|c| upsert(c, &mk(&ship("Golem OX", "P4")), now))
            .map_err(|e| e.to_string())
            .unwrap();
        cache
            .with(|c| upsert(c, &mk(&module("Pack-Modul", "P4", "")), now))
            .map_err(|e| e.to_string())
            .unwrap();
        let linked2 = cache
            .with(migrate_unlinked_modules)
            .map_err(|e| e.to_string())
            .unwrap();
        assert_eq!(linked2, 0, "mehrdeutige Pledge-Gruppe bleibt unverbunden");
        assert_eq!(base_of("Pack-Modul"), "");
    }

    #[test]
    fn manual_module_without_base_ship_rejected() {
        // R2 Punkt 8: Module sind keine eigenständigen Einträge mehr — manuelle
        // Anlage ohne Basisschiff-Bezug wird abgelehnt, MIT Bezug erlaubt.
        let mut m = hangar_blank_like_module();
        assert!(
            validate_manual_item(&m).is_err(),
            "ohne base_ship abgelehnt"
        );
        m.base_ship = "600i Explorer".into();
        assert!(validate_manual_item(&m).is_ok(), "mit base_ship erlaubt");
        // Bestehende Modul-Zeile (id) ohne base_ship: über hangar_update
        // editierbar (Zuordnungspfad) — keine Ablehnung.
        m.id = Some(42);
        m.base_ship = String::new();
        assert!(
            validate_manual_item(&m).is_ok(),
            "Bestand bleibt editierbar"
        );
    }

    fn hangar_blank_like_module() -> HangarItem {
        HangarItem {
            id: None,
            source: "manual".into(),
            name: "Test-Modul".into(),
            origin: "pledge".into(),
            insurance: None,
            custom_name: String::new(),
            entity_type: "module".into(),
            base_ship: String::new(),
            ship_code: String::new(),
            manufacturer: String::new(),
            purchase_price: None,
            purchase_date: String::new(),
            pledge_id: String::new(),
            pledge_name: String::new(),
            warbond: false,
            ccud: false,
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    #[ignore = "echte DB: R2_REAL_DB=<pfad zur cache.sqlite3-Kopie>"]
    fn r2_migration_real_db_modules() {
        // R2 Migrationstest gegen eine KOPIE der echten Cache-DB: Das
        // zeilenbasierte Module-Modell bleibt unverändert (Schema v4) — die
        // Migration ist datenseitig ein No-Op; verifiziert wird:
        //   1. hangar_list öffnet die bestehende DB ohne Fehler,
        //   2. Module mit base_ship-Bezug verweisen auf ein existierendes
        //      Schiff/Fahrzeug (Detail-Anzeige klappt),
        //   3. bezugslose Module existieren („Modul (unverbunden)“-Stapel)
        //      bzw. der Bestand ist vollständig gezählt.
        let path = std::path::PathBuf::from(
            std::env::var("R2_REAL_DB").expect("R2_REAL_DB setzen (DB-Kopie)"),
        );
        let cache = cache::Cache::open(&path).unwrap();
        let items: Vec<HangarItem> = cache
            .with(|c| {
                let mut stmt = c.prepare(&format!(
                    "SELECT {HANGAR_COLS} FROM hangar ORDER BY entity_type, name"
                ))?;
                let rows = stmt.query_map([], row_to_item)?;
                rows.collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap();
        let ship_names: std::collections::HashSet<String> = items
            .iter()
            .filter(|i| i.entity_type != "module")
            .map(|i| norm_name(&i.name))
            .collect();
        let modules: Vec<&HangarItem> =
            items.iter().filter(|i| i.entity_type == "module").collect();
        let mut linked = 0;
        let mut unlinked = 0;
        for m in &modules {
            if m.base_ship.trim().is_empty() || !ship_names.contains(&norm_name(&m.base_ship)) {
                unlinked += 1;
            } else {
                linked += 1;
            }
        }
        println!(
            "r2_migration_real_db_modules: {} Einträge, {} Module ({} verlinkt, {} unverbunden)",
            items.len(),
            modules.len(),
            linked,
            unlinked
        );
        assert_eq!(
            linked + unlinked,
            modules.len(),
            "alle Module klassifiziert"
        );
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
        let mut xs = xplorer;
        for it in &mut xs {
            it.source = "xplorer".to_string();
        }
        let mut cs = ccugame;
        for it in &mut cs {
            it.source = "ccugame".to_string();
        }
        cache
            .with(|c| import_items(c, &xs, now))
            .map_err(|e| e.to_string())
            .unwrap();
        cache
            .with(|c| import_items(c, &cs, now))
            .map_err(|e| e.to_string())
            .unwrap();
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
