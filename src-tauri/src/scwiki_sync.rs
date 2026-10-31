//! Modul-Zuweisung (S4, Karte t_66980b3c): Modul-Kompatibilität kommt
//! AUSSCHLIESSLICH aus `module-overrides.json` — die SC-Wiki-API liefert
//! KEINE Modul-Daten mehr (weder Hardpoints/Ports noch Item-Katalog).
//!
//! * Die `modul_compat`-Tabelle wird bei jedem Sync aus der Zuweisungs-Datei
//!   neu befüllt (Repo-Baseline, zusätzlich Remote-Override von der
//!   GitHub-Raw-URL (öffentliches Mirror-Repo) — Fehler sind nicht fatal,
//!   Fallback-Kette Remote-Cache →
//!   Repo-Baseline). Der erste Sync überschreibt damit auch alte API-Daten
//!   (Migration: API-Datensatz verfällt, Nutzereinträge im Hangar bleiben).
//! * Stündlicher Lauf: lädt die Remote-Datei (1 Request) — kein
//!   game-versions-Trigger mehr.
//! * `modul_compat_refresh` — manueller Force-Sync aus den Settings
//!   („Modul-Zuweisungen aktualisieren“), zieht NUR die Remote-Datei.
//! * `modul_for_ship` — Nur-Datei-Regel: Schiff mit Eintrag in der Datei →
//!   Modul-Feld mit GENAU den Datei-Modulen; Schiff ohne Eintrag → gar
//!   kein Modul-Feld (kein API-Fallschirm mehr).

use std::sync::Arc;
use std::time::Duration;

use cache::Cache;
use cache::modul_compat::{ModulCompatRow, ModulCompatState};
use serde::Serialize;
use tauri::State;

use crate::trade::TradeState;

/// Stündlicher Remote-Refresh (1 Request auf die Gitea-Raw-URL — kein
/// Game-Version-Check mehr, die Zuweisung lebt in der Datei).
pub const MODUL_SYNC_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// Erster Lauf kurz nach Boot, damit die Tabelle beim ersten Hangar-Aufruf
/// aus der Datei gefüllt ist.
const FIRST_SYNC_DELAY: Duration = Duration::from_secs(15);

/// Prozessweite Busy-Markierung gegen parallele Syncs (Stunde + manueller
/// Button). `AtomicBool` genügt: nur ein App-Prozess pro Cache.
static SYNC_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

// ---------- Modul-Zuweisung (module-overrides.json, S4: einzige Quelle) ----------

/// Quelle-Label für die `meta`-Version (Info-Anzeige in den Settings — die
/// SC-Wiki-Game-Version ist als Trigger entfallen).
pub const SOURCE_LABEL: &str = "module-overrides.json";

/// Repo-Baseline der Zuweisung (Sebo, 17 Schiffe: Eclipse/Retaliator/
/// Gladiator-BombRacks, Idris/Kruger/Meteor/Nova-Kits, Endeavor-Bays,
/// Aurora MK II, Galaxy). Einzige Quelle der Modul-Zuordnung (S4).
const BASELINE_OVERRIDES: &str = include_str!("../resources/module-overrides.json");

/// Remote-Override (öffentliches GitHub-Mirror-Repo, branch `main` —
/// TF t_0d510f31): Sebo pflegt die Datei im Repo, Endnutzer bekommen
/// Änderungen mit dem nächsten Sync/Refresh — gecacht im Cache-DB,
/// Fallback auf die Repo-Baseline. Die URL ist öffentlich (raw.
/// githubusercontent.com, kein Auth-Header nötig); schlägt der Fetch fehl
/// (403/404/Offline), greift die Fallback-Kette (Remote-Cache →
/// Repo-Baseline). Pflege: docs/MODULE-OVERRIDES.md.
const REMOTE_OVERRIDES_URL: &str = "https://raw.githubusercontent.com/SeBoOne/architects-overlay/main/src-tauri/resources/module-overrides.json";
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
/// Zuweisungs-Namen abgeleitet).
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

/// Übersetzt eine Zuweisungs-Datei (ship → modules) in Zuordnungs-Zeilen
/// (Modul → Schiffe). Parse-Fehler sind NICHT fatal — leere Liste + Log.
/// Doppelte Modul-Namen pro Datei werden vereinigt (ein Eintrag pro Modul).
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

/// Effektive Zuweisungs-Zeilen zum Lesen: Remote-Cache (letzte erfolgreich
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

/// Lädt bei jedem Sync/Refresh die Remote-Datei von der GitHub-Raw-URL und
/// cachet sie. Fehler sind NICHT fatal — Fallback-Kette greift beim nächsten
/// Lesen (Remote-Cache → Repo-Baseline).
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

/// Ein Sync-Durchlauf: Remote-Datei laden (nicht fatal), dann die Tabelle
/// AUSSCHLIESSLICH aus der effektiven Zuweisungs-Datei neu befüllen
/// (Snapshot: DELETE + INSERT — ersetzt auch alte API-Daten, Migration S4).
/// Rückkehr: "synced" | "busy".
pub async fn sync_once(cache: &Cache) -> Result<String, String> {
    if SYNC_BUSY.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Ok("busy".to_string());
    }
    let out = sync_once_inner(cache).await;
    SYNC_BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
    out
}

async fn sync_once_inner(cache: &Cache) -> Result<String, String> {
    // Remote-Override laden (S4: einzige Fremd-Quelle neben der Baseline;
    // Fehler sind nicht fatal — Fallback-Kette Cache → Repo-Baseline).
    sync_remote_overrides(cache).await;
    let rows = effective_override_rows(cache);
    if rows.is_empty() {
        return Err("module-overrides: keine Zuweisungen (Baseline unparsabel?)".to_string());
    }
    cache
        .modul_compat_store(SOURCE_LABEL, &rows)
        .map_err(|e| e.to_string())?;
    log::info!(
        "modul-sync: {} Zuweisungen aus der Datei gespeichert",
        rows.len()
    );
    Ok("synced".to_string())
}

/// Stündlicher Background-Loop + erster Lauf nach dem Boot. Fehler sind
/// nicht fatal — der nächste Tick versucht es erneut.
pub fn spawn_modul_sync(cache: Arc<Cache>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_SYNC_DELAY).await;
        match sync_once(&cache).await {
            Ok(ref other) if other == "synced" => {
                log::info!("modul-sync: erster Lauf abgeschlossen")
            }
            Ok(other) => log::debug!("modul-sync: erster Lauf: {other}"),
            Err(e) => log::warn!("modul-sync: erster Lauf fehlgeschlagen: {e}"),
        }
        loop {
            tokio::time::sleep(MODUL_SYNC_INTERVAL).await;
            if let Err(e) = sync_once(&cache).await {
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
    /// `false` = Schiff hat KEINEN Eintrag in module-overrides.json →
    /// kein Modul-Feld (S4 Nur-Datei-Regel).
    pub known: bool,
    /// `true` nur bei Schiffen mit Datei-Eintrag (S4: es gibt keine
    /// API-Ports mehr, das Feld hängt allein an der Datei).
    pub has_ports: bool,
    pub modules: Vec<ModulHit>,
}

#[tauri::command]
pub fn modul_compat_status(state: State<'_, TradeState>) -> Result<ModulStatus, String> {
    let cache = &state.cache;
    let s: Option<ModulCompatState> = cache.modul_compat_state().map_err(|e| e.to_string())?;
    // Zählung auf die EFFEKTIVE Datei (Remote-Cache → Repo-Baseline) — auch
    // vor dem ersten Sync aussagekräftig.
    let rows = effective_override_rows(cache);
    if rows.is_empty() {
        return Ok(ModulStatus {
            version: String::new(),
            synced_at: 0,
            age_seconds: None,
            module_count: 0,
            ship_count: 0,
        });
    }
    let synced_at = s.as_ref().map_or(0, |s| s.synced_at);
    let age = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .checked_sub(synced_at)
        .filter(|_| synced_at > 0);
    Ok(ModulStatus {
        version: SOURCE_LABEL.to_string(),
        synced_at,
        age_seconds: age,
        module_count: rows.len(),
        ship_count: rows
            .iter()
            .flat_map(|r| r.ships.iter())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
    })
}

#[tauri::command]
pub async fn modul_compat_refresh(state: State<'_, TradeState>) -> Result<String, String> {
    sync_once(&state.cache).await
}

/// Modul-Zuordnung für ein Schiff (Hangar-Modul-Autocomplete) — S4
/// Nur-Datei-Regel: nur Schiffe mit Eintrag in `module-overrides.json`
/// bekommen das Feld, Autocomplete = ausschließlich deren Datei-Module.
/// Reine Entscheidungslogik — ohne Tauri-State testbar.
pub fn modul_for_ship_decision(ship: &str, overrides: &[ModulCompatRow]) -> ModulForShip {
    let needle = norm_name(ship);
    if needle.is_empty() {
        return ModulForShip {
            known: false,
            has_ports: false,
            modules: Vec::new(),
        };
    }
    let modules: Vec<ModulHit> = overrides
        .iter()
        .filter(|r| r.ships.iter().any(|v| norm_name(v) == needle))
        .map(|r| ModulHit {
            name: r.name.clone(),
            category: r.category.clone(),
        })
        .collect();
    if modules.is_empty() {
        // Kein Datei-Eintrag → gar kein Modul-Feld (S4).
        ModulForShip {
            known: false,
            has_ports: false,
            modules,
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
    let overrides = effective_override_rows(&state.cache);
    Ok(modul_for_ship_decision(&ship, &overrides))
}

/// Geteilte Normalisierung aus `cache::modul_compat::norm` (Case +
/// Whitespace + Hersteller-Präfix-Strip: „RSI Galaxy“ ≡ „Galaxy“,
/// „RSI Aurora MK II“ ≡ „Aurora Mk II“ — SA-Fix t_d0076295).
fn norm_name(name: &str) -> String {
    cache::modul_compat::norm(name)
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

    /// Sebos Lieferung (S4-Baseline): Die Datei parst, enthält 17 Schiffe —
    /// Eclipse/Retaliator/Gladiator-BombRacks, Kits (Idris/Kruger/Meteor/
    /// Nova), Endeavor-Bays, Aurora MK II und Galaxy.
    #[test]
    fn override_baseline_parses_with_sebo_ships() {
        let rows = override_rows(BASELINE_OVERRIDES, "test");
        assert!(!rows.is_empty(), "Baseline unparsabel oder leer");
        let mut by_ship: std::collections::BTreeMap<&str, usize> =
            std::collections::BTreeMap::new();
        for r in &rows {
            for s in &r.ships {
                *by_ship.entry(s.as_str()).or_default() += 1;
            }
        }
        assert!(by_ship.len() >= 17, "zu wenige Schiffe: {by_ship:?}");
        for (ship, modules) in [
            ("Aegis Eclipse", 2), // 3 Datei-Einträge, S10 doppelt → 2 unique
            ("Aegis Retaliator", 11),
            ("Galaxy", 3),          // SA: Live-Hangar-Name (ohne RSI-Präfix)
            ("Aurora Mk II", 2),    // SA: Live-Hangar-Name („Mk“, kein Präfix)
            ("L-22 Alpha Wolf", 1), // SA: Live-Hangar-Name
            ("Anvil Gladiator", 2),
            ("Aegis Idris M", 2),
            ("Kruger L-21 Wolf", 1),
            ("MISC Endeavor", 9),
            ("RSI Meteor", 1),
            ("Tumbril Nova", 1),
        ] {
            assert_eq!(by_ship.get(ship), Some(&modules), "{ship}: {by_ship:?}");
        }
        let eclipse = rows
            .iter()
            .find(|r| r.name == "S10 Bomb Rack Weapon Kit")
            .unwrap();
        assert_eq!(eclipse.category, "Bombrack");
        assert!(eclipse.ships.contains(&"Aegis Eclipse".to_string()));
        // Retaliator hat eigene Racks („Size 3/5 Bomb Rack …“) — keine
        // geteilte Zeile mit dem Eclipse-S10-Kit.
        assert!(!eclipse.ships.contains(&"Aegis Retaliator".to_string()));
        let ret_rack = rows
            .iter()
            .find(|r| r.name == "Size 5 Bomb Rack - Front")
            .unwrap();
        assert!(ret_rack.ships.contains(&"Aegis Retaliator".to_string()));
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

    /// S4 Nur-Datei-Regel am echten Entscheidungs-Pfad: Eclipse = nur seine
    /// Racks, Retaliator = seine 11 Module, Galaxy = seine 3 Module,
    /// Aurora MK II = DM/TS — und ein Schiff OHNE Datei-Eintrag (Arrow)
    /// bekommt GAR KEIN Modul-Feld mehr (kein API-Fallschirm).
    #[test]
    fn decision_file_only_rule() {
        let overrides = override_rows(BASELINE_OVERRIDES, "test");
        assert!(!overrides.is_empty());

        let ecl = modul_for_ship_decision("Aegis Eclipse", &overrides);
        assert!(ecl.known && ecl.has_ports, "{ecl:?}");
        assert!(
            ecl.modules.iter().all(|m| m.name.contains("Bomb Rack")),
            "Fremd-Module bei Eclipse: {ecl:?}"
        );
        assert!(
            ecl.modules
                .iter()
                .all(|m| !m.name.contains("Retaliator") && !m.name.contains("Galadiator")),
            "Fremd-BombRacks bei Eclipse: {ecl:?}"
        );

        let ret = modul_for_ship_decision("Aegis Retaliator", &overrides);
        assert_eq!(ret.modules.len(), 11, "{ret:?}");
        // Genau die 11 Datei-Module (Living/Cargo/Torpedo/Dropship + 4 Racks),
        // keine Fremd-Module (z. B. Galaxy Refinery).
        assert!(
            ret.modules
                .iter()
                .all(|m| !m.name.contains("Galaxy") && !m.name.contains("Refinery")),
            "{ret:?}"
        );

        let gal = modul_for_ship_decision("RSI Galaxy", &overrides);
        assert_eq!(gal.modules.len(), 3, "{gal:?}");
        for expect in ["Cargo Module", "Med Bay Module", "Refinery Module"] {
            assert!(
                gal.modules.iter().any(|m| m.name.contains(expect)),
                "{expect} fehlt: {gal:?}"
            );
        }

        // SA-Regression (t_d0076295): Hangar-Namen OHNE Hersteller-Präfix
        // matchen die Datei-Einträge (Prefix-Strip + Case/Whitespace).
        // Live-Dump 2026-10-30: Hangar führt „Galaxy“ und „Aurora Mk II“.
        for (hangar_name, expected) in [("Galaxy", 3), ("Aurora Mk II", 2)] {
            let d = modul_for_ship_decision(hangar_name, &overrides);
            assert!(d.known && d.has_ports, "{hangar_name}: {d:?}");
            assert_eq!(d.modules.len(), expected, "{hangar_name}: {d:?}");
        }
        // Auch die geprefixte Form matcht weiter (Anzeige- ODER Kurzname).
        let gal = modul_for_ship_decision("RSI Galaxy", &overrides);
        assert!(gal.known && gal.modules.len() == 3, "{gal:?}");

        // Match ist Case/Whitespace-tolerant (Hangar-Name „Aurora Mk II“).
        let aur = modul_for_ship_decision("  rsi aurora   mk ii ", &overrides);
        assert!(aur.known && aur.modules.len() == 2, "{aur:?}");

        // Schiff ohne Datei-Eintrag → GAR KEIN Modul-Feld (S4 Kernregel).
        for ship in ["Arrow", "Aegis Arrow", "Kraken", "MISC Hull C"] {
            let d = modul_for_ship_decision(ship, &overrides);
            assert!(
                !d.known && !d.has_ports && d.modules.is_empty(),
                "{ship}: {d:?}"
            );
        }
        // Leerer Name ebenso.
        let d = modul_for_ship_decision("", &overrides);
        assert!(!d.known && !d.has_ports && d.modules.is_empty());
    }

    /// Remote-Cache-Vorrang: gecachte Remote-Datei überschreibt die Baseline;
    /// unparsabler Remote-Cache fällt auf die Baseline zurück.
    #[test]
    fn effective_rows_remote_cache_precedence() {
        let cache = Cache::open_in_memory().unwrap();
        // Ohne Remote-Cache: Baseline.
        let base = effective_override_rows(&cache);
        assert!(base.iter().any(|r| r.name == "S10 Bomb Rack Weapon Kit"));
        // Remote-Cache mit eigener Datei → deren Zeilen.
        cache
            .modul_overrides_cache_set(
                r#"{"ships": [{"ship": "Test Ship", "modules": ["Test Modul A"]}]}"#,
            )
            .unwrap();
        let eff = effective_override_rows(&cache);
        assert_eq!(eff.len(), 1);
        assert_eq!(eff[0].name, "Test Modul A");
        // Unparsabler Remote-Cache → Fallback Baseline.
        cache.modul_overrides_cache_set("{ kaputt").unwrap();
        let eff = effective_override_rows(&cache);
        assert!(eff.iter().any(|r| r.name == "S10 Bomb Rack Weapon Kit"));
    }

    /// Sync befüllt die Tabelle ausschließlich aus der Datei (Snapshot
    /// ersetzt alte API-Daten — Migration S4); der Entscheidungs-Pfad liest
    /// die Lesezeit-Zeilen (Remote-Cache → Baseline).
    #[test]
    fn sync_stores_file_rows_only() {
        let cache = Cache::open_in_memory().unwrap();
        // Simulierter alter API-Datensatz (Migration: muss verfallen).
        cache
            .modul_compat_store(
                "4.10.2-LIVE.12881860",
                &[row("API Legacy Modul", "Modul", &["Aurora Mk II"])],
            )
            .unwrap();
        // Remote-Cache leer → Sync befüllt aus der Repo-Baseline.
        cache.modul_overrides_cache_clear().unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let out = rt.block_on(async {
            loop {
                let out = sync_once(&cache).await.unwrap();
                if out != "busy" {
                    break out;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        });
        assert_eq!(out, "synced");
        let state = cache.modul_compat_state().unwrap().expect("Datensatz");
        assert_eq!(state.version, SOURCE_LABEL);
        // Kein API-Legacy-Modul mehr in der Tabelle.
        assert!(
            !state.rows.iter().any(|r| r.name == "API Legacy Modul"),
            "API-Altbestand nicht verfallen: {}",
            state.rows.len()
        );
        // Alles kommt aus der Datei (SA: Datei-Namen = Live-Hangar-Namen).
        let ships: std::collections::BTreeSet<String> = state
            .rows
            .iter()
            .flat_map(|r| r.ships.iter().cloned())
            .collect();
        assert!(
            ships.contains("Aegis Eclipse")
                && ships.contains("Galaxy")
                && ships.contains("Aurora Mk II"),
            "Datei-Schiffe fehlen: {ships:?}"
        );
    }

    /// Live-Beleg für S4 (t_66980b3c): Der Sync läuft gegen die echte
    /// GitHub-Raw-URL (öffentlich — TF t_0d510f31; offline fällt er auf die
    /// Repo-Baseline zurück — beides muss "synced" liefern), die Tabelle
    /// enthält danach ausschließlich Datei-Zeilen, und die Nur-Datei-Regel
    /// liefert Eclipse/Racks, Retaliator/11, Galaxy/3, Arrow/kein Feld.
    /// Ignored by default — `cargo test -p architects-overlay --lib --
    /// --ignored modul` (nur 1 geringrateter Request pro Lauf).
    #[tokio::test]
    #[ignore = "live GitHub-Raw — run with -- --ignored"]
    async fn live_sync_file_only_and_decision() {
        let cache = Cache::open_in_memory().unwrap();
        let outcome = loop {
            let out = sync_once(&cache).await.unwrap();
            if out != "busy" {
                break out;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        };
        assert_eq!(outcome, "synced");

        let state = cache.modul_compat_state().unwrap().expect("Datensatz");
        assert_eq!(state.version, SOURCE_LABEL);
        assert!(state.synced_at > 0);
        assert!(!state.rows.is_empty(), "Zuweisungen aus der Datei gefüllt");

        // Entscheidungs-Pfad auf den GESPEICHERTEN (nicht nur Lesezeit-)
        // Datensatz: Nur-Datei-Regel greift.
        let ecl = modul_for_ship_decision("Aegis Eclipse", &state.rows);
        assert!(ecl.known && ecl.modules.iter().all(|m| m.name.contains("Bomb Rack")));
        let ret = modul_for_ship_decision("Aegis Retaliator", &state.rows);
        assert_eq!(ret.modules.len(), 11);
        let gal = modul_for_ship_decision("RSI Galaxy", &state.rows);
        assert_eq!(gal.modules.len(), 3);
        let arrow = modul_for_ship_decision("Aegis Arrow", &state.rows);
        assert!(!arrow.known && !arrow.has_ports && arrow.modules.is_empty());

        // Zweiter Lauf ist erneut "synced" (kein Game-Version-Gate mehr).
        let second = loop {
            let out = sync_once(&cache).await.unwrap();
            if out != "busy" {
                break out;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        };
        assert_eq!(second, "synced");

        // Fixture aus dem echten Pfad für die UI-Probe (S4): Nur-Datei-
        // Ergebnisse für die Probe-Schiffe — inkl. Arrow OHNE Datei-Eintrag.
        let overrides = effective_override_rows(&cache);
        let fixture = serde_json::json!({
            "_doc": "Erzeugt vom Live-Test live_sync_file_only_and_decision (echte Gitea-Raw/Repo-Baseline + reale modul_for_ship_decision-Logik, S4 Nur-Datei-Regel) — NICHT von Hand editieren.",
            "for_ship": {
                "aegis eclipse": modul_for_ship_decision("Aegis Eclipse", &overrides),
                "aegis retaliator": modul_for_ship_decision("Aegis Retaliator", &overrides),
                "rsi galaxy": modul_for_ship_decision("RSI Galaxy", &overrides),
                "rsi aurora mk ii": modul_for_ship_decision("RSI Aurora MK II", &overrides),
                "aegis arrow": modul_for_ship_decision("Aegis Arrow", &overrides),
            }
        });
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../docs/scripts/s4-modul-fixture.json");
        std::fs::write(&path, serde_json::to_string_pretty(&fixture).unwrap()).unwrap();
        println!("fixture geschrieben: {}", path.display());
    }
}
