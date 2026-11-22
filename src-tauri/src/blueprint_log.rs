//! M9-Q2 (Sebo): Blueprint-Erhalt aus SC-Spiel-Logs.
//!
//! SC schreibt bei jedem Blueprint-Erhalt eine Notice-Zeile in `Game.log`:
//! `[Notice] <SHUDEvent_OnNotification> Added notification "Received Blueprint:
//! NAME: " [n] to queue. …` sowie (je nach Warteschlange) `UpdateNotificationItem`
//! Follow-ups mit demselben Muster. Der Name steht zwischen `Received Blueprint: `
//! und dem nächsten `: "` — Doppelpunkte im Namen selbst sind nicht beobachtet.
//!
//! Zwei Wege:
//! 1. **Backfill** (`blueprint_owned_sync_logs`): alle `Game.log` +
//!    `logbackups/*.log` parsen, Namen gegen den Strata-Item-Katalog matchen
//!    und die Treffer als „im Besitz" persistieren.
//! 2. **Live-Tail** (`start_blueprint_log_tail`): `Game.log` inkrementell
//!    lesen (Offset-Tracking), neue Erhalte sofort matchen + markieren und
//!    das Frontend per `blueprint-owned-changed`-Event informieren.

use std::collections::BTreeSet;
use std::io::{Seek, SeekFrom};
use std::path::PathBuf;

use serde::Serialize;

use tauri::Manager;

use crate::app_settings::SettingsState;
use crate::strata::StrataState;

/// Die Zeichenkette, die jede Blueprint-Erhalt-Zeile enthält.
const MARKER: &str = "Received Blueprint: ";
/// Abschluss des Namens in der Zeile (das Muster endet immer mit `: "`).
const NAME_END: &str = ": \"";

/// Alle Blueprint-Namen aus EINER Log-Zeile. Namen können mehrfach pro Zeile
/// vorkommen (Queue-Dump-Zeilen listen mehrere Benachrichtigungen) und
/// doppelte innerhalb einer Zeile werden dedupliziert.
pub fn parse_received_blueprints(line: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = line;
    while let Some(pos) = rest.find(MARKER) {
        let after = &rest[pos + MARKER.len()..];
        match after.find(NAME_END) {
            Some(end) if end > 0 => {
                let name = after[..end].trim();
                if !name.is_empty() && !out.iter().any(|v| v == name) {
                    out.push(name.to_owned());
                }
                rest = &after[end + NAME_END.len()..];
            }
            // Zeile endet mitten im Namen (unvollständiger Schreib-Flush) —
            // Teilnamen nicht übernehmen.
            _ => break,
        }
    }
    out
}

/// Default-`LIVE`-Ordner je Plattform (Windows-Standard-Install bzw. LUG-
/// Helper-Prefix unter Linux). `~` wird für den Home-Ordner expandiert.
pub fn default_game_log_dir() -> String {
    if cfg!(windows) {
        r"C:\Program Files\Roberts Space Industries\StarCitizen\LIVE".to_owned()
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home".to_owned());
        format!(
            "{home}/Games/star-citizen/drive_c/Program Files/Roberts Space Industries/StarCitizen/LIVE"
        )
    }
}

/// Effektiver Log-Ordner: Settings-Wert oder Default.
fn effective_dir(settings: &settings::Settings) -> PathBuf {
    PathBuf::from(
        settings
            .game_log_dir()
            .map(str::to_owned)
            .unwrap_or_else(default_game_log_dir),
    )
}

/// Alle auszuwertenden Log-Dateien: `Game.log` (aktuelle Session) +
/// `logbackups/*.log` (vergangene Sessions). Es ist kein Fehler, wenn
/// Dateien/Ordner fehlen — dann ist die Liste leer.
fn collect_log_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let game_log = dir.join("Game.log");
    if game_log.is_file() {
        files.push(game_log);
    }
    if let Ok(rd) = std::fs::read_dir(dir.join("logbackups")) {
        let mut entries: Vec<PathBuf> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("log"))
            })
            .collect();
        entries.sort();
        files.extend(entries);
    }
    files
}

/// Alle Blueprint-Namen aus einer Liste von Log-Dateien (UTF-8-Verluste
/// pro Zeile toleriert — Windows-Logs sind nicht garantiert sauber UTF-8).
fn names_from_files(files: &[PathBuf]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for path in files {
        let Ok(content) = std::fs::read(path) else {
            continue;
        };
        let text = String::from_utf8_lossy(&content);
        for line in text.lines() {
            for name in parse_received_blueprints(line) {
                out.insert(name);
            }
        }
    }
    out
}

/// Report des Backfill-Syncs (UI-Zusammenfassung).
#[derive(Debug, Clone, Serialize)]
pub struct BlueprintSyncReport {
    /// Log-Dateien ausgewertet.
    pub files: usize,
    /// Eindeutige Blueprint-Namen in den Logs.
    pub names: usize,
    /// Davon gegen den Strata-Katalog gematcht (jetzt markiert).
    pub matched: usize,
    /// NEU hinzugefügte Besitz-IDs (vorher nicht markiert).
    pub added: usize,
    /// Namen ohne Katalog-Treffer (max. 25 für die UI).
    pub unmatched: Vec<String>,
}

/// Match Log-Namen gegen den Strata-Item-Katalog → (id, name-lookup-Tabelle).
/// Vergleich case-insensitiv exakt (die Log-Namen sind die Spiel-Namen).
async fn strata_items_lookup(
    strata: &StrataState,
    settings: &settings::Settings,
) -> Result<Vec<(String, String)>, String> {
    let c = crate::strata::client_for(strata, settings)?;
    let items = c.items().await.map_err(|e| crate::strata::strata_error(&e))?;
    Ok(items
        .items
        .iter()
        .map(|it| (it.name.to_lowercase(), it.id.clone()))
        .collect())
}

/// Backfill: alle Spiel-Logs parsen, Namen gegen den Strata-Katalog matchen
/// und die Treffer in `blueprint_owned` aufnehmen (merge, dedupliziert).
#[tauri::command]
pub async fn blueprint_owned_sync_logs(
    strata: tauri::State<'_, StrataState>,
    settings: tauri::State<'_, SettingsState>,
) -> Result<BlueprintSyncReport, String> {
    let snapshot = crate::strata::snapshot_of(&settings);
    let dir = effective_dir(&snapshot);
    let files = collect_log_files(&dir);
    let names = names_from_files(&files);
    let lookup = strata_items_lookup(&strata, &snapshot).await?;
    let mut added_ids: Vec<String> = Vec::new();
    let mut matched = 0usize;
    let mut unmatched: Vec<String> = Vec::new();
    {
        let mut s = settings.inner.lock().expect("settings lock");
        let owned: std::collections::HashSet<String> =
            s.blueprint_owned().iter().cloned().collect();
        for name in &names {
            let ln = name.to_lowercase();
            match lookup.iter().find(|(n, _)| *n == ln) {
                Some((_, id)) => {
                    matched += 1;
                    if !owned.contains(id) {
                        added_ids.push(id.clone());
                    }
                }
                None => unmatched.push(name.clone()),
            }
        }
        if !added_ids.is_empty() {
            let mut merged = s.blueprint_owned().to_vec();
            merged.extend(added_ids.iter().cloned());
            s.set_blueprint_owned(&merged);
            crate::app_settings::persist_settings(&s);
        }
    }
    unmatched.truncate(25);
    Ok(BlueprintSyncReport {
        files: files.len(),
        names: names.len(),
        matched,
        added: added_ids.len(),
        unmatched,
    })
}

/// Event-Payload für das Frontend.
#[derive(Debug, Clone, Serialize)]
pub struct BlueprintOwnedChanged {
    /// Neu hinzugefügte Besitz-IDs.
    pub added_ids: Vec<String>,
    /// Blueprint-Namen der Treffer (für Hinweis-Texte).
    pub names: Vec<String>,
}

/// Live-Tail-State: Datei-Identität + Offset der zuletzt gelesenen Position.
struct TailState {
    dev_ino: Option<(u64, u64)>,
    offset: u64,
}

/// Live-Tail: `Game.log` inkrementell lesen und frische Blueprint-Erhalte
/// sofort markieren. Poll alle 3 s — nur bei geänderter Datei-Länge wird
/// gelesen (Metadaten-Check, kein ständiges Read). Läuft für die gesamte
/// App-Laufzeit; fehlender Pfad = stiller Idle-Ordner.
pub fn start_blueprint_log_tail(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut tail = TailState {
            dev_ino: None,
            offset: 0,
        };
        let mut item_cache: Option<Vec<(String, String)>> = None; // (lowercase name, id)
        let mut item_cache_at: Option<std::time::Instant> = None;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            let settings_state = app.state::<SettingsState>();
            let strata_state = app.state::<StrataState>();
            let snapshot = crate::strata::snapshot_of(&settings_state);
            let dir = effective_dir(&snapshot);
            let game_log = dir.join("Game.log");
            let Ok(meta) = std::fs::metadata(&game_log) else {
                tail = TailState {
                    dev_ino: None,
                    offset: 0,
                };
                continue;
            };
            let dev_ino = file_dev_ino(&meta);
            if tail.dev_ino != dev_ino {
                // Neue Datei (Rotation/Neustart): Offset zurücksetzen.
                tail.dev_ino = dev_ino;
                tail.offset = 0;
            }
            let len = meta.len();
            if len <= tail.offset {
                continue;
            }
            let appended = match read_from(&game_log, tail.offset) {
                Ok((bytes, new_offset)) => {
                    tail.offset = new_offset;
                    bytes
                }
                Err(_) => continue,
            };
            let text = String::from_utf8_lossy(&appended);
            let mut names: BTreeSet<String> = BTreeSet::new();
            for line in text.lines() {
                for name in parse_received_blueprints(line) {
                    names.insert(name);
                }
            }
            if names.is_empty() {
                continue;
            }
            // Item-Katalog: Cache 10 min, sonst frisch (Key kann sich ändern).
            let fresh = item_cache_at
                .map(|t| t.elapsed() >= std::time::Duration::from_secs(600))
                .unwrap_or(true);
            if fresh {
                if let Ok(list) = strata_items_lookup(&strata_state, &snapshot).await {
                    item_cache = Some(list);
                    item_cache_at = Some(std::time::Instant::now());
                }
            }
            let lookup = item_cache.clone().unwrap_or_default();
            let mut added_ids: Vec<String> = Vec::new();
            let mut matched_names: Vec<String> = Vec::new();
            {
                let mut s = settings_state.inner.lock().expect("settings lock");
                let owned: std::collections::HashSet<String> =
                    s.blueprint_owned().iter().cloned().collect();
                for name in &names {
                    let ln = name.to_lowercase();
                    if let Some((_, id)) = lookup.iter().find(|(n, _)| *n == ln) {
                        if !owned.contains(id) {
                            added_ids.push(id.clone());
                            matched_names.push(name.clone());
                        }
                    }
                }
                if !added_ids.is_empty() {
                    let mut merged = s.blueprint_owned().to_vec();
                    merged.extend(added_ids.iter().cloned());
                    s.set_blueprint_owned(&merged);
                    crate::app_settings::persist_settings(&s);
                }
            }
            if !added_ids.is_empty() {
                use tauri::Emitter;
                let _ = app.emit(
                    "blueprint-owned-changed",
                    BlueprintOwnedChanged {
                        added_ids,
                        names: matched_names,
                    },
                );
            }
        }
    });
}

/// (dev, ino) einer Datei — Rotation-Erkennung. Windows: None (keine
/// portablen dev/ino) — Rotation wird dort über die Offset-Logik abgedeckt
/// (neue Datei kürzer als Offset → Offset-Reset).
fn file_dev_ino(meta: &std::fs::Metadata) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some((meta.dev(), meta.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        None
    }
}

/// Ab einem Offset lesen und die neue Position zurückgeben.
fn read_from(path: &std::path::Path, offset: u64) -> std::io::Result<(Vec<u8>, u64)> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let new_offset = offset + buf.len() as u64;
    Ok((buf, new_offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_add_notification() {
        let line = "<2026-11-14T20:05:38.516Z> [Notice] <SHUDEvent_OnNotification> Added notification \"Received Blueprint: Skarnov \"Raider Camo\" Rifle: \" [242] to queue. New queue size: 2, MissionId: [00000000-0000-0000-0000-000000000000], ObjectiveId: [] [Team_CoreGameplayFeatures][Missions][Comms]";
        assert_eq!(
            parse_received_blueprints(line),
            vec!["Skarnov \"Raider Camo\" Rifle"]
        );
    }

    #[test]
    fn parse_update_notification() {
        let line = "<2026-11-14T20:05:39.511Z> [Notice] <UpdateNotificationItem> Notification \"Received Blueprint: Palatino Core Raider Camo: \" [5], Action: Next [Team_CoreGameplayFeatures][Missions][Comms]";
        assert_eq!(
            parse_received_blueprints(line),
            vec!["Palatino Core Raider Camo"]
        );
    }

    #[test]
    fn parse_queue_dump_multiple() {
        let line = "\"Received Blueprint: Palatino Core Raider Camo: \" [5] \"Received Blueprint: Palatino Backpack Raider Camo: \" [6]";
        assert_eq!(
            parse_received_blueprints(line),
            vec!["Palatino Core Raider Camo", "Palatino Backpack Raider Camo"]
        );
    }

    #[test]
    fn parse_no_marker() {
        assert!(parse_received_blueprints("Nothing interesting here").is_empty());
    }

    #[test]
    fn parse_dedupes_within_line() {
        let line = "\"Received Blueprint: Foo: \" [1] \"Received Blueprint: Foo: \" [1]";
        assert_eq!(parse_received_blueprints(line), vec!["Foo"]);
    }

    #[test]
    fn parse_truncated_name_skipped() {
        let line = "\"Received Blueprint: Half Name";
        assert!(parse_received_blueprints(line).is_empty());
    }
}
