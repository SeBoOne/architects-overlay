//! # locale
//!
//! T1 (t_530e5f59): lokale (Community-)Sprachdateien für das Overlay.
//!
//! - Built-ins (`en`, `de`) leben im Repo (`resources/locales/*.json`) und
//!   werden vom Frontend STATISCH importiert (Vite) — der Live-Switch braucht
//!   keinen IPC-Roundtrip.
//! - Zusätzlich kann der Nutzer eigene Sprachdateien importieren
//!   (Einstellungen → Sprache → „Übersetzung hinzufügen“, `<input type=file>`
//!   in der Webview liest den Text, `locale_import` validiert + persistiert).
//! - Ablage: `<app-local-data-dir>/locales/<code>.json` (neben cache.sqlite3).
//!
//! Fehler im Locale-Bereich werden als **error_key + params** geliefert
//! (JSON-String als `Err`) — kein Klartext, kein gettext-Layer: das Frontend
//! übersetzt den Key mit derselben Fallback-Kette (gewählt → en → raw Key).
//! Das ist das Muster, auf das T2 die übrigen Rust-UI-Meldungen umstellt.
//!
//! Format der Dateien (built-ins UND Community identisch):
//!
//! ```jsonc
//! { "meta": { "code": "fr", "name": "Français" },
//!   "strings": { "settings.language.title": "Langue", … } }  // flach
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::json;

/// Import-Größenlimit (Geist wie Connector-Manifeste: verhindern, dass eine
/// Riesen-"Übersetzung" den Boot verstopft). 128 KB reichen für tausende Keys.
pub const LOCALE_IMPORT_MAX_BYTES: usize = 128 * 1024;

/// Validiertes Locale (aus Datei oder Import) — der unter
/// `<locales-dir>/<code>.json` abgelegte bzw. gelesene Stand.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocaleMeta {
    /// Sprach-Code (a-z0-9, `-`/`_`, max. 32 Zeichen, bereits normalisiert).
    pub code: String,
    /// Anzeigename (vom Autor der Datei gesetzt, native Schreibweise: „Deutsch“).
    pub name: String,
    /// `false` für lokale Dateien — Built-ins listet das Frontend separat.
    pub builtin: bool,
}

/// Flaches Key→Text-Map (Keys mit Punktnamen, z. B. `nav.trade`).
pub type LocaleStrings = BTreeMap<String, String>;

/// Fehler als Key + Params (UI-Vertrag von T1/T2): serialisiert zu
/// `{"error_key": "...", "params": {...}}` und landet als String im
/// `invoke`-Reject; das Frontend parst ihn (`trError` in `src/lib/i18n.ts`)
/// und übersetzt über dieselbe Fallback-Kette.
#[derive(Debug, Clone, Serialize)]
pub struct LocaleError {
    pub error_key: String,
    pub params: BTreeMap<String, String>,
}

impl LocaleError {
    pub fn new(key: &str, params: &[(&str, String)]) -> Self {
        LocaleError {
            error_key: key.to_owned(),
            params: params
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        }
    }

    pub fn to_json_string(&self) -> String {
        json!({ "error_key": self.error_key, "params": self.params }).to_string()
    }
}

/// Rohes Datei-Layout (wie die Repo-Baseline en.json/de.json).
#[derive(Debug, Deserialize)]
struct RawLocaleFile {
    meta: Option<RawLocaleMeta>,
    strings: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
struct RawLocaleMeta {
    code: Option<serde_json::Value>,
    name: Option<serde_json::Value>,
}

/// Sprach-Code-Regel (Spiegel zu `settings::norm_language`, aber für IMPORT:
/// ein ungültiger Code wird NICHT still auf den Default gefaltet, sondern
/// als Fehler gemeldet — der Nutzer will seine Sprache importieren, still
/// „en“ hinein zu schreiben wäre der stille Tod des Imports).
pub fn is_valid_locale_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= settings::LANGUAGE_MAX_LEN
        && code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// Built-in-Codes: dürfen nicht durch lokale Dateien überschattet werden
/// (sonst würde ein böser/caputter Import `en`/`de` für ALLE Nutzer dieser
/// Installation verändern — die Built-ins sind das, was die Fallback-Kette
/// am Ende garantiert).
pub const BUILTIN_CODES: [&str; 2] = ["en", "de"];

/// Validiert Import-Text → (Meta, Strings). Tauri-frei, damit die Regel
/// cargo-testbar bleibt. Reihenfolge der Checks spiegelt die error_keys
/// (jeder Check seine eigene, präzise Meldung statt einer Sammel-Meldung).
pub fn parse_locale(
    text: &str,
    builtin_codes: &[&str],
) -> Result<(LocaleMeta, LocaleStrings), LocaleError> {
    if text.len() > LOCALE_IMPORT_MAX_BYTES {
        return Err(LocaleError::new(
            "error.locale.import.too_large",
            &[("limit", (LOCALE_IMPORT_MAX_BYTES / 1024).to_string())],
        ));
    }
    let raw: RawLocaleFile = serde_json::from_str(text).map_err(|e| {
        LocaleError::new(
            "error.locale.import.invalid_json",
            // detail kurz halten — serde_errors können kilometerlang werden.
            &[("detail", e.to_string().chars().take(160).collect())],
        )
    })?;
    let meta_raw = raw
        .meta
        .ok_or_else(|| LocaleError::new("error.locale.import.missing_meta", &[]))?;
    let code = meta_raw
        .code
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    let name = meta_raw
        .name
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    if !is_valid_locale_code(&code) {
        return Err(LocaleError::new(
            "error.locale.import.bad_code",
            &[("code", code.clone())],
        ));
    }
    if builtin_codes.contains(&code.as_str()) {
        return Err(LocaleError::new(
            "error.locale.import.builtin_conflict",
            &[("code", code.clone())],
        ));
    }
    if name.trim().is_empty() || name.len() > 64 {
        return Err(LocaleError::new("error.locale.import.missing_meta", &[]));
    }
    let strings_raw = raw
        .strings
        .ok_or_else(|| LocaleError::new("error.locale.import.missing_strings", &[]))?;
    let mut strings = LocaleStrings::new();
    for (k, v) in strings_raw {
        // NUR flache String-Maps: verschachtelte Objekte/Arrays/Zahlen sind
        // kein gültiger Wert — sonst bricht die Fallback-Kette still.
        let s = match v.as_str() {
            Some(s) => s,
            None => return Err(LocaleError::new("error.locale.import.missing_strings", &[])),
        };
        strings.insert(k, s.to_owned());
    }
    Ok((
        LocaleMeta {
            code,
            name: name.trim().to_owned(),
            builtin: false,
        },
        strings,
    ))
}

/// Speicherverzeichnis der lokalen Sprachdateien.
pub fn locales_dir(app: &tauri::AppHandle) -> Result<PathBuf, LocaleError> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| {
            LocaleError::new(
                "error.locale.import.write_failed",
                &[("detail", e.to_string())],
            )
        })?
        .join("locales");
    Ok(dir)
}

/// Eine lokal persistierte Sprachdatei (Meta + Strings) — bewusst als
/// Struct statt als Tupel (Serde serialisiert Tupel als Arrays; das
/// Frontend will `{meta, strings}`-Objekte).
#[derive(Debug, Clone, Serialize)]
pub struct LocaleFileEntry {
    pub meta: LocaleMeta,
    pub strings: LocaleStrings,
}

/// Liste der lokal importierten Sprachen (gefundene Dateien, Sortiert nach
/// Code). Lesefehler einer einzelnen Datei werden NICHT als Fehler
/// hochgeworfen — eine kaputte Datei darf die Sprachliste nicht killen (sie
/// wird einfach ausgelassen; beim Boot fällt die Fallback-Kette auf en).
pub fn list_locales(dir: &std::path::Path) -> Vec<LocaleFileEntry> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    for path in paths {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Ok((meta, strings)) = parse_locale(&text, &BUILTIN_CODES) {
            out.push(LocaleFileEntry { meta, strings });
        }
    }
    out
}

/// ---- Tauri-Commands ------------------------------------------------------
/// Lokale Sprachdateien listet + Strings (das Frontend ergänzt die Built-ins
/// statisch). Ohne Tauri (SSR/dev) wird der Command nicht gerufen.
#[tauri::command]
pub fn locale_list(app: tauri::AppHandle) -> Result<Vec<LocaleFileEntry>, String> {
    let dir = locales_dir(&app).map_err(|e| e.to_json_string())?;
    let _ = std::fs::create_dir_all(&dir);
    Ok(list_locales(&dir))
}

/// Import: validierter JSON-Text aus dem Webview-File-Input → persistiert
/// unter `<locales-dir>/<code>.json`. Fehler als `{"error_key":…}`-String.
#[tauri::command]
pub fn locale_import(app: tauri::AppHandle, text: String) -> Result<LocaleMeta, String> {
    let (meta, strings) = parse_locale(&text, &BUILTIN_CODES).map_err(|e| e.to_json_string())?;
    let dir = locales_dir(&app).map_err(|e| e.to_json_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| {
        LocaleError::new(
            "error.locale.import.write_failed",
            &[("detail", e.to_string())],
        )
        .to_json_string()
    })?;
    let path = dir.join(format!("{}.json", meta.code));
    let mut body = serde_json::Map::new();
    body.insert(
        "meta".to_owned(),
        serde_json::to_value(&meta).expect("LocaleMeta serializes"),
    );
    body.insert(
        "strings".to_owned(),
        serde_json::to_value(&strings).expect("LocaleStrings serializes"),
    );
    let content = serde_json::to_string_pretty(&serde_json::Value::Object(body))
        .expect("locale body serializes");
    // Atomic write (tmp + rename) wie die Settings-Persistenz.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, content).map_err(|e| {
        LocaleError::new(
            "error.locale.import.write_failed",
            &[("detail", e.to_string())],
        )
        .to_json_string()
    })?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        LocaleError::new(
            "error.locale.import.write_failed",
            &[("detail", e.to_string())],
        )
        .to_json_string()
    })?;
    Ok(meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Locale-Parität (de/en): die Repo-Baseline der beiden Builtin-Dateien
    /// muss denselben Key-Bestand haben — fehlende Übersetzung = Build-Fehler
    /// im Test statt stiller Fallback-Kette im Feld (SCLM v2 Karte A Gate).
    #[test]
    fn builtin_locales_have_parity() {
        let read_keys = |name: &str| -> std::collections::BTreeSet<String> {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../resources/locales")
                .join(name);
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            let raw: RawLocaleFile = serde_json::from_str(&text).expect("valid locale json");
            raw.strings
                .unwrap_or_default()
                .into_iter()
                .map(|(k, _)| k)
                .collect::<std::collections::BTreeSet<String>>()
        };
        let de = read_keys("de.json");
        let en = read_keys("en.json");
        let only_de: Vec<&String> = de.difference(&en).collect();
        let only_en: Vec<&String> = en.difference(&de).collect();
        assert!(
            only_de.is_empty() && only_en.is_empty(),
            "locale parity broken — only de: {only_de:?} / only en: {only_en:?}"
        );
    }

    const EXAMPLE: &str = r#"{
        "meta": { "code": "fr", "name": "Français" },
        "strings": { "settings.language.title": "Langue", "nav.trade": "Commerce" }
    }"#;

    #[test]
    fn parse_accepts_valid_import() {
        let (meta, strings) = parse_locale(EXAMPLE, &BUILTIN_CODES).unwrap();
        assert_eq!(meta.code, "fr");
        assert_eq!(meta.name, "Français");
        assert!(!meta.builtin);
        assert_eq!(
            strings.get("nav.trade").map(String::as_str),
            Some("Commerce")
        );
    }

    #[test]
    fn parse_rejects_builtin_conflict() {
        let text = r#"{ "meta": { "code": "de", "name": "Deutsch" }, "strings": {} }"#;
        let err = parse_locale(text, &BUILTIN_CODES).unwrap_err();
        assert_eq!(err.error_key, "error.locale.import.builtin_conflict");
        assert_eq!(err.params.get("code").map(String::as_str), Some("de"));
        // Der Fehler serialisiert als error_key+params (UI-Vertrag T1/T2).
        let s = err.to_json_string();
        assert!(s.contains("error.locale.import.builtin_conflict"));
        assert!(s.contains("params"));
    }

    #[test]
    fn parse_rejects_bad_code_and_missing_blocks() {
        let bad_code = r#"{ "meta": { "code": "FR<script>", "name": "X" }, "strings": {} }"#;
        assert_eq!(
            parse_locale(bad_code, &BUILTIN_CODES)
                .unwrap_err()
                .error_key,
            "error.locale.import.bad_code"
        );
        assert_eq!(
            parse_locale("[]", &BUILTIN_CODES).unwrap_err().error_key,
            "error.locale.import.invalid_json"
        );
        let no_meta = r#"{ "strings": {} }"#;
        assert_eq!(
            parse_locale(no_meta, &BUILTIN_CODES).unwrap_err().error_key,
            "error.locale.import.missing_meta"
        );
        let no_strings = r#"{ "meta": { "code": "fr", "name": "Français" } }"#;
        assert_eq!(
            parse_locale(no_strings, &BUILTIN_CODES)
                .unwrap_err()
                .error_key,
            "error.locale.import.missing_strings"
        );
        let nested =
            r#"{ "meta": { "code": "fr", "name": "Français" }, "strings": { "a": { "b": 1 } } }"#;
        assert_eq!(
            parse_locale(nested, &BUILTIN_CODES).unwrap_err().error_key,
            "error.locale.import.missing_strings"
        );
    }

    #[test]
    fn parse_rejects_oversize() {
        let big = " ".repeat(LOCALE_IMPORT_MAX_BYTES + 1);
        assert_eq!(
            parse_locale(&big, &BUILTIN_CODES).unwrap_err().error_key,
            "error.locale.import.too_large"
        );
    }

    #[test]
    fn list_locales_skips_broken_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("fr.json"), EXAMPLE).unwrap();
        std::fs::write(dir.join("broken.json"), "{ not json").unwrap();
        let list = list_locales(dir);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].meta.code, "fr");
        // Code-Sortierung: zwei valide Dateien kommen alphabetisch.
        std::fs::write(
            dir.join("ab.json"),
            r#"{ "meta": { "code": "ab", "name": "Ab" }, "strings": {} }"#,
        )
        .unwrap();
        let list = list_locales(dir);
        assert_eq!(
            list.iter()
                .map(|e| e.meta.code.as_str())
                .collect::<Vec<_>>(),
            vec!["ab", "fr"]
        );
    }
}
