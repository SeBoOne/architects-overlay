//! # cache::modul_compat
//!
//! Schiffs-Modul-Kompatibilität (S3, Karte t_988e132f, Schema v12): welche
//! Module akzeptiert welches Schiff — cross-referenziert aus der SC-Wiki-API
//! (Port-Hardpoints × Item-Katalog, siehe `scwiki_client::build_compat`).
//!
//! Snapshot-Semantik: [`Cache::modul_compat_store`] ersetzt den kompletten
//! Datensatz (eine Game-Version = ein Sync, Etikette Punkt 6). Versionierung
//! über `meta`-Keys `modul_compat:version` + `modul_compat:synced_at` — der
//! Background-Sync prüft stündlich `/api/game-versions/default` und resynced
//! nur bei Versionswechsel (oder leerer Tabelle = erster Lauf).
//!
//! Reads:
//! * [`Cache::modul_compat_state`] — Datensatz + Version + Alter.
//! * [`Cache::modul_modules_for_ship`] — `None` = Schiff ist der Zuordnung
//!   unbekannt (Fallschirm: Frontend bietet alle Module an); `Some(list)` =
//!   passende Module (leere Liste = Schiff ohne Ports → kein Modulfeld).
//! * [`Cache::modul_ships_with_ports`] — Schiffe mit ≥ 1 Port.

use std::time::{SystemTime, UNIX_EPOCH};

use super::{Cache, CacheError, Result};

/// `meta`-Key: Game-Version, für die der Datensatz gilt.
pub const META_VERSION: &str = "modul_compat:version";
/// `meta`-Key: Sync-Zeitpunkt (Epoch-Sekunden).
pub const META_SYNCED_AT: &str = "modul_compat:synced_at";
/// `meta`-Key: JSON-Liste der Vehicle-Namen OHNE Ports (dort versteckt das
/// Frontend das Modul-Eingabefeld — Karte S3 Punkt 4).
pub const META_NO_PORTS: &str = "modul_compat:noport_ships";
/// `meta`-Key: Remote-Override-Text (`module-overrides.json` vom Gitea-Raw,
/// S3/D2). Zwischen-Syncs gecacht; Fallback beim Lesen ist die Repo-Baseline.
pub const META_OVERRIDES: &str = "modul_compat:overrides";

/// Eine Modul-Zuordnungs-Zeile.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ModulCompatRow {
    pub name: String,
    /// Deutsches Kategorie-Label (z. B. „Schild", „Modul").
    pub category: String,
    /// Schiffsnamen (SC-Wiki-Vehicle-Namen), die das Modul akzeptieren.
    pub ships: Vec<String>,
}

/// Vollständiger gespeicherter Datensatz.
#[derive(Clone, Debug)]
pub struct ModulCompatState {
    pub version: String,
    pub synced_at: u64,
    pub rows: Vec<ModulCompatRow>,
    /// Vehicle-Namen ohne Ports (leer = kein Modulfeld-Versteck möglich).
    pub no_port_ships: Vec<String>,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Case-insensitive Schiffsnamen-Normalisierung für den Match gegen Hangar-
/// Einträge (Umlaute/Whitespace tolerant, „Mk II"-Rauschen bleibt drin —
/// exakte Vehicle-Namen der SC-Wiki sind die Referenz).
fn norm(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// serde_json-Fehler innerhalb der `with`-Closure (die Result<rslite::Error>
/// liefert) in einen rusqlite-Fehler umwandeln.
fn rusqlite_err(e: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(e))
}

impl Cache {
    /// Ersetzt den kompletten Datensatz (Snapshot einer Game-Version).
    pub fn modul_compat_store(
        &self,
        version: &str,
        rows: &[ModulCompatRow],
        no_port_ships: &[impl AsRef<str>],
    ) -> Result<()> {
        if version.trim().is_empty() {
            return Err(CacheError::Open("modul_compat_store: leere Version".into()));
        }
        self.with(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute("DELETE FROM modul_compat", [])?;
            {
                let mut stmt = tx.prepare(
                    "INSERT INTO modul_compat (module_name, category, ships) VALUES (?1, ?2, ?3)",
                )?;
                for row in rows {
                    let ships = serde_json::to_string(&row.ships).map_err(rusqlite_err)?;
                    stmt.execute(rusqlite::params![row.name, row.category, ships])?;
                }
            }
            let now = now_unix();
            let no_ports_list: Vec<String> = no_port_ships
                .iter()
                .map(|s| s.as_ref().to_string())
                .collect();
            let no_ports = serde_json::to_string(&no_ports_list).map_err(rusqlite_err)?;
            for (key, value) in [
                (META_VERSION, version.to_string()),
                (META_SYNCED_AT, now.to_string()),
                (META_NO_PORTS, no_ports),
            ] {
                tx.execute(
                    "INSERT INTO meta (key, value) VALUES (?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    rusqlite::params![key, value],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    /// Remote-Override-Text (S3/D2): letzter erfolgreich geforeter
    /// `module-overrides.json`-Inhalt von der Gitea-Raw-URL.
    pub fn modul_overrides_cache_get(&self) -> Result<Option<String>> {
        self.with(|conn| {
            let mut stmt = conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
            let mut rows = stmt.query([META_OVERRIDES])?;
            match rows.next()? {
                Some(row) => Ok(row.get::<_, Option<String>>(0)?),
                None => Ok(None),
            }
        })
    }

    /// Remote-Override-Text cachen (nach erfolgreichem Fetch + Parse).
    pub fn modul_overrides_cache_set(&self, text: &str) -> Result<()> {
        self.with(|conn| {
            conn.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                rusqlite::params![META_OVERRIDES, text],
            )?;
            Ok(())
        })
    }

    /// Gesamter Datensatz inkl. Version + Alter (`None` = noch nie gesynced).
    pub fn modul_compat_state(&self) -> Result<Option<ModulCompatState>> {
        self.with(|conn| {
            let mut stmt = conn.prepare("SELECT module_name, category, ships FROM modul_compat")?;
            let mut rows = stmt.query([])?;
            let mut out = Vec::new();
            while let Some(r) = rows.next()? {
                let ships_json: String = r.get(2)?;
                let ships: Vec<String> = serde_json::from_str(&ships_json).map_err(rusqlite_err)?;
                out.push(ModulCompatRow {
                    name: r.get(0)?,
                    category: r.get(1)?,
                    ships,
                });
            }
            if out.is_empty() {
                // Kein Datensatz — auch keine Meta-Keys akzeptieren.
                return Ok(None);
            }
            let meta = |key: &str| -> rusqlite::Result<Option<String>> {
                let mut stmt = conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
                let mut rows = stmt.query([key])?;
                match rows.next()? {
                    Some(row) => row.get::<_, Option<String>>(0),
                    None => Ok(None),
                }
            };
            let version = meta(META_VERSION)?.unwrap_or_default();
            let synced_at: u64 = meta(META_SYNCED_AT)?
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let no_port_ships = meta(META_NO_PORTS)?
                .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
                .unwrap_or_default();
            Ok(Some(ModulCompatState {
                version,
                synced_at,
                rows: out,
                no_port_ships,
            }))
        })
    }

    /// Module, die `ship` laut Zuordnung akzeptiert.
    ///
    /// * `Ok(None)` — Zuordnung unbekannt (leer oder Schiff nicht gefunden):
    ///   Fallschirm, Frontend bietet alle Module an.
    /// * `Ok(Some(list))` — passende Module; leere Liste = Schiff bekannt,
    ///   aber ohne Ports bzw. ohne passendes Modul im Katalog.
    pub fn modul_modules_for_ship(&self, ship: &str) -> Result<Option<Vec<ModulCompatRow>>> {
        let needle = norm(ship);
        if needle.is_empty() {
            return Ok(None);
        }
        let state = self.modul_compat_state()?;
        let Some(state) = state else { return Ok(None) };
        // Schiff OHNE Ports → bekannt, aber leere Modulliste (Karte S3
        // Punkt 4: kein Modulfeld für diese Schiffe).
        if state.no_port_ships.iter().any(|s| norm(s) == needle) {
            return Ok(Some(Vec::new()));
        }
        let mut out = Vec::new();
        for row in &state.rows {
            if row.ships.iter().any(|s| norm(s) == needle) {
                out.push(row.clone());
            }
        }
        if out.is_empty() {
            Ok(None)
        } else {
            Ok(Some(out))
        }
    }

    /// Normalisierte Namen aller Schiffe, die laut Zuordnung ≥ 1 Port haben.
    pub fn modul_ships_with_ports(&self) -> Result<Vec<String>> {
        let state = self.modul_compat_state()?;
        let Some(state) = state else {
            return Ok(Vec::new());
        };
        let mut seen = std::collections::BTreeSet::new();
        for row in &state.rows {
            for s in &row.ships {
                seen.insert(norm(s));
            }
        }
        Ok(seen.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, category: &str, ships: &[&str]) -> ModulCompatRow {
        ModulCompatRow {
            name: name.into(),
            category: category.into(),
            ships: ships.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn store_and_state_roundtrip() {
        let cache = Cache::open_in_memory().unwrap();
        assert!(cache.modul_compat_state().unwrap().is_none());

        cache
            .modul_compat_store(
                "4.10.2-LIVE.12881860",
                &[
                    row("Aurora Mk II DM Module", "Modul", &["Aurora Mk II"]),
                    row("5CA 'Akura'", "Schild", &["C2 Hercules", "Aurora Mk II"]),
                ],
                &["Hull E"],
            )
            .unwrap();
        let state = cache.modul_compat_state().unwrap().unwrap();
        assert_eq!(state.version, "4.10.2-LIVE.12881860");
        assert_eq!(state.rows.len(), 2);
        assert_eq!(state.no_port_ships, vec!["Hull E".to_string()]);
        assert!(state.synced_at > 0);

        // Erneuter Store ersetzt (Snapshot, kein Anwachsen).
        cache
            .modul_compat_store(
                "4.11.0-LIVE.1",
                &[row("X", "Modul", &["Y"])],
                &[] as &[&str],
            )
            .unwrap();
        let state = cache.modul_compat_state().unwrap().unwrap();
        assert_eq!(state.version, "4.11.0-LIVE.1");
        assert_eq!(state.rows.len(), 1);
        assert!(state.no_port_ships.is_empty());
    }

    #[test]
    fn modules_for_ship_matches_normalized() {
        let cache = Cache::open_in_memory().unwrap();
        cache
            .modul_compat_store(
                "v1",
                &[
                    row("Aurora Mk II DM Module", "Modul", &["Aurora Mk II"]),
                    row("Aurora Mk II TS Module", "Modul", &["Aurora Mk II"]),
                    row("5CA 'Akura'", "Schild", &["C2 Hercules"]),
                ],
                &["Hull E"],
            )
            .unwrap();

        let aur = cache
            .modul_modules_for_ship("aurora   mk ii")
            .unwrap()
            .unwrap();
        assert_eq!(aur.len(), 2);
        // Schiff mit Ports und passendem Modul → genau die Zuordnungszeile.
        assert_eq!(
            cache
                .modul_modules_for_ship("C2 Hercules")
                .unwrap()
                .unwrap()
                .len(),
            1
        );
        // Schiff OHNE Ports → Some(leer) → kein Modulfeld.
        assert_eq!(
            cache
                .modul_modules_for_ship("Hull E")
                .unwrap()
                .unwrap()
                .len(),
            0
        );
        // Unbekanntes Schiff (nicht in Zuordnung) → None (Fallschirm: alle).
        assert!(cache.modul_modules_for_ship("Kraken").unwrap().is_none());
        assert!(cache.modul_modules_for_ship("").unwrap().is_none());
    }

    #[test]
    fn ships_with_ports_dedupes() {
        let cache = Cache::open_in_memory().unwrap();
        cache
            .modul_compat_store(
                "v1",
                &[
                    row("A", "Modul", &["Aurora Mk II", "aurora mk ii"]),
                    row("B", "Schild", &["C2 Hercules"]),
                ],
                &[] as &[&str],
            )
            .unwrap();
        let ships = cache.modul_ships_with_ports().unwrap();
        assert_eq!(ships.len(), 2);
    }
}
