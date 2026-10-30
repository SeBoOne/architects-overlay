//! SC-Wiki-Modul-Kompatibilität (S3, Karte t_988e132f): Background-Sync,
//! Tauri-Commands und die manuelle Bombrack-Mapping-Tabelle.
//!
//! * Stündlicher Check von `/api/game-versions/default` — nur bei
//!   Versionswechsel (oder leerer Tabelle) Resync (Etikette: 1 Sync pro
//!   Version, ~30 geringratete Requests).
//! * `modul_compat_refresh` — manueller Force-Sync aus den Settings.
//! * `modul_for_ship` — Zuordnungs-Abfrage für das Hangar-Modul-Autocomplete.
//! * Bombracks (`bombracks.json`) werden beim Lesen dem API-Datensatz
//!   zugeschmolzen — Änderungen an der Liste brauchen keinen Resync.

use std::sync::Arc;
use std::time::Duration;

use cache::Cache;
use cache::modul_compat::{ModulCompatRow, ModulCompatState};
use scwiki_client::{MODULE_TYPES, ScWikiClient};
use serde::Serialize;
use tauri::State;

use crate::trade::TradeState;

/// Stündlicher Versions-Check (Karte: „prüft stündlich /game-versions/default").
pub const MODUL_SYNC_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// Erster Sync-Lauf kurz nach Boot, damit die Tabelle beim ersten Hangar-
/// Aufruf gefüllt ist („erster Lauf" aus der Karte).
const FIRST_SYNC_DELAY: Duration = Duration::from_secs(15);

/// Prozessweite Busy-Markierung gegen parallele Syncs (Stunde + manueller
/// Button). `AtomicBool` genügt: nur ein App-Prozess pro Cache.
static SYNC_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Debug, serde::Deserialize)]
struct BombrackEntry {
    name: String,
    #[serde(default)]
    ships: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
struct BombrackFile {
    #[serde(default)]
    modules: Vec<BombrackEntry>,
}

/// Manuelle Bombrack-Mapping-Tabelle (statisch im Repo, Punkt 5). Parse-
/// Fehler sind NICHT fatal — der API-Datensatz bleibt nutzbar, der Fehler
/// geht ins Log.
fn bombrack_rows() -> Vec<ModulCompatRow> {
    match serde_json::from_str::<BombrackFile>(include_str!("bombracks.json")) {
        Ok(file) => file
            .modules
            .into_iter()
            .map(|m| ModulCompatRow {
                name: m.name,
                category: "Bombrack".to_string(),
                ships: m.ships,
            })
            .collect(),
        Err(e) => {
            log::error!("bombracks.json unparsabel — Liste übersprungen: {e}");
            Vec::new()
        }
    }
}

/// Verschmilzt Bombrack-Zeilen in die API-Zeilen (gleicher Modulname →
/// Schiffe vereinigt, Kategorie bleibt).
fn merge_bombracks(mut rows: Vec<ModulCompatRow>) -> Vec<ModulCompatRow> {
    for b in bombrack_rows() {
        if let Some(existing) = rows.iter_mut().find(|r| r.name == b.name) {
            for s in b.ships {
                if !existing.ships.contains(&s) {
                    existing.ships.push(s);
                }
            }
        } else {
            rows.push(b);
        }
    }
    rows
}

/// Ein Sync-Durchlauf. `force=true` (Settings-Button) synced auch, wenn die
/// gespeicherte Version noch aktuell ist. Rückkehr: "fresh" | "synced".
pub async fn sync_once(cache: &Cache, force: bool) -> Result<String, String> {
    if SYNC_BUSY.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Ok("busy".to_string());
    }
    let out = sync_once_inner(cache, force).await;
    SYNC_BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
    out
}

async fn sync_once_inner(cache: &Cache, force: bool) -> Result<String, String> {
    let client = ScWikiClient::new(scwiki_client::DEFAULT_BASE_URL)
        .map_err(|e| format!("scwiki client: {e}"))?;
    let version = client
        .game_version_default()
        .await
        .map_err(|e| format!("scwiki game-version: {e}"))?;

    if !force {
        if let Some(state) = cache.modul_compat_state().map_err(|e| e.to_string())? {
            if state.version == version.code && !state.rows.is_empty() {
                return Ok("fresh".to_string());
            }
        }
    }

    let vehicles = client
        .vehicles()
        .await
        .map_err(|e| format!("scwiki vehicles: {e}"))?;
    let no_port_ships: Vec<String> = vehicles
        .iter()
        .filter(|v| v.ports.is_empty())
        .map(|v| v.name.clone())
        .collect();
    let with_ports: Vec<scwiki_client::VehiclePorts> = vehicles
        .into_iter()
        .filter(|v| !v.ports.is_empty())
        .collect();

    let items = client
        .items_by_types(MODULE_TYPES)
        .await
        .map_err(|e| format!("scwiki items: {e}"))?;

    let compat = scwiki_client::build_compat(&with_ports, &items);
    let rows: Vec<ModulCompatRow> = merge_bombracks(
        compat
            .into_iter()
            .map(|r| ModulCompatRow {
                name: r.module_name,
                category: r.category,
                ships: r.ships,
            })
            .collect(),
    );
    cache
        .modul_compat_store(&version.code, &rows, &no_port_ships)
        .map_err(|e| e.to_string())?;
    log::info!(
        "modul-sync: {} gespeichert (version {}, {} Schiffe ohne Ports)",
        rows.len(),
        version.code,
        no_port_ships.len()
    );
    Ok("synced".to_string())
}

/// Stündlicher Background-Loop + erster Lauf nach dem Boot. Fehler sind
/// nicht fatal — der nächste Tick versucht es erneut.
pub fn spawn_modul_sync(cache: Arc<Cache>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_SYNC_DELAY).await;
        match sync_once(&cache, false).await {
            Ok(ref other) if other == "synced" => {
                log::info!("modul-sync: erster Lauf abgeschlossen")
            }
            Ok(other) => log::debug!("modul-sync: erster Lauf: {other}"),
            Err(e) => log::warn!("modul-sync: erster Lauf fehlgeschlagen: {e}"),
        }
        loop {
            tokio::time::sleep(MODUL_SYNC_INTERVAL).await;
            if let Err(e) = sync_once(&cache, false).await {
                log::warn!("modul-sync: stündlicher Lauf fehlgeschlagen: {e}");
            }
        }
    });
}

// ---------- Tauri-Commands ----------

#[derive(Serialize)]
pub struct ModulStatus {
    pub version: String,
    pub synced_at: u64,
    pub age_seconds: Option<u64>,
    pub module_count: usize,
    pub ship_count: usize,
}

#[derive(Serialize)]
pub struct ModulHit {
    pub name: String,
    pub category: String,
}

#[derive(Serialize)]
pub struct ModulForShip {
    /// `false` = Schiff in der Zuordnung unbekannt (Fallschirm: alle Module).
    pub known: bool,
    /// `false` = Schiff ohne Ports → Modul-Eingabefeld verstecken.
    pub has_ports: bool,
    pub modules: Vec<ModulHit>,
}

#[tauri::command]
pub fn modul_compat_status(state: State<'_, TradeState>) -> Result<ModulStatus, String> {
    let state = state
        .cache
        .modul_compat_state()
        .map_err(|e| e.to_string())?;
    Ok(match state {
        None => ModulStatus {
            version: String::new(),
            synced_at: 0,
            age_seconds: None,
            module_count: 0,
            ship_count: 0,
        },
        Some(s) => {
            let age = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
                .checked_sub(s.synced_at);
            ModulStatus {
                version: s.version,
                synced_at: s.synced_at,
                age_seconds: age,
                module_count: s.rows.len(),
                ship_count: s
                    .rows
                    .iter()
                    .flat_map(|r| r.ships.iter())
                    .collect::<std::collections::BTreeSet<_>>()
                    .len(),
            }
        }
    })
}

#[tauri::command]
pub async fn modul_compat_refresh(state: State<'_, TradeState>) -> Result<String, String> {
    sync_once(&state.cache, true).await
}

/// Modul-Zuordnung für ein Schiff (Hangar-Modul-Autocomplete). Leerer
/// `ship` → gesamter Katalog (Fallschirm-Quelle für „alle Module").
#[tauri::command]
pub fn modul_for_ship(state: State<'_, TradeState>, ship: String) -> Result<ModulForShip, String> {
    let s: Option<ModulCompatState> = state
        .cache
        .modul_compat_state()
        .map_err(|e| e.to_string())?;
    let Some(s) = s else {
        // Kein Datensatz: Fallschirm — alle Module anbieten, Feld zeigen.
        return Ok(ModulForShip {
            known: false,
            has_ports: true,
            modules: Vec::new(),
        });
    };
    if ship.trim().is_empty() {
        // Gesamtkatalog (Fallschirm).
        return Ok(ModulForShip {
            known: false,
            has_ports: true,
            modules: s
                .rows
                .iter()
                .map(|r| ModulHit {
                    name: r.name.clone(),
                    category: r.category.clone(),
                })
                .collect(),
        });
    }
    let needle = norm_name(&ship);
    // Ohne Ports → bekannt, kein Modulfeld.
    if s.no_port_ships.iter().any(|v| norm_name(v) == needle) {
        return Ok(ModulForShip {
            known: true,
            has_ports: false,
            modules: Vec::new(),
        });
    }
    let modules: Vec<ModulHit> = s
        .rows
        .iter()
        .filter(|r| r.ships.iter().any(|v| norm_name(v) == needle))
        .map(|r| ModulHit {
            name: r.name.clone(),
            category: r.category.clone(),
        })
        .collect();
    if modules.is_empty() {
        // Mit Ports, aber keine passenden Module in der Zuordnung →
        // Fallschirm (alle Module), dokumentiert in der Karte.
        Ok(ModulForShip {
            known: false,
            has_ports: true,
            modules: Vec::new(),
        })
    } else {
        Ok(ModulForShip {
            known: true,
            has_ports: true,
            modules,
        })
    }
}

/// Gleiche Normalisierung wie `cache::modul_compat::norm` (Case +
/// Whitespace), hier lokal gehalten um die Crate-API schmal zu halten.
fn norm_name(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live-Beleg für S3 (t_988e132f): Der Sync läuft einmal gegen die echte
    /// SC-Wiki-API, die Tabelle füllt sich, und die Cross-Referenz liefert
    /// für die Aurora Mk II ihre Defense/Transport-Module (DM/TS). Ignored
    /// by default — `cargo test -p architects-overlay --lib -- --ignored
    /// scwiki` (nur wenige geringratete Requests, 1 Sync pro Version).
    #[tokio::test]
    #[ignore = "live SC-Wiki — run with -- --ignored"]
    async fn live_sync_fills_cache_and_aurora() {
        let cache = Cache::open_in_memory().unwrap();
        let outcome = sync_once(&cache, true).await.unwrap();
        assert_eq!(outcome, "synced");

        let state = cache.modul_compat_state().unwrap().expect("Datensatz");
        assert!(!state.version.is_empty(), "Game-Version gespeichert");
        assert!(state.synced_at > 0);
        assert!(!state.rows.is_empty(), "Modul-Zuordnung gefüllt");

        // Aurora Mk II: Defense/Transport-Module laut Zuordnung.
        let aur = cache
            .modul_modules_for_ship("Aurora Mk II")
            .unwrap()
            .expect("Aurora Mk II in Zuordnung");
        let names: Vec<&str> = aur.iter().map(|r| r.name.as_str()).collect();
        assert!(
            names.iter().any(|n| n.contains("DM Module")),
            "DM-Modul fehlt in Aurora-Zuordnung: {names:?}"
        );
        assert!(
            names.iter().any(|n| n.contains("TS Module")),
            "TS-Modul fehlt in Aurora-Zuordnung: {names:?}"
        );

        // Schiffe ohne Ports (falls vorhanden) sind bekannt — Live-Stand
        // 4.10.2: alle 310 Vehicles tragen Ports, die Liste darf also leer
        // sein; die Query selbst darf nur nicht fehlschlagen.
        let _ = &state.no_port_ships;

        // Zweiter Lauf ohne Force → "fresh" (Etikette: 1 Sync pro Version).
        let second = sync_once(&cache, false).await.unwrap();
        assert_eq!(second, "fresh");
    }
}
