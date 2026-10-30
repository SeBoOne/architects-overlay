//! # cache::modul_compat
//!
//! Schiffs-Modul-Zuweisung (S4, Karte t_66980b3c, Schema v12): welche
//! Module akzeptiert welches Schiff — AUSSCHLIESSLICH aus
//! `module-overrides.json` (Repo-Baseline + Remote-Override von der
//! Gitea-Raw-URL, siehe `scwiki_sync`). Die SC-Wiki-API liefert KEINE
//! Modul-Daten mehr (Hardpoints/Ports und Item-Katalog sind raus).
//!
//! Snapshot-Semantik: [`Cache::modul_compat_store`] ersetzt den kompletten
//! Datensatz (ein Sync = die aktuelle Datei). Versionierung über den
//! `meta`-Key `modul_compat:version` (Quelle-Label) + `modul_compat:synced_at`.
//!
//! Reads:
//! * [`Cache::modul_compat_state`] — Datensatz + Version + Alter.
//! * [`Cache::modul_modules_for_ship`] — `None` = Schiff hat keinen Eintrag
//!   in der Datei → kein Modul-Feld (S4 Nur-Datei-Regel); `Some(list)` =
//!   die Datei-Module des Schiffs.

use std::time::{SystemTime, UNIX_EPOCH};

use super::{Cache, CacheError, Result};

/// `meta`-Key: Quelle/Version, für die der Datensatz gilt (S4: immer das
/// Quelle-Label „module-overrides.json“ — kein Game-Version-Trigger mehr).
pub const META_VERSION: &str = "modul_compat:version";
/// `meta`-Key: Sync-Zeitpunkt (Epoch-Sekunden).
pub const META_SYNCED_AT: &str = "modul_compat:synced_at";
/// `meta`-Key: Remote-Override-Text (`module-overrides.json` vom Gitea-Raw).
/// Zwischen-Syncs gecacht; Fallback beim Lesen ist die Repo-Baseline.
pub const META_OVERRIDES: &str = "modul_compat:overrides";

/// Eine Modul-Zuordnungs-Zeile.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ModulCompatRow {
    pub name: String,
    /// Deutsches Kategorie-Label (z. B. „Bombrack“, „Weapon-Kit“, „Modul“).
    pub category: String,
    /// Schiffsnamen (exakt wie in der Datei gepflegt), die das Modul annehmen.
    pub ships: Vec<String>,
}

/// Vollständiger gespeicherter Datensatz.
#[derive(Clone, Debug)]
pub struct ModulCompatState {
    pub version: String,
    pub synced_at: u64,
    pub rows: Vec<ModulCompatRow>,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Case-insensitive Schiffsnamen-Normalisierung für den Match gegen Hangar-
/// Einträge (Umlaute/Whitespace tolerant — exakte Datei-Namen sind die
/// Referenz, der Match ist nur Case/Whitespace-tolerant).
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
    /// Ersetzt den kompletten Datensatz (Snapshot der Zuweisungs-Datei).
    pub fn modul_compat_store(&self, version: &str, rows: &[ModulCompatRow]) -> Result<()> {
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
            for (key, value) in [
                (META_VERSION, version.to_string()),
                (META_SYNCED_AT, now.to_string()),
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

    /// Remote-Override-Cache verwerfen (Tests).
    pub fn modul_overrides_cache_clear(&self) -> Result<()> {
        self.with(|conn| {
            conn.execute("DELETE FROM meta WHERE key = ?1", [META_OVERRIDES])?;
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
            Ok(Some(ModulCompatState {
                version,
                synced_at,
                rows: out,
            }))
        })
    }

    /// Module, die `ship` laut Zuweisungs-Datei angenommen werden.
    ///
    /// * `Ok(None)` — Schiff hat keinen Eintrag in der Datei: KEIN
    ///   Modul-Feld (S4 Nur-Datei-Regel, kein Fallschirm mehr).
    /// * `Ok(Some(list))` — die Datei-Module des Schiffs (nicht leer).
    pub fn modul_modules_for_ship(&self, ship: &str) -> Result<Option<Vec<ModulCompatRow>>> {
        let needle = norm(ship);
        if needle.is_empty() {
            return Ok(None);
        }
        let state = self.modul_compat_state()?;
        let Some(state) = state else { return Ok(None) };
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

    /// Normalisierte Namen aller Schiffe mit ≥ 1 Zuweisung in der Datei.
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
                "module-overrides.json",
                &[
                    row("Aurora MK II - DM Module", "Modul", &["RSI Aurora MK II"]),
                    row("5CA 'Akura'", "Schild", &["RSI Galaxy"]),
                ],
            )
            .unwrap();
        let state = cache.modul_compat_state().unwrap().unwrap();
        assert_eq!(state.version, "module-overrides.json");
        assert_eq!(state.rows.len(), 2);
        assert!(state.synced_at > 0);

        // Erneuter Store ersetzt (Snapshot, kein Anwachsen) — S4-Migration:
        // alte API-Zeilen verfallen dabei.
        cache
            .modul_compat_store("module-overrides.json", &[row("X", "Modul", &["Y"])])
            .unwrap();
        let state = cache.modul_compat_state().unwrap().unwrap();
        assert_eq!(state.rows.len(), 1);
    }

    #[test]
    fn modules_for_ship_matches_normalized() {
        let cache = Cache::open_in_memory().unwrap();
        cache
            .modul_compat_store(
                "module-overrides.json",
                &[
                    row("Aurora MK II - DM Module", "Modul", &["RSI Aurora MK II"]),
                    row("Aurora MK II - TS Module", "Modul", &["RSI Aurora MK II"]),
                    row("Galaxy - Cargo Module", "Modul", &["RSI Galaxy"]),
                ],
            )
            .unwrap();

        let aur = cache
            .modul_modules_for_ship("rsi aurora   mk ii")
            .unwrap()
            .unwrap();
        assert_eq!(aur.len(), 2);
        assert_eq!(
            cache
                .modul_modules_for_ship("RSI Galaxy")
                .unwrap()
                .unwrap()
                .len(),
            1
        );
        // Schiff ohne Datei-Eintrag → None → kein Modul-Feld (S4).
        assert!(
            cache
                .modul_modules_for_ship("Aegis Arrow")
                .unwrap()
                .is_none()
        );
        assert!(cache.modul_modules_for_ship("").unwrap().is_none());
    }

    #[test]
    fn ships_with_ports_dedupes() {
        let cache = Cache::open_in_memory().unwrap();
        cache
            .modul_compat_store(
                "module-overrides.json",
                &[
                    row("A", "Modul", &["RSI Aurora MK II", "rsi aurora mk ii"]),
                    row("B", "Modul", &["RSI Galaxy"]),
                ],
            )
            .unwrap();
        let ships = cache.modul_ships_with_ports().unwrap();
        assert_eq!(ships.len(), 2);
    }
}
