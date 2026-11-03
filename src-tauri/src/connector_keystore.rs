//! # connector_keystore — Session-Keystore pro Connector (SCLM v2 Karte A, Spec 5.2)
//!
//! Lokale, pro Connector persistierte JSON-Liste bekannter Sessions
//! (`<app-local-data-dir>/connector-state/<id>.json`):
//! `sessions[{sid, join_code, name, participant_id?, pcode?, result_id?, added}]`.
//! Geschrieben bei Code-Eingabe (join_code+sid), Auto-Join/Claim
//! (participant_id+pcode) und erster Ergebnis-Sicht (result_id). Gelesen von
//! UI (B) und Flow-Pins (C) — "Öffnen"-Button genau dann, wenn der `sid`
//! bereits im Keystore liegt.
//!
//! Reiner Kern ohne Tauri (unit-testbar). Schreiben atomar (tmp+rename,
//! 0o600 wie Manifest-Store): derselbe persist()-Mechanismus wie
//! `ConnectorState::persist`. Kaputte Dateien fallen auf leere Liste zurück —
//! ein kaputter Keystore darf Flow/UI nie crashen.

use serde::{Deserialize, Serialize};

/// Ein Keystore-Eintrag pro bekannter Session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEntry {
    pub sid: String,
    #[serde(default)]
    pub join_code: String,
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub participant_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pcode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_id: Option<String>,
    /// Unix-Sekunden der ersten Speicherung.
    #[serde(default)]
    pub added: i64,
}

/// Root-Form des Keystore-Files (Spec 5.2): `{ "sessions": [...] }`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Keystore {
    #[serde(default)]
    pub sessions: Vec<SessionEntry>,
}

/// Wo der Keystore pro Connector liegt (Manifest-Store-Geschwister).
pub struct KeystorePaths {
    pub state_dir: std::path::PathBuf,
}

impl KeystorePaths {
    /// `connector-state/` als Geschwister des Manifest-Verzeichnisses
    /// (`<app-local-data-dir>/connectors/` → `<app-local-data-dir>/connector-state/`).
    pub fn from_manifest_dir(manifest_dir: &std::path::Path) -> KeystorePaths {
        let state_dir = manifest_dir
            .parent()
            .map(|p| p.join("connector-state"))
            .unwrap_or_else(|| std::path::PathBuf::from("connector-state"));
        KeystorePaths { state_dir }
    }

    pub fn file(&self, id: &str) -> std::path::PathBuf {
        self.state_dir.join(format!("{id}.json"))
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn trimmed(s: &Option<String>) -> Option<String> {
    s.as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

/// Load: fehlend → leer; kaputt (JSON-Fehler) → leer + log (Boot/UI-Rule wie
/// beim Manifest-Store: nie crashen, nur überspringen).
pub fn load_sessions(path: &std::path::Path) -> Vec<SessionEntry> {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Keystore>(&text) {
            Ok(k) => k.sessions,
            Err(e) => {
                log::warn!("connector-keystore: skip {}: {e}", path.display());
                Vec::new()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => {
            log::warn!("connector-keystore: read {}: {e}", path.display());
            Vec::new()
        }
    }
}

/// Atomares Speichern (tmp+rename, 0o600 wie Manifest-Store).
pub fn save_sessions(path: &std::path::Path, sessions: &[SessionEntry]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let keystore = Keystore {
        sessions: sessions.to_vec(),
    };
    let content = serde_json::to_string_pretty(&keystore)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, content.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp, path)
}

/// Upsert mit PARTIELLEM Patch: nur die übergebenen Felder werden gesetzt —
/// vorhandene Einträge behalten ihre übrigen Werte (pcode darf nie versehentlich
/// überschrieben/verloren gehen, Spec 5.2). sid ist Identität (case-sensitive).
/// Gibt den fusionierten Eintrag zurück.
#[allow(clippy::too_many_arguments)]
pub fn upsert_entry(
    sessions: &mut Vec<SessionEntry>,
    sid: &str,
    join_code: Option<String>,
    name: Option<String>,
    participant_id: Option<String>,
    pcode: Option<String>,
    result_id: Option<String>,
    now: i64,
) -> SessionEntry {
    let sid = sid.trim().to_owned();
    if let Some(existing) = sessions.iter_mut().find(|e| e.sid == sid) {
        if let Some(v) = trimmed(&join_code) {
            existing.join_code = v;
        }
        if let Some(v) = trimmed(&name) {
            existing.name = v;
        }
        if let Some(v) = trimmed(&participant_id) {
            existing.participant_id = Some(v);
        }
        if let Some(v) = trimmed(&pcode) {
            existing.pcode = Some(v);
        }
        if let Some(v) = trimmed(&result_id) {
            existing.result_id = Some(v);
        }
        return existing.clone();
    }
    let entry = SessionEntry {
        sid,
        join_code: trimmed(&join_code).unwrap_or_default(),
        name: trimmed(&name).unwrap_or_default(),
        participant_id: trimmed(&participant_id),
        pcode: trimmed(&pcode),
        result_id: trimmed(&result_id),
        added: now,
    };
    sessions.push(entry.clone());
    entry
}

/// find by sid (exakt).
pub fn find_entry(sessions: &[SessionEntry], sid: &str) -> Option<SessionEntry> {
    let sid = sid.trim();
    sessions.iter().find(|e| e.sid == sid).cloned()
}

/// list — neueste zuerst (added absteigend; stabile Reihenfolge für UI).
pub fn list_entries(sessions: &[SessionEntry]) -> Vec<SessionEntry> {
    let mut out = sessions.to_vec();
    out.sort_by_key(|e| std::cmp::Reverse(e.added));
    out
}

/// Convenience für die Commands: Load → Upsert (partial) → Save, gibt den
/// fusionierten Eintrag zurück.
#[allow(clippy::too_many_arguments)]
pub fn upsert_and_save(
    path: &std::path::Path,
    sid: &str,
    join_code: Option<String>,
    name: Option<String>,
    participant_id: Option<String>,
    pcode: Option<String>,
    result_id: Option<String>,
) -> SessionEntry {
    let mut sessions = load_sessions(path);
    let entry = upsert_entry(
        &mut sessions,
        sid,
        join_code,
        name,
        participant_id,
        pcode,
        result_id,
        now_secs(),
    );
    if let Err(e) = save_sessions(path, &sessions) {
        log::warn!("connector-keystore: write {}: {e}", path.display());
    }
    entry
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_file(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ao-keystore-{}", name));
        let _ = std::fs::create_dir_all(&dir);
        dir.join("sclm.json")
    }

    #[test]
    fn upsert_creates_then_patches_partially() {
        let mut sessions: Vec<SessionEntry> = Vec::new();
        let created = upsert_entry(
            &mut sessions,
            "9k2m4x7q",
            Some("ABC123".into()),
            Some("Jumptown Raid #4".into()),
            None,
            None,
            None,
            100,
        );
        assert_eq!(created.sid, "9k2m4x7q");
        assert_eq!(created.join_code, "ABC123");
        assert_eq!(created.added, 100);
        assert!(created.participant_id.is_none());

        // Späterer Join/Claim-Patch: setzt participant_id+pcode, LASSEN die
        // bestehenden Werte (join_code/name/added bleiben).
        let patched = upsert_entry(
            &mut sessions,
            "9k2m4x7q",
            None,
            None,
            Some("p8n24rt".into()),
            Some("pc0123456789ab".into()),
            None,
            999,
        );
        assert_eq!(patched.join_code, "ABC123");
        assert_eq!(patched.name, "Jumptown Raid #4");
        assert_eq!(patched.added, 100, "added bleibt beim Patch erhalten");
        assert_eq!(patched.participant_id.as_deref(), Some("p8n24rt"));
        assert_eq!(sessions.len(), 1, "kein Duplikat bei gleichem sid");

        // Ergebnis-Sicht: nur result_id wird gesetzt.
        let done = upsert_entry(
            &mut sessions,
            "9k2m4x7q",
            None,
            None,
            None,
            None,
            Some("r3t4".into()),
            999,
        );
        assert_eq!(done.result_id.as_deref(), Some("r3t4"));
        assert_eq!(done.pcode.as_deref(), Some("pc0123456789ab"));
        assert_eq!(sessions.len(), 1);
    }

    #[test]
    fn upsert_ignores_empty_values_and_trims() {
        let mut sessions: Vec<SessionEntry> = Vec::new();
        upsert_entry(
            &mut sessions,
            " sid1 ",
            Some("  ".into()),
            Some("Name".into()),
            None,
            None,
            None,
            1,
        );
        assert_eq!(sessions[0].sid, "sid1");
        assert_eq!(sessions[0].join_code, "", "leeres Feld = nicht gesetzt");
        // Leerer Patch auf vorhandenen Wert überschreibt NICHT.
        upsert_entry(
            &mut sessions,
            "sid1",
            Some("".into()),
            None,
            None,
            None,
            None,
            2,
        );
        assert_eq!(sessions[0].join_code, "");
        upsert_entry(
            &mut sessions,
            "sid1",
            Some("CODE".into()),
            None,
            None,
            None,
            None,
            2,
        );
        upsert_entry(
            &mut sessions,
            "sid1",
            Some("  ".into()),
            None,
            None,
            None,
            None,
            3,
        );
        assert_eq!(
            sessions[0].join_code, "CODE",
            "leerer Patch erhält den Wert"
        );
    }

    #[test]
    fn load_roundtrip_and_corrupt_fallback_empty() {
        let path = tmp_file("roundtrip");
        let _ = std::fs::remove_file(&path);
        let mut sessions: Vec<SessionEntry> = Vec::new();
        upsert_entry(
            &mut sessions,
            "a1",
            Some("C1".into()),
            Some("Event A".into()),
            Some("pa".into()),
            Some("pcaa".into()),
            None,
            10,
        );
        upsert_entry(
            &mut sessions,
            "b2",
            Some("C2".into()),
            None,
            None,
            None,
            None,
            20,
        );
        save_sessions(&path, &sessions).expect("save");

        let loaded = load_sessions(&path);
        assert_eq!(loaded, sessions);

        // Korrupte Datei → leer (Fallback, kein Crash — Spec 5.2 Acceptance).
        std::fs::write(&path, "{ nicht json").unwrap();
        assert!(load_sessions(&path).is_empty());

        // Fehlende Datei → leer.
        let missing = path.with_extension("missing.json");
        let _ = std::fs::remove_file(&missing);
        assert!(load_sessions(&missing).is_empty());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn find_and_list_order() {
        let mut sessions: Vec<SessionEntry> = Vec::new();
        upsert_entry(&mut sessions, "old", None, None, None, None, None, 10);
        upsert_entry(&mut sessions, "new", None, None, None, None, None, 30);
        upsert_entry(&mut sessions, "mid", None, None, None, None, None, 20);
        let listed = list_entries(&sessions);
        assert_eq!(
            listed.iter().map(|e| e.sid.as_str()).collect::<Vec<_>>(),
            vec!["new", "mid", "old"]
        );
        assert_eq!(find_entry(&sessions, "mid").unwrap().added, 20);
        assert!(find_entry(&sessions, "ghost").is_none());
    }

    #[test]
    fn upsert_and_save_roundtrip_via_file() {
        let path = tmp_file("cmdlike");
        let _ = std::fs::remove_file(&path);
        let e1 = upsert_and_save(
            &path,
            "s1",
            Some("JC".into()),
            Some("Session".into()),
            None,
            None,
            None,
        );
        assert_eq!(e1.join_code, "JC");
        // Command-artiger Zweitschritt liest den persistenten Stand.
        let e2 = upsert_and_save(
            &path,
            "s1",
            None,
            None,
            Some("pid".into()),
            Some("pc".into()),
            None,
        );
        assert_eq!(e2.join_code, "JC");
        assert_eq!(e2.participant_id.as_deref(), Some("pid"));
        assert_eq!(load_sessions(&path).len(), 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn paths_are_sibling_of_manifest_store() {
        let p = KeystorePaths::from_manifest_dir(std::path::Path::new("/data/app/connectors"));
        assert_eq!(
            p.state_dir,
            std::path::PathBuf::from("/data/app/connector-state")
        );
        assert_eq!(
            p.file("sclm"),
            std::path::PathBuf::from("/data/app/connector-state/sclm.json")
        );
        // Relative Manifest-Dir ohne Parent → Fallback-Ordnername.
        let rel = KeystorePaths::from_manifest_dir(std::path::Path::new("connectors"));
        assert_eq!(rel.state_dir, std::path::PathBuf::from("connector-state"));
    }
}
