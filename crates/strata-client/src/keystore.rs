//! Strata-Key-Keystore — connector_keystore-Muster (M7-K1, t_e38faa51).
//!
//! Der `celd_`-Key liegt NICHT in der Settings-Config (dort steht nur die
//! `key_ref`, der Bezeichner des Eintrags), sondern in einer eigenen Datei
//! `strata-key.json` (0o600, atomar tmp+rename — derselbe Mechanismus wie
//! `connector_keystore::save_sessions`). Die App darf KEINEN geteilten Key
//! ausliefern (ToS: Revocation folgt sonst) — jeder Nutzer legt per
//! Self-Service einen eigenen Key an.
//!
//! Reiner Kern ohne Tauri (unit-testbar). Kaputte Datei → leerer Store +
//! `log::warn!` — ein kaputter Keystore darf den Client nie crashen.

use std::collections::BTreeMap;

/// Root-Form der Keystore-Datei: benannte Keys (`key_ref` → `celd_…`).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StrataKeyStore {
    #[serde(default)]
    pub keys: BTreeMap<String, String>,
}

#[cfg(test)]
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Load: fehlend → leer; kaputt (JSON-Fehler) → leer + log (kein Crash).
pub fn load_keys(path: &std::path::Path) -> BTreeMap<String, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<StrataKeyStore>(&text) {
            Ok(k) => k.keys,
            Err(e) => {
                log::warn!("strata-keystore: skip {}: {e}", path.display());
                BTreeMap::new()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(e) => {
            log::warn!("strata-keystore: read {}: {e}", path.display());
            BTreeMap::new()
        }
    }
}

/// Atomares Speichern (tmp+rename, 0o600 — Key bleibt user-only).
pub fn save_keys(path: &std::path::Path, keys: &BTreeMap<String, String>) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(&StrataKeyStore { keys: keys.clone() })?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, content.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp, path)
}

/// Upsert eines Keys (leere/getrimmte Leereingabe = kein Write). `None`-Wert
/// entfernt den Eintrag (Revocation des Nutzers). Persistiert sofort.
pub fn set_key(path: &std::path::Path, key_ref: &str, value: Option<&str>) -> bool {
    let key_ref = key_ref.trim();
    if key_ref.is_empty() {
        return false;
    }
    let mut keys = load_keys(path);
    // Some("")/whitespace = KEIN Write (Upsert mit leerem Patch erhält den
    // Wert — connector_keystore-Vertrag); explizites None entfernt.
    if let Some(v) = value
        && v.trim().is_empty()
    {
        return true;
    }
    let changed = match value.map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) => keys.insert(key_ref.to_owned(), v.to_owned()).as_deref() != Some(v),
        None => keys.remove(key_ref).is_some(),
    };
    if changed && let Err(e) = save_keys(path, &keys) {
        log::warn!("strata-keystore: write {}: {e}", path.display());
        return false;
    }
    true
}

/// `key_ref` → `celd_…`-Key oder None. Fehlende/leere Ref → None (Default-Ref
/// ist `"default"`, siehe [`DEFAULT_KEY_REF`]).
pub const DEFAULT_KEY_REF: &str = "default";

pub fn resolve_key(path: &std::path::Path, key_ref: &str) -> Option<String> {
    let key_ref = key_ref.trim();
    if key_ref.is_empty() {
        return None;
    }
    load_keys(path)
        .get(key_ref)
        .map(|k| k.trim())
        .filter(|k| !k.is_empty())
        .map(str::to_owned)
}

/// Standardpfad-Geschwister-Helfer wie `KeystorePaths` (connector_keystore):
/// `<dir>/strata-key.json`.
pub fn key_file(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("strata-key.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_file(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("strata-ks-{name}-{}", now_secs()));
        let _ = std::fs::create_dir_all(&dir);
        key_file(&dir)
    }

    #[test]
    fn set_then_resolve_roundtrip() {
        let path = tmp_file("roundtrip");
        let _ = std::fs::remove_file(&path);
        assert!(resolve_key(&path, "default").is_none(), "fehlend → None");
        assert!(set_key(&path, "default", Some("celd_abc123")));
        assert_eq!(
            resolve_key(&path, "default").as_deref(),
            Some("celd_abc123")
        );
        // Zweit-Eintrag, Select per key_ref.
        assert!(set_key(&path, "laptop", Some("celd_xyz")));
        assert_eq!(resolve_key(&path, "laptop").as_deref(), Some("celd_xyz"));
        assert_eq!(
            resolve_key(&path, "default").as_deref(),
            Some("celd_abc123")
        );
        // Kaputte Datei → leer, kein Crash.
        std::fs::write(&path, "{ kaputt").unwrap();
        assert!(resolve_key(&path, "default").is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn remove_and_trim_rules() {
        let path = tmp_file("remove");
        let _ = std::fs::remove_file(&path);
        assert!(set_key(&path, "default", Some("  celd_x  ")));
        assert_eq!(resolve_key(&path, "default").as_deref(), Some("celd_x"));
        // Leerer Set überschreibt NICHT (trimmt auf "" → kein Write).
        assert!(set_key(&path, "default", Some("   ")));
        assert_eq!(resolve_key(&path, "default").as_deref(), Some("celd_x"));
        assert!(
            !set_key(&path, " ", Some("celd_y")),
            "leere key_ref → false"
        );
        // None entfernt.
        assert!(set_key(&path, "default", None));
        assert!(resolve_key(&path, "default").is_none());
        // Persistenz-Datei ist 0o600.
        set_key(&path, "default", Some("celd_z"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_file(&path);
    }
}
