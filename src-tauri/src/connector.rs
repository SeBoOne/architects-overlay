//! # connector — Manifest-Connector-Verwaltung (C2, t_91124322)
//!
//! Generische Verwaltung deklarativer JSON-Manifeste (`crates/connector`
//! liefert Schema + Validator + Dot-Path-Mapping — hier lebt KEINE
//! connector-spezifische Logik, auch keine SCLM-Hardcoding):
//!
//! - Import: Frontend liest die lokale Datei (`<input type=file>`),
//!   `connector_import` validiert (streng; `ignore_warnings` = bewusste
//!   „Warnung ignorieren"-Option, Verantwortung des Nutzers) und legt eine
//!   Kopie unter `<app-local-data-dir>/connectors/<id>.json` ab.
//! - Liste: vollständige Manifeste (KEIN Secret-Anteil — Auth-Werte leben in
//!   der Settings-Config und verlassen das Backend nie, nur `_set`-Booleans).
//! - Toggle/Remove: wirken über `settings::connector_*` (Placement
//!   erscheint/verschwindet; Remove räumt Toggle/Auth/Hotkey mit auf).
//! - Fetch: GET `base_url + endpoint` mit Header-Auth aus den Settings,
//!   TTL-Cache im State, Dot-Path-Mapping gemäß Manifest (`mapped`) plus
//!   Rohantwort (`body`).
//!
//! Fehler als **error_key + params** (LocaleError-Muster wie locale.rs) —
//! das Frontend übersetzt über dieselbe Fallback-Kette.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::json;

use crate::i18n_err::{err_key, err_keyp};
use crate::locale::LocaleError;

/// Connector-Manifeste sind klein; 1 MiB Obergrenze (gleiche Geist wie
/// Locale-Importe — verhindern, dass ein Riesen-Manifest Boot/Poll verstopft).
pub use connector::MAX_MANIFEST_BYTES;

/// Standard-TTL-Cache-Dauer (ms) für Views ohne `poll_seconds`? — NEIN:
/// `poll_seconds` steuert das Polling-Intervall der UI; der Cache-TTL ist
/// derselbe Wert (Refresh vor Ablauf = cachefrisch, Force = echte Abfrage).
const DEFAULT_POLL_SECONDS: u64 = 300;

#[derive(Debug, Clone, Serialize)]
pub struct ConnectorEntry {
    pub id: String,
    /// Vollständiges Manifest (validiert, gespeicherter Stand).
    pub manifest: serde_json::Value,
    pub enabled: bool,
    /// Pro deklariertem `value_source`-Key: ist ein Wert gesetzt? Der Wert
    /// selbst verlässt das Backend nie (UEX-Secret-Mechanik).
    pub auth_set: BTreeMap<String, bool>,
    /// Konfigurierter Hotkey-Slot (nur placement.kind=own relevant).
    pub hotkey: Option<String>,
}

impl ConnectorEntry {
    fn from_parts(id: &str, manifest: &serde_json::Value, s: &settings::Settings) -> ConnectorEntry {
        // Auth-Deklaration aus dem Manifest lesen (value_source-Keys).
        let mut auth_set = BTreeMap::new();
        if let Some(auth) = manifest.get("auth").and_then(|a| a.get("value_source")) {
            if let Some(key) = auth.as_str() {
                auth_set.insert(key.to_owned(), s.connector_auth_value(id, key).is_some());
            }
        }
        ConnectorEntry {
            id: id.to_owned(),
            manifest: manifest.clone(),
            enabled: s.connector_enabled(id),
            auth_set,
            hotkey: s.connector_hotkey(id).map(str::to_owned),
        }
    }
}

#[derive(Debug, Clone)]
struct StoredConnector {
    /// Persistierter, validierter Manifest-Text (genau der gespeicherte Stand).
    raw: String,
    manifest: connector::ConnectorManifest,
    /// Derselbe Stand als JSON-Value (für die Frontend-Liste).
    value: serde_json::Value,
}

#[derive(Debug, Clone)]
struct CachedFetch {
    fetched_at_ms: u64,
    status: u16,
    body: serde_json::Value,
}

pub struct ConnectorState {
    dir: PathBuf,
    entries: std::sync::Mutex<Vec<StoredConnector>>,
    cache: std::sync::Mutex<HashMap<(String, String), CachedFetch>>,
}

impl ConnectorState {
    /// Setup-Hang: Verzeichnis lesen, alle gespeicherten Manifeste validieren
    /// (kaputte Dateien werden übersprungen + geloggt — Boot darf nie an
    /// einem Connector scheitern).
    pub fn load(dir: PathBuf) -> ConnectorState {
        let entries = std::fs::read_dir(&dir)
            .map(|rd| {
                let mut found: Vec<StoredConnector> = Vec::new();
                for item in rd.flatten() {
                    let path = item.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("json") {
                        continue;
                    }
                    if let Ok(text) = std::fs::read_to_string(&path) {
                        match connector::ConnectorManifest::parse(text.as_bytes()) {
                            Ok(m) => found.push(StoredConnector {
                                value: serde_json::from_str(&text)
                                    .unwrap_or(serde_json::Value::Null),
                                raw: text,
                                manifest: m,
                            }),
                            Err(e) => log::warn!("connector: skip {}: {e}", path.display()),
                        }
                    }
                }
                found.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
                found
            })
            .unwrap_or_default();
        ConnectorState {
            dir,
            entries: std::sync::Mutex::new(entries),
            cache: std::sync::Mutex::new(HashMap::new()),
        }
    }

    fn list_unlocked(&self, s: &settings::Settings) -> Vec<ConnectorEntry> {
        let entries = self.entries.lock().expect("connector entries lock");
        entries
            .iter()
            .map(|st| ConnectorEntry::from_parts(&st.manifest.id, &st.value, s))
            .collect()
    }

    fn persist(&self, st: &StoredConnector) -> Result<(), LocaleError> {
        std::fs::create_dir_all(&self.dir).map_err(|e| {
            LocaleError::new("error.connector.write_failed", &[("detail", e.to_string())])
        })?;
        let path = self.dir.join(format!("{}.json", st.manifest.id));
        let tmp = path.with_extension("json.tmp");
        let write = |p: &std::path::Path| -> std::io::Result<()> {
            std::fs::write(p, st.raw.as_bytes())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600))?;
            }
            Ok(())
        };
        write(&tmp)
            .and_then(|_| std::fs::rename(&tmp, &path))
            .map_err(|e| {
                LocaleError::new("error.connector.write_failed", &[("detail", e.to_string())])
            })
    }
}

/// Reines Parse+Validate (tauri-frei, unit-testbar). `ignore_warnings` lässt
/// WARN-Level durch (nicht Errors) — bewusste Nutzer-Verantwortung.
pub fn parse_and_validate(
    text: &str,
    ignore_warnings: bool,
) -> Result<connector::ConnectorManifest, LocaleError> {
    if text.len() > MAX_MANIFEST_BYTES {
        return Err(LocaleError::new(
            "error.connector.too_large",
            &[("limit", (MAX_MANIFEST_BYTES / 1024).to_string())],
        ));
    }
    let manifest = connector::ConnectorManifest::parse(text.as_bytes()).map_err(|e| {
        LocaleError::new("error.connector.invalid_json", &[("detail", e.to_string())])
    })?;
    let report = connector::validate_manifest(text.as_bytes());
    if !report.is_ok() {
        let items: Vec<String> = report
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect();
        return Err(LocaleError::new(
            "error.connector.validation_failed",
            &[("details", items.join("; "))],
        ));
    }
    if !ignore_warnings && !report.warnings.is_empty() {
        let items: Vec<String> = report.warnings.iter().map(|w| w.to_string()).collect();
        return Err(LocaleError::new(
            "error.connector.warnings",
            &[("details", items.join("; "))],
        ));
    }
    Ok(manifest)
}

/// Importiert (oder ersetzt bei gleicher ID) ein Manifest. Antwort: die
/// frische Liste (das Frontend rendert direkt daraus).
#[tauri::command]
pub async fn connector_import(
    state: tauri::State<'_, ConnectorState>,
    settings_state: tauri::State<'_, crate::app_settings::SettingsState>,
    manifest_json: String,
    ignore_warnings: bool,
) -> Result<Vec<ConnectorEntry>, String> {
    let manifest = parse_and_validate(&manifest_json, ignore_warnings)
        .map_err(|e| e.to_json_string())?;
    let st = StoredConnector {
        value: serde_json::from_str(&manifest_json).unwrap_or(serde_json::Value::Null),
        raw: manifest_json,
        manifest,
    };
    state.persist(&st).map_err(|e| e.to_json_string())?;
    {
        let mut entries = state.entries.lock().expect("connector entries lock");
        entries.retain(|e| e.manifest.id != st.manifest.id);
        let pos = entries
            .binary_search_by(|e| e.manifest.id.cmp(&st.manifest.id))
            .unwrap_or_else(|i| i);
        entries.insert(pos, st);
    }
    state.cache.lock().expect("cache lock").clear();
    Ok(list_cmd_inner(&state, &settings_state))
}

fn list_cmd_inner(
    state: &ConnectorState,
    settings_state: &crate::app_settings::SettingsState,
) -> Vec<ConnectorEntry> {
    let s = settings_state.inner.lock().expect("settings lock").clone();
    state.list_unlocked(&s)
}

#[tauri::command]
pub async fn connector_list(
    state: tauri::State<'_, ConnectorState>,
    settings_state: tauri::State<'_, crate::app_settings::SettingsState>,
) -> Result<Vec<ConnectorEntry>, String> {
    Ok(list_cmd_inner(&state, &settings_state))
}

#[tauri::command]
pub async fn connector_remove(
    state: tauri::State<'_, ConnectorState>,
    settings_state: tauri::State<'_, crate::app_settings::SettingsState>,
    id: String,
) -> Result<Vec<ConnectorEntry>, String> {
    let removed = {
        let mut entries = state.entries.lock().expect("connector entries lock");
        let before = entries.len();
        entries.retain(|e| e.manifest.id != id);
        entries.len() != before
    };
    if removed {
        let _ = std::fs::remove_file(state.dir.join(format!("{id}.json")));
    }
    {
        let mut s = settings_state.inner.lock().expect("settings lock");
        s.remove_connector_state(&id);
        crate::app_settings::persist_settings(&s);
    }
    state.cache.lock().expect("cache lock").clear();
    Ok(list_cmd_inner(&state, &settings_state))
}

#[tauri::command]
pub async fn connector_set_enabled(
    state: tauri::State<'_, ConnectorState>,
    settings_state: tauri::State<'_, crate::app_settings::SettingsState>,
    id: String,
    on: bool,
) -> Result<Vec<ConnectorEntry>, String> {
    {
        let entries = state.entries.lock().expect("connector entries lock");
        if !entries.iter().any(|e| e.manifest.id == id) {
            return Err(err_key("error.connector.unknown"))
        }
    }
    {
        let mut s = settings_state.inner.lock().expect("settings lock");
        s.set_connector_enabled(&id, on);
        crate::app_settings::persist_settings(&s);
    }
    state.cache.lock().expect("cache lock").clear();
    Ok(list_cmd_inner(&state, &settings_state))
}

#[tauri::command]
pub async fn connector_set_auth_value(
    state: tauri::State<'_, ConnectorState>,
    settings_state: tauri::State<'_, crate::app_settings::SettingsState>,
    id: String,
    key: String,
    value: Option<String>,
) -> Result<Vec<ConnectorEntry>, String> {
    // Key MUSS vom Manifest deklariert sein — kein wilder Settings-Schmuggel.
    let declared = {
        let entries = state.entries.lock().expect("connector entries lock");
        entries
            .iter()
            .find(|e| e.manifest.id == id)
            .and_then(|e| e.manifest.auth.as_ref())
            .and_then(|a| a.value_source.clone())
            .map(|v| v == key)
            .unwrap_or(false)
    };
    if !declared {
        return Err(err_key("error.connector.auth_undeclared"));
    }
    {
        let mut s = settings_state.inner.lock().expect("settings lock");
        s.set_connector_auth_value(&id, &key, value.filter(|v| !v.trim().is_empty()));
        crate::app_settings::persist_settings(&s);
    }
    state.cache.lock().expect("cache lock").clear();
    Ok(list_cmd_inner(&state, &settings_state))
}

/// Hotkey-Slot eines own-Connectors setzen/entfernen (None/leer = löschen).
/// Validierung wie settings_set_mode_hotkey: nur F-Keys, F5 reserviert, keine
/// Doppelbelegung mit den festen Mode-Slots oder anderen Connector-Slots.
#[tauri::command]
pub async fn connector_set_hotkey(
    state: tauri::State<'_, ConnectorState>,
    settings_state: tauri::State<'_, crate::app_settings::SettingsState>,
    id: String,
    key: Option<String>,
) -> Result<Vec<ConnectorEntry>, String> {
    let canonical = match key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
        None => None,
        Some(raw) => Some(settings::validate_fkey(raw)?),
    };
    {
        let mut s = settings_state.inner.lock().expect("settings lock");
        if let Some(c) = &canonical {
            if c == settings::MODE_HOTKEY_RESERVED {
                return Err(err_key("hotkeys.err_f5_noslot"));
            }
            let slots = [
                s.mode_hotkeys().search.as_str(),
                s.mode_hotkeys().market.as_str(),
                s.mode_hotkeys().routes.as_str(),
                s.mode_hotkeys().inventory.as_str(),
                s.mode_hotkeys().hangar.as_str(),
                s.mode_hotkeys().settings.as_str(),
            ];
            let clashes_other_connector = s
                .connector_hotkeys()
                .iter()
                .any(|(cid, k)| *cid != id && k == c);
            if slots.contains(&c.as_str()) || clashes_other_connector {
                return Err(err_keyp(
                    "hotkeys.err_dup",
                    json!({ "slot": "connector.hotkey_slot", "key": c }),
                ));
            }
            s.set_connector_hotkey(&id, c.clone());
        } else {
            s.remove_connector_hotkey(&id);
        }
        crate::app_settings::persist_settings(&s);
    }
    Ok(list_cmd_inner(&state, &settings_state))
}

#[derive(Debug, Serialize)]
pub struct FetchResult {
    pub status: u16,
    pub body: serde_json::Value,
    /// Gemappte Felder: table/cards/calendar → Zeilen-Array, detail → Objekt.
    pub mapped: serde_json::Value,
    pub cached: bool,
    pub fetched_at_ms: u64,
}

#[tauri::command]
pub async fn connector_fetch(
    state: tauri::State<'_, ConnectorState>,
    settings_state: tauri::State<'_, crate::app_settings::SettingsState>,
    id: String,
    view_id: String,
    force: bool,
) -> Result<FetchResult, String> {
    let (manifest, view) = {
        let entries = state.entries.lock().expect("connector entries lock");
        let Some(st) = entries.iter().find(|e| e.manifest.id == id) else {
            return Err(err_key("error.connector.unknown"))
        };
        let Some(v) = st.manifest.views.iter().find(|v| v.id == view_id) else {
            return Err(err_key("error.connector.view_unknown"));
        };
        (st.manifest.clone(), v.clone())
    };
    if view.view_type == connector::ViewType::Form {
        // Form-Views sind Write-Kontext (C3) — kein GET hier.
        return Err(err_key("error.connector.form_not_fetchable"));
    }

    // TTL-Cache: fetch nur bei Ablauf oder force (F5/Manual-Button).
    let poll_secs = view.poll_seconds.unwrap_or(DEFAULT_POLL_SECONDS).max(1);
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if !force {
        let cache = state.cache.lock().expect("cache lock");
        if let Some(c) = cache.get(&(id.clone(), view_id.clone())) {
            if now_ms.saturating_sub(c.fetched_at_ms) < poll_secs * 1000 {
                let mapped = map_view(&view, &c.body)?;
                return Ok(FetchResult {
                    status: c.status,
                    body: c.body.clone(),
                    mapped,
                    cached: true,
                    fetched_at_ms: c.fetched_at_ms,
                });
            }
        }
    }

    // Auth-Header (Header-Auth-Mechanik): Wert aus den Settings, Settings-Key
    // = Manifest-`value_source`. Fehlender Wert → fetch trotzdem (offene APIs
    // wie SCLM-Reads), die Antwort entscheidet.
    let mut req = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| err_keyp("error.connector.http", json!({ "detail": e.to_string() })))?
        .get(format!("{}{}", manifest.base_url.trim_end_matches('/'), view.endpoint));
    if let Some(auth) = &manifest.auth {
        if auth.auth_type == connector::AuthType::Header {
            if let (Some(header), Some(source)) = (&auth.header, &auth.value_source) {
                let s = settings_state.inner.lock().expect("settings lock");
                if let Some(v) = s.connector_auth_value(&id, source) {
                    req = req.header(header.as_str(), v.to_owned());
                }
            }
        }
    }
    let resp = req
        .send()
        .await
        .map_err(|e| err_keyp("error.connector.http", json!({ "detail": e.to_string() })))?;
    let status = resp.status().as_u16();
    let text = resp
        .text()
        .await
        .map_err(|e| err_keyp("error.connector.http", json!({ "detail": e.to_string() })))?;
    let body: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        err_keyp("error.connector.invalid_json", json!({ "detail": e.to_string() }))
    })?;
    let mapped = map_view(&view, &body)?;
    {
        let mut cache = state.cache.lock().expect("cache lock");
        cache.insert(
            (id, view_id),
            CachedFetch {
                fetched_at_ms: now_ms,
                status,
                body: body.clone(),
            },
        );
    }
    Ok(FetchResult {
        status,
        body,
        mapped,
        cached: false,
        fetched_at_ms: now_ms,
    })
}

/// Dot-Path-Mapping gemäß Manifest: `mapping`-Feld → Wert pro Zeile
/// (table/cards/calendar: Iterationspfade erzeugen Zeilen) bzw. Detail-Objekt.
/// Mapping-Fehler (falsche Struktur) sind ein Fehler — ein kaputtes Mapping
/// darf nicht als leere Tabelle durchschauen.
fn map_view(view: &connector::View, body: &serde_json::Value) -> Result<serde_json::Value, String> {
    if view.mapping.is_empty() {
        // Kein Mapping deklariert → Rohantwort durchreichen (Views rendern
        // selbst flexibel). Für SCLM/AAWA deklarieren Manifeste Mapping.
        return Ok(body.clone());
    }
    match view.view_type {
        connector::ViewType::Detail => {
            let mut out = serde_json::Map::new();
            for (field, path) in &view.mapping {
                let hits = connector::dot_path(body, path)
                    .map_err(|e| err_keyp("error.connector.mapping", json!({ "detail": e.0 })))?;
                if let Some(v) = hits.first() {
                    out.insert(field.clone(), (*v).clone());
                }
            }
            Ok(serde_json::Value::Object(out))
        }
        _ => {
            // Zeilen-Views: die längste Iterations-Trefferliste bestimmt die
            // Zeilenzahl; pro Feld per Index greifen.
            let mut cols: Vec<(String, Vec<serde_json::Value>)> = Vec::new();
            let mut rows_len = 0usize;
            for (field, path) in &view.mapping {
                let hits = connector::dot_path(body, path)
                    .map_err(|e| err_keyp("error.connector.mapping", json!({ "detail": e.0 })))?;
                rows_len = rows_len.max(hits.len());
                cols.push((field.clone(), hits.into_iter().cloned().collect()));
            }
            let mut rows = Vec::with_capacity(rows_len);
            for i in 0..rows_len {
                let mut row = serde_json::Map::new();
                for (field, vals) in &cols {
                    if let Some(v) = vals.get(i) {
                        row.insert(field.clone(), v.clone());
                    }
                }
                rows.push(serde_json::Value::Object(row));
            }
            Ok(serde_json::Value::Array(rows))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn view(t: connector::ViewType, mapping: &[(&str, &str)]) -> connector::View {
        connector::View {
            id: "v".into(),
            view_type: t,
            title: connector::manifest::LocalizedText { de: "T".into(), en: "T".into() },
            endpoint: "/x".into(),
            mapping: mapping
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            poll_seconds: None,
            form: None,
        }
    }

    #[test]
    fn table_mapping_builds_rows() {
        let v = view(connector::ViewType::Table, &[("name", "data[].ship_name")]);
        let body = json!({ "data": [ { "ship_name": "Sabre" }, { "ship_name": "Kraken" } ] });
        let out = map_view(&v, &body).unwrap();
        assert_eq!(out.as_array().unwrap().len(), 2);
        assert_eq!(out[0]["name"], json!("Sabre"));
    }

    #[test]
    fn detail_mapping_single_object() {
        let v = view(connector::ViewType::Detail, &[("name", "session.name")]);
        let body = json!({ "session": { "name": "Weihnachtsverlosung" } });
        let out = map_view(&v, &body).unwrap();
        assert_eq!(out["name"], json!("Weihnachtsverlosung"));
    }

    #[test]
    fn bad_mapping_is_error_not_empty() {
        let v = view(connector::ViewType::Table, &[("name", "data[].ship_name")]);
        let body = json!({ "data": 5 });
        assert!(map_view(&v, &body).is_err());
    }

    #[test]
    fn parse_and_validate_size_and_errors() {
        assert!(parse_and_validate("{\"x\":1}", false).is_err());
        assert!(parse_and_validate("not json", false).is_err());
    }
}
