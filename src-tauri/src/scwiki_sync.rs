//! SC-Wiki-Modul-Kompatibilität (S3, Karte t_988e132f): Background-Sync,
//! Tauri-Commands und die manuelle Modul-Zuweisung (BombRacks/Weapon-Kits).
//!
//! * Stündlicher Check von `/api/game-versions/default` — nur bei
//!   Versionswechsel (oder leerer Tabelle) Resync (Etikette: 1 Sync pro
//!   Version, ~30 geringratete Requests).
//! * `modul_compat_refresh` — manueller Force-Sync aus den Settings.
//! * `modul_for_ship` — Zuordnungs-Abfrage für das Hangar-Modul-Autocomplete.
//! * Manuelle Zuweisung (`module-overrides.json`, S3/D2 — Items, die in der
//!   SC-Wiki-API nicht als Module erkennbar sind): Repo-Baseline wird zur
//!   Lesezeit in den API-Datensatz verschmolzen — Änderungen an der Liste
//!   brauchen KEINEN Resync. Bei jedem Sync/Refresh wird zusätzlich die
//!   Remote-Datei von der Gitea-Raw-URL geladen (für Endnutzer ohne
//!   Update-Trigger), im Cache gecacht; Fallback-Kette: Remote-Cache →
//!   Repo-Baseline.

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
struct NoPortFile {
    #[serde(default)]
    ships: Vec<String>,
}

// ---------- Manuelle Modul-Zuweisung (S3/D2, module-overrides.json) ----------

/// Repo-Baseline der manuellen Zuweisung (Sebo-Lieferung, 16 Schiffe:
/// BombRacks per API-Tag, Idris/Kruger/Meteor/Nova-Kits + Endeavor-Bays per
/// Name-Match — in der SC-Wiki-API nicht als Module erkennbar).
const BASELINE_OVERRIDES: &str = include_str!("../resources/module-overrides.json");

/// Remote-Override (Gitea-Raw, branch `main`): Endnutzer ohne Update-Trigger
/// bekommen Änderungen an der Zuweisung mit dem nächsten Sync/Refresh —
/// gecacht im Cache-DB, Fallback auf die Repo-Baseline. Hinweis: Das Gitea-
/// Repo ist privat — kann der Endnutzer die URL nicht lesen (403/404),
/// greift die Fallback-Kette (Remote-Cache → Repo-Baseline).
const REMOTE_OVERRIDES_URL: &str = "https://git.sebo-home.de/sebo/architects-overlay/raw/branch/main/resources/module-overrides.json";
const REMOTE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, serde::Deserialize)]
struct OverrideEntry {
    ship: String,
    #[serde(default)]
    modules: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
struct OverrideFile {
    #[serde(default)]
    ships: Vec<OverrideEntry>,
}

/// Deutsche Kategorie-Labels für die Autocomplete-Anzeige (aus dem
/// Zuweisungs-Namen abgeleitet — die API kennt die Items nicht).
fn override_category(module: &str) -> String {
    let ml = module.to_lowercase();
    if ml.contains("bomb rack") || ml.contains("bombrack") {
        "Bombrack".to_string()
    } else if ml.contains("kit") {
        "Weapon-Kit".to_string()
    } else {
        "Modul".to_string()
    }
}

/// Strenge Match-Norm für Overrides: lowercase, Satzzeichen/-striche als
/// Trenner („RSI Meteor - Laser Weapon Kit" matcht auch
/// „RSI Meteor Laser Weapon Kit" der API).
fn match_norm(name: &str) -> String {
    name.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Übersetzt eine Zuweisungs-Datei (ship → modules) in Zuordnungs-Zeilen
/// (Modul → Schiffe). Parse-Fehler sind NICHT fatal — leere Liste + Log.
fn override_rows(text: &str, source: &str) -> Vec<ModulCompatRow> {
    match serde_json::from_str::<OverrideFile>(text) {
        Ok(file) => {
            // Modul → (Kategorie, Schiffe) vereinigen (Kits stehen bei
            // mehreren Schiffen derselben Klasse).
            let mut by_mod: std::collections::BTreeMap<String, ModulCompatRow> =
                std::collections::BTreeMap::new();
            for entry in file.ships {
                let ship = entry.ship.trim().to_string();
                if ship.is_empty() {
                    continue;
                }
                for module in entry.modules {
                    let module = module.trim().to_string();
                    if module.is_empty() {
                        continue;
                    }
                    let row = by_mod
                        .entry(module.clone())
                        .or_insert_with(|| ModulCompatRow {
                            category: override_category(&module),
                            name: module,
                            ships: Vec::new(),
                        });
                    if !row.ships.iter().any(|s| s == &ship) {
                        row.ships.push(ship.clone());
                    }
                }
            }
            by_mod.into_values().collect()
        }
        Err(e) => {
            log::error!("module-overrides ({source}) unparsabel — Liste übersprungen: {e}");
            Vec::new()
        }
    }
}

/// Verschmilzt Override-Zeilen in die API-Zeilen (S3/D2, toleranter
/// Name-Match): Kits werden von der API in Bestandteile aufgeteilt — eine
/// API-Zeile, deren strenge Norm den Kit-Namen enthält („… Weapons Kit
/// Cannon" ⊃ „Kruger Ballistic Weapons Kit"), bekommt die Kit-Schiffe
/// zugeschlagen; zusätzlich existiert der Kit-Name selbst als Zeile
/// (Fallschirm: genau der Name, den Sebo pflegt). Guard: Kit-Norm ≥ 10
/// Zeichen, damit kurze Namen nicht über den ganzen Katalog laufen.
fn merge_overrides_rows(
    mut rows: Vec<ModulCompatRow>,
    overrides: &[ModulCompatRow],
) -> Vec<ModulCompatRow> {
    for o in overrides {
        let on = match_norm(&o.name);
        for r in rows.iter_mut() {
            let rn = match_norm(&r.name);
            if !on.is_empty() && on.len() >= 10 && rn != on && rn.contains(&on) {
                for s in &o.ships {
                    if !r.ships.contains(s) {
                        r.ships.push(s.clone());
                    }
                }
            }
        }
        match rows.iter_mut().find(|r| match_norm(&r.name) == on) {
            Some(existing) => {
                for s in &o.ships {
                    if !existing.ships.contains(s) {
                        existing.ships.push(s.clone());
                    }
                }
            }
            None => rows.push(o.clone()),
        }
    }
    rows
}

/// Effektive Override-Zeilen zum Lesen: Remote-Cache (letzte erfolgreich
/// gefetchte Remote-Datei) → sonst Repo-Baseline.
fn effective_override_rows(cache: &Cache) -> Vec<ModulCompatRow> {
    let cached = cache
        .modul_overrides_cache_get()
        .ok()
        .flatten()
        .filter(|t| !t.trim().is_empty());
    match cached {
        Some(text) => {
            let rows = override_rows(&text, "remote-cache");
            if rows.is_empty() {
                override_rows(BASELINE_OVERRIDES, "repo-baseline")
            } else {
                rows
            }
        }
        None => override_rows(BASELINE_OVERRIDES, "repo-baseline"),
    }
}

/// Lädt bei jedem Sync/Refresh die Remote-Datei von der Gitea-Raw-URL und
/// cachet sie (S3/D2). Fehler sind NICHT fatal — Fallback-Kette greift beim
/// nächsten Lesen (Remote-Cache → Repo-Baseline).
async fn sync_remote_overrides(cache: &Cache) {
    let client = match reqwest::Client::builder()
        .timeout(REMOTE_TIMEOUT)
        .user_agent(concat!("architects-overlay/", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            log::warn!("module-overrides: HTTP-Client-Build fehlgeschlagen: {e}");
            return;
        }
    };
    let resp = match client.get(REMOTE_OVERRIDES_URL).send().await {
        Ok(r) => r,
        Err(e) => {
            log::warn!(
                "module-overrides: Remote-Fetch fehlgeschlagen: {e} — Fallback Cache/Baseline"
            );
            return;
        }
    };
    if !resp.status().is_success() {
        log::warn!(
            "module-overrides: Remote HTTP {} — Fallback Cache/Baseline",
            resp.status()
        );
        return;
    }
    let text = match resp.text().await {
        Ok(t) => t,
        Err(e) => {
            log::warn!("module-overrides: Remote-Body fehlgeschlagen: {e}");
            return;
        }
    };
    let rows = override_rows(&text, "remote");
    if rows.is_empty() {
        log::warn!("module-overrides: Remote-Datei leer/unparsabel — Cache unverändert");
        return;
    }
    match cache.modul_overrides_cache_set(&text) {
        Ok(()) => log::info!(
            "module-overrides: Remote-Override geladen ({} Module)",
            rows.len()
        ),
        Err(e) => log::warn!("module-overrides: Cache-Speicherung fehlgeschlagen: {e}"),
    }
}

/// Kuratierte Liste der Schiffe OHNE Modul-Ports (`no_port_ships.json`,
/// Review D1): Die paginierte `/api/vehicles`-Liste kennt keinen einzigen
/// Port-losen Vehicle (Live-Stand 4.10.2: 310 Vehicles, 0 ohne Ports) und
/// lässt Schiffe wie Hull E komplett weg — die sind nur im Einzel-Endpoint
/// belegt portlos. Parse-Fehler sind NICHT fatal (wie bombracks.json).
fn curated_no_port_ships() -> Vec<String> {
    match serde_json::from_str::<NoPortFile>(include_str!("no_port_ships.json")) {
        Ok(file) => file.ships,
        Err(e) => {
            log::error!("no_port_ships.json unparsabel — Liste übersprungen: {e}");
            Vec::new()
        }
    }
}

/// Match gegen die kuratierte Liste: exakt ODER als eigenständige Namens-
/// gruppe („MISC Hull E" matcht „Hull E", „Hull C" matcht „Hull E" nicht).
fn matches_curated(needle: &str, candidates: &[String]) -> bool {
    candidates.iter().any(|c| {
        let n = norm_name(c);
        if n.is_empty() {
            return false;
        }
        n == needle
            || needle.starts_with(&format!("{n} "))
            || needle.ends_with(&format!(" {n}"))
            || needle.contains(&format!(" {n} "))
    })
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
        .chain(curated_no_port_ships())
        .collect();
    let with_ports: Vec<scwiki_client::VehiclePorts> = vehicles
        .into_iter()
        .filter(|v| !v.ports.is_empty())
        .collect();

    let items = client
        .items_by_types(MODULE_TYPES)
        .await
        .map_err(|e| format!("scwiki items: {e}"))?;

    // Remote-Override mit jedem echten Sync laden (S3/D2) — Fehler sind
    // nicht fatal (Fallback-Kette Cache → Repo-Baseline).
    sync_remote_overrides(cache).await;

    let compat = scwiki_client::build_compat(&with_ports, &items);
    // Reine API-Snapshot-Zeilen speichern — Overrides werden zur Lesezeit
    // verschmolzen (Änderungen an der Liste brauchen keinen Resync).
    let rows: Vec<ModulCompatRow> = compat
        .into_iter()
        .map(|r| ModulCompatRow {
            name: r.module_name,
            category: r.category,
            ships: r.ships,
        })
        .collect();
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

#[derive(Serialize, Debug)]
pub struct ModulHit {
    pub name: String,
    pub category: String,
}

#[derive(Serialize, Debug)]
pub struct ModulForShip {
    /// `false` = Schiff in der Zuordnung unbekannt (Fallschirm: alle Module).
    pub known: bool,
    /// `false` = Schiff ohne Ports → Modul-Eingabefeld verstecken.
    pub has_ports: bool,
    pub modules: Vec<ModulHit>,
}

#[tauri::command]
pub fn modul_compat_status(state: State<'_, TradeState>) -> Result<ModulStatus, String> {
    let cache = &state.cache;
    let s: Option<ModulCompatState> = cache.modul_compat_state().map_err(|e| e.to_string())?;
    Ok(match s {
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
            // Zählung auf den EFFEKTIVEN Datensatz (API + Overrides).
            let merged = merge_overrides_rows(s.rows.clone(), &effective_override_rows(cache));
            ModulStatus {
                version: s.version,
                synced_at: s.synced_at,
                age_seconds: age,
                module_count: merged.len(),
                ship_count: merged
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
/// Reine Entscheidungslogik hinter `modul_for_ship` — ohne Tauri-State
/// testbar (Review D1: Belege müssen den echten Pfad treffen). `overrides`
/// = manuelle Zuweisung (module-overrides.json, zur Lesezeit verschmolzen).
pub fn modul_for_ship_decision(
    ship: &str,
    curated: &[String],
    overrides: &[ModulCompatRow],
    s: Option<&ModulCompatState>,
) -> ModulForShip {
    let needle = norm_name(ship);
    // Kuratierte No-Port-Liste (no_port_ships.json, Review D1) — gilt auch
    // VOR dem ersten Sync (Feld muss nie auf ein portloses Schiff zeigen).
    if !ship.trim().is_empty() && matches_curated(&needle, curated) {
        return ModulForShip {
            known: true,
            has_ports: false,
            modules: Vec::new(),
        };
    }
    let Some(s) = s else {
        // Kein API-Datensatz: Fallschirm — Feld zeigen. Override-Schiffe
        // (z. B. BombRack-Träger) kennen ihre Module aber auch jetzt schon.
        let modules: Vec<ModulHit> = overrides
            .iter()
            .filter(|r| r.ships.iter().any(|v| norm_name(v) == needle))
            .map(|r| ModulHit {
                name: r.name.clone(),
                category: r.category.clone(),
            })
            .collect();
        return ModulForShip {
            known: !ship.trim().is_empty() && !modules.is_empty(),
            has_ports: true,
            modules,
        };
    };
    // Effektiver Katalog: API-Snapshot + Overrides (S3/D2).
    let rows = merge_overrides_rows(s.rows.clone(), overrides);
    if ship.trim().is_empty() {
        // Gesamtkatalog (Fallschirm).
        return ModulForShip {
            known: false,
            has_ports: true,
            modules: rows
                .iter()
                .map(|r| ModulHit {
                    name: r.name.clone(),
                    category: r.category.clone(),
                })
                .collect(),
        };
    }
    // Ohne Ports → bekannt, kein Modulfeld.
    if s.no_port_ships.iter().any(|v| norm_name(v) == needle) {
        return ModulForShip {
            known: true,
            has_ports: false,
            modules: Vec::new(),
        };
    }
    let modules: Vec<ModulHit> = rows
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
        ModulForShip {
            known: false,
            has_ports: true,
            modules: Vec::new(),
        }
    } else {
        ModulForShip {
            known: true,
            has_ports: true,
            modules,
        }
    }
}

#[tauri::command]
pub fn modul_for_ship(state: State<'_, TradeState>, ship: String) -> Result<ModulForShip, String> {
    let s: Option<ModulCompatState> = state
        .cache
        .modul_compat_state()
        .map_err(|e| e.to_string())?;
    let overrides = effective_override_rows(&state.cache);
    Ok(modul_for_ship_decision(
        &ship,
        &curated_no_port_ships(),
        &overrides,
        s.as_ref(),
    ))
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

    fn row(name: &str, category: &str, ships: &[&str]) -> ModulCompatRow {
        ModulCompatRow {
            name: name.into(),
            category: category.into(),
            ships: ships.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Kuratierte No-Port-Liste: geparst, nicht leer, Hull E drin.
    #[test]
    fn curated_no_port_list_parses() {
        let list = curated_no_port_ships();
        assert!(!list.is_empty(), "no_port_ships.json leer — D1 unbelegt");
        assert!(
            list.iter().any(|s| norm_name(s) == "hull e"),
            "Hull E fehlt in der kuratierten Liste: {list:?}"
        );
    }

    /// Namens-Match: Präfix/Suffix-Toleranz, keine Fehltreffer.
    #[test]
    fn curated_match_tolerates_prefix_but_no_crossmatch() {
        let list = vec!["Hull E".to_string()];
        assert!(matches_curated(&norm_name("Hull E"), &list));
        assert!(matches_curated(&norm_name("MISC Hull E"), &list));
        assert!(!matches_curated(&norm_name("MISC Hull C"), &list));
        assert!(!matches_curated(&norm_name("Kraken"), &list));
        assert!(!matches_curated(&norm_name(""), &list));
    }

    /// Der reale Entscheidungs-Pfad (Review D1): Hull E aus der kuratierten
    /// Liste → Feld versteckt; das gilt auch VOR dem ersten Sync (State None).
    #[test]
    fn decision_hides_module_field_for_curated_no_port_ship() {
        let curated = curated_no_port_ships();
        let overrides = Vec::new();
        for ship in ["Hull E", "MISC Hull E", "  hull   e  "] {
            let d = modul_for_ship_decision(ship, &curated, &overrides, None);
            assert!(d.known && !d.has_ports, "{ship}: {d:?}");
            assert!(d.modules.is_empty());
        }
        // Portloses Gegenstück darf nicht fälschlich matchen.
        let d = modul_for_ship_decision("MISC Hull C", &curated, &overrides, None);
        assert!(!d.known && d.has_ports, "Hull C: {d:?}");
        // Mit Cache-Datensatz bleibt Aurora der Normalfall (Feld sichtbar,
        // nur passende Module).
        let cache = Cache::open_in_memory().unwrap();
        cache
            .modul_compat_store(
                "v1",
                &[ModulCompatRow {
                    name: "Aurora Mk II DM Module".into(),
                    category: "Modul".into(),
                    ships: vec!["Aurora Mk II".into()],
                }],
                &["Hull E"],
            )
            .unwrap();
        let state = cache.modul_compat_state().unwrap().unwrap();
        let d = modul_for_ship_decision("Aurora Mk II", &curated, &overrides, Some(&state));
        assert!(d.known && d.has_ports && d.modules.len() == 1, "{d:?}");
    }

    /// Sebo-Lieferung (S3/D2): Die Repo-Baseline parst, enthält 16 Schiffe —
    /// BombRacks (Eclips/Retaliator/Gladiator), Kits (Idris/Kruger/Meteor/
    /// Nova) und Endeavor-Bays.
    #[test]
    fn override_baseline_parses_with_sebo_ships() {
        let rows = override_rows(BASELINE_OVERRIDES, "test");
        assert!(!rows.is_empty(), "Baseline unparsabel oder leer");
        let ships: std::collections::BTreeSet<&str> = rows
            .iter()
            .flat_map(|r| r.ships.iter().map(|s| s.as_str()))
            .collect();
        assert!(ships.len() >= 15, "zu wenige Schiffe: {ships:?}");
        for ship in [
            "Aegis Eclips",
            "Aegis Retaliator",
            "Anvil Gladiator",
            "Aegis Idris M",
            "Kruger L-21 Wolf",
            "MISC Endeavor",
            "RSI Meteor",
            "Tumbril Nova",
        ] {
            assert!(ships.contains(ship), "{ship} fehlt: {ships:?}");
        }
        let eclipse = rows.iter().find(|r| r.name == "Eclipse BombRack").unwrap();
        assert_eq!(eclipse.category, "Bombrack");
        assert!(eclipse.ships.contains(&"Aegis Eclips".to_string()));
        let kit = rows
            .iter()
            .find(|r| r.name == "Kruger Ballistic Weapons Kit")
            .unwrap();
        assert_eq!(kit.category, "Weapon-Kit");
        // Idris-T Kit: Schiffe M + P vereinigt.
        let idris = rows.iter().find(|r| r.name == "Idris-T Kit").unwrap();
        assert!(idris.ships.contains(&"Aegis Idris M".to_string()));
        assert!(idris.ships.contains(&"Aegis Idris P".to_string()));
    }

    /// Toleranter Name-Match (S3/D2): Kits werden von der API in Bestand-
    /// teile aufgeteilt — die API-Zeile bekommt die Kit-Schiffe; der Kit-Name
    /// selbst existiert als Zeile; Satzzeichen-Toleranz („-"); kurze Namen
    /// laufen nicht über den Katalog (Len-Guard).
    #[test]
    fn merge_overrides_tolerant_kit_components() {
        let api = vec![
            row("Kruger Ballistic Weapons Kit Cannon", "Waffe", &[]),
            row("RSI Meteor Laser Weapon Kit", "Waffe", &[]),
            row("Aurora Mk II DM Module", "Modul", &["Aurora Mk II"]),
        ];
        let overrides = vec![
            ModulCompatRow {
                name: "Kruger Ballistic Weapons Kit".into(),
                category: "Weapon-Kit".into(),
                ships: vec!["Kruger S-65 Stingray".into()],
            },
            ModulCompatRow {
                name: "RSI Meteor - Laser Weapon Kit".into(),
                category: "Weapon-Kit".into(),
                ships: vec!["RSI Meteor".into()],
            },
            ModulCompatRow {
                name: "Aegis".into(), // kurz — kein Katalog-Overmatch
                category: "Modul".into(),
                ships: vec!["X".into()],
            },
        ];
        let merged = merge_overrides_rows(api, &overrides);
        // Bestandteil trägt die Kit-Schiffe …
        let comp = merged
            .iter()
            .find(|r| r.name == "Kruger Ballistic Weapons Kit Cannon")
            .unwrap();
        assert!(
            comp.ships.contains(&"Kruger S-65 Stingray".to_string()),
            "{comp:?}"
        );
        // … Kit-Name existiert selbst als Zeile …
        assert!(
            merged
                .iter()
                .any(|r| r.name == "Kruger Ballistic Weapons Kit")
        );
        // … Satzzeichen-Toleranz: Der Meteor-Override matcht die API-Zeile
        // (kein Duplikat "RSI Meteor - Laser Weapon Kit").
        let meteor = merged
            .iter()
            .find(|r| r.name == "RSI Meteor Laser Weapon Kit")
            .unwrap();
        assert!(
            meteor.ships.contains(&"RSI Meteor".to_string()),
            "{meteor:?}"
        );
        assert!(
            !merged
                .iter()
                .any(|r| r.name == "RSI Meteor - Laser Weapon Kit")
        );
        // Len-Guard: Kurzer Override-Name matcht keine Fremdzeilen …
        let sabre = merged
            .iter()
            .find(|r| r.name.contains("Aurora Mk II DM"))
            .unwrap();
        assert!(!sabre.ships.contains(&"X".to_string()), "{sabre:?}");
        // … existiert aber als eigene Zeile.
        assert!(merged.iter().any(|r| r.name == "Aegis"));
        // Aurora-Zeile unverändert (2 Schiffe nie dabei).
        assert_eq!(sabre.ships, vec!["Aurora Mk II".to_string()]);
        // Merge ist idempotent (keine Doppel-Schiffe bei zweitem Lauf).
        let again = merge_overrides_rows(merged.clone(), &overrides);
        let comp2 = again
            .iter()
            .find(|r| r.name == "Kruger Ballistic Weapons Kit Cannon")
            .unwrap();
        assert_eq!(comp2.ships.len(), comp.ships.len());
    }

    /// Lesezeit-Merge (S3/D2): Eclips bekommt sein BombRack auch OHNE
    /// API-Datensatz (vor dem ersten Sync); mit Datensatz bleibt die
    /// Aurora-Zuordnung unangetastet.
    #[test]
    fn decision_merges_overrides_at_read_time() {
        let curated = curated_no_port_ships();
        let overrides = vec![ModulCompatRow {
            name: "Eclipse BombRack".into(),
            category: "Bombrack".into(),
            ships: vec!["Aegis Eclips".into()],
        }];
        // Vor dem ersten Sync: Override-Schiff ist bekannt mit seinem Modul.
        let d = modul_for_ship_decision("Aegis Eclips", &curated, &overrides, None);
        assert!(d.known && d.has_ports, "{d:?}");
        assert_eq!(d.modules.len(), 1);
        assert_eq!(d.modules[0].name, "Eclipse BombRack");
        // Fremdes Schiff bleibt Fallschirm.
        let d = modul_for_ship_decision("Aurora Mk II", &curated, &overrides, None);
        assert!(!d.known && d.has_ports && d.modules.is_empty(), "{d:?}");
        // Mit API-Datensatz: Overrides kommen in der Zuordnung dazu.
        let cache = Cache::open_in_memory().unwrap();
        cache
            .modul_compat_store(
                "v1",
                &[row("DM", "Modul", &["Aurora Mk II"])],
                &[] as &[&str],
            )
            .unwrap();
        let state = cache.modul_compat_state().unwrap().unwrap();
        let ecl = modul_for_ship_decision("Aegis Eclips", &curated, &overrides, Some(&state));
        assert!(ecl.known && ecl.modules.len() == 1, "{ecl:?}");
    }

    async fn sync_once_wait_busy(cache: &Cache, force: bool) -> String {
        // Tests laufen parallel und teilen SYNC_BUSY — "busy" einfach
        // abwarten statt als Erfolg zu verbuchen.
        loop {
            let out = sync_once(cache, force).await.unwrap();
            if out != "busy" {
                return out;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Live-Beleg für S3 (t_988e132f): Der Sync läuft einmal gegen die echte
    /// SC-Wiki-API, die Tabelle füllt sich, und die Cross-Referenz liefert
    /// für die Aurora Mk II ihre Defense/Transport-Module (DM/TS). Ignored
    /// by default — `cargo test -p architects-overlay --lib -- --ignored
    /// scwiki` (nur wenige geringratete Requests, 1 Sync pro Version).
    #[tokio::test]
    #[ignore = "live SC-Wiki — run with -- --ignored"]
    async fn live_sync_fills_cache_and_aurora() {
        let cache = Cache::open_in_memory().unwrap();
        let outcome = sync_once_wait_busy(&cache, true).await;
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

    /// Live-Beleg für den D1-Fix (Review Runde 1): Hull E ist in der
    /// paginierten /api/vehicles-Liste nicht enthalten, im Einzel-Endpoint
    /// aber belegt portlos (ports:0) — und über die kuratierte Liste liefert
    /// der reale Entscheidungs-Pfad has_ports:false. Schreibt außerdem das
    /// Fixture für die UI-Probe (docs/scripts/s3-modul-fixture.json) aus
    /// echten modul_for_ship_decision-Ergebnissen — inkl. BombRack-/Kit-
    /// Schiff aus der Sebo-Lieferung (S3/D2, zur Lesezeit verschmolzen).
    #[tokio::test]
    #[ignore = "live SC-Wiki — run with -- --ignored"]
    async fn live_hull_e_portless_and_probe_fixture() {
        let client = ScWikiClient::new(scwiki_client::DEFAULT_BASE_URL).unwrap();
        // API-Beleg: Hull E existiert (Einzeln-Endpoint) und hat 0 Ports.
        let hull_e = client.vehicle_by_slug("hull-e").await.unwrap();
        assert!(
            hull_e.ports.is_empty(),
            "Hull E plötzlich mit Ports — kuratierte Liste prüfen"
        );

        let curated = curated_no_port_ships();
        let d = modul_for_ship_decision("Hull E", &curated, &[], None);
        assert!(d.known && !d.has_ports, "Hull E: {d:?}");

        // Fixture aus dem echten Pfad für die UI-Probe.
        let cache = Cache::open_in_memory().unwrap();
        let outcome = sync_once_wait_busy(&cache, true).await;
        assert_eq!(outcome, "synced");
        let state = cache.modul_compat_state().unwrap();
        let overrides = effective_override_rows(&cache);
        assert!(
            overrides.iter().any(
                |r| r.name == "Eclipse BombRack" && r.ships.iter().any(|s| s == "Aegis Eclips")
            ),
            "Repo-Baseline nicht geladen: Eclipse BombRack fehlt"
        );
        let aurora = modul_for_ship_decision("Aurora Mk II", &curated, &overrides, state.as_ref());
        assert!(aurora.has_ports && !aurora.modules.is_empty(), "{aurora:?}");
        // S3/D2: BombRack-Schiff aus Sebos Lieferung — Modul kommt aus der
        // manuellen Zuweisung (API kennt es nicht als Modul).
        let eclips = modul_for_ship_decision("Aegis Eclips", &curated, &overrides, state.as_ref());
        assert!(eclips.known && eclips.has_ports, "{eclips:?}");
        assert!(
            eclips.modules.iter().any(|m| m.name == "Eclipse BombRack"),
            "Eclipse BombRack fehlt in Eclips-Zuordnung: {eclips:?}"
        );
        // Kein Fremd-BombRack (Retaliator/Gladiator) im Eclips-Vorschlag.
        assert!(
            eclips
                .modules
                .iter()
                .all(|m| !m.name.contains("Retaliator") && !m.name.contains("Gladiator")),
            "Fremd-BombRacks in Eclips-Zuordnung: {eclips:?}"
        );
        // Kit-Schiff: Kruger P-52 Merlin → Snub Laser Kit.
        let merlin =
            modul_for_ship_decision("Kruger P-52 Merlin", &curated, &overrides, state.as_ref());
        assert!(merlin.known, "{merlin:?}");
        assert!(
            merlin
                .modules
                .iter()
                .any(|m| m.name == "Kruger Snub Laser Weapons Kit"),
            "Kruger Snub Laser Weapons Kit fehlt: {merlin:?}"
        );
        let fixture = serde_json::json!({
            "_doc": "Erzeugt vom Live-Test live_hull_e_portless_and_probe_fixture (echte SC-Wiki-API + reale modul_for_ship_decision-Logik + Lesezeit-Merge der Sebo-Overrides) — NICHT von Hand editieren.",
            "generated_version": cache.modul_compat_state().unwrap().unwrap().version,
            "for_ship": {
                "aurora mk ii": aurora,
                "hull e": d,
                "aegis eclips": eclips,
                "kruger p-52 merlin": merlin,
            }
        });
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../docs/scripts/s3-modul-fixture.json");
        std::fs::write(&path, serde_json::to_string_pretty(&fixture).unwrap()).unwrap();
        println!("fixture geschrieben: {}", path.display());
    }
}
