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

pub use crate::connector_keystore::{
    KeystorePaths, SessionEntry, find_entry as keystore_find, list_entries as keystore_list,
    load_sessions as keystore_load, upsert_and_save as keystore_upsert,
    upsert_entry as keystore_upsert_entry,
};

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
    /// Pro deklariertem `settings_fields`-Key: ist ein Wert gesetzt? Der Wert
    /// selbst verlässt das Backend nie (Auth-Feld-Mechanik, Spec 5.1).
    pub settings_set: BTreeMap<String, bool>,
}

impl ConnectorEntry {
    fn from_parts(
        id: &str,
        manifest: &serde_json::Value,
        s: &settings::Settings,
    ) -> ConnectorEntry {
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
            settings_set: s.connector_setting_values_set(id),
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
        let items: Vec<String> = report.errors.iter().map(|e| e.to_string()).collect();
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
    let manifest =
        parse_and_validate(&manifest_json, ignore_warnings).map_err(|e| e.to_json_string())?;
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
        let _ = std::fs::remove_file(keystore_paths(&state).file(&id));
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
            return Err(err_key("error.connector.unknown"));
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

/// Settings-Feld-Werte eines Connectors (spec 5.1): nur deklarierte Keys aus
/// `settings_fields` bekommen gespeicherte Werte (Platzhalter-Auflösung im
/// Flow-Kontext wie params). Leere Map bei unbekanntem Connector/ohne Felder.
pub(crate) fn settings_field_values(
    state: &ConnectorState,
    settings_state: &crate::app_settings::SettingsState,
    id: &str,
) -> BTreeMap<String, String> {
    let declared: Vec<String> = {
        let entries = state.entries.lock().expect("connector entries lock");
        entries
            .iter()
            .find(|e| e.manifest.id == id)
            .map(|e| {
                e.manifest
                    .settings_fields
                    .iter()
                    .map(|f| f.key.clone())
                    .collect()
            })
            .unwrap_or_default()
    };
    let s = settings_state.inner.lock().expect("settings lock");
    declared
        .iter()
        .filter_map(|k| {
            s.connector_setting_value(id, k)
                .map(|v| (k.clone(), v.to_owned()))
        })
        .collect()
}

/// Manifest-gefiltert: gespeicherte Werte der deklarierten settings_fields
/// (tauri-frei, testbar — Spec 5.1 Placeholder-Basis für Flow/Formular).
pub(crate) fn declared_setting_values(
    manifest: &connector::ConnectorManifest,
    settings_state: &crate::app_settings::SettingsState,
    id: &str,
) -> BTreeMap<String, String> {
    let s = settings_state.inner.lock().expect("settings lock");
    manifest
        .settings_fields
        .iter()
        .filter_map(|f| {
            s.connector_setting_value(id, &f.key)
                .map(|v| (f.key.clone(), v.to_owned()))
        })
        .collect()
}

/// Deklariert das Manifest den Key als settings_fields-Eintrag?
fn setting_field_declared(state: &ConnectorState, id: &str, key: &str) -> bool {
    let entries = state.entries.lock().expect("connector entries lock");
    entries
        .iter()
        .find(|e| e.manifest.id == id)
        .is_some_and(|e| e.manifest.settings_fields.iter().any(|f| f.key == key))
}

/// Keystore-Pfade des Connectors (Geschwister-Ordner des Manifest-Stores).
fn keystore_paths(state: &ConnectorState) -> KeystorePaths {
    KeystorePaths::from_manifest_dir(&state.dir)
}

/// Settings-Feld-Wert eines Connectors setzen/löschen (Key MUSS vom Manifest
/// deklariert sein — kein wilder Settings-Schmuggel, gleiche Regel wie Auth).
#[tauri::command]
pub async fn connector_set_setting_field(
    state: tauri::State<'_, ConnectorState>,
    settings_state: tauri::State<'_, crate::app_settings::SettingsState>,
    id: String,
    key: String,
    value: Option<String>,
) -> Result<Vec<ConnectorEntry>, String> {
    {
        let entries = state.entries.lock().expect("connector entries lock");
        if !entries.iter().any(|e| e.manifest.id == id) {
            return Err(err_key("error.connector.unknown"));
        }
    }
    if !setting_field_declared(&state, &id, &key) {
        return Err(err_key("error.connector.setting_field_undeclared"));
    }
    {
        let mut s = settings_state.inner.lock().expect("settings lock");
        s.set_connector_setting_value(&id, &key, value.filter(|v| !v.trim().is_empty()));
        crate::app_settings::persist_settings(&s);
    }
    Ok(list_cmd_inner(&state, &settings_state))
}

/// Mindestziel aus Spec 5.1: `{key: value}` der gespeicherten Settings-Felder
/// (Karte B/C bauen daraus die `{key}`-Placeholder im Flow-Kontext).
#[tauri::command]
pub async fn connector_get_settings_fields(
    state: tauri::State<'_, ConnectorState>,
    settings_state: tauri::State<'_, crate::app_settings::SettingsState>,
    connector_id: String,
) -> Result<BTreeMap<String, String>, String> {
    {
        let entries = state.entries.lock().expect("connector entries lock");
        if !entries.iter().any(|e| e.manifest.id == connector_id) {
            return Err(err_key("error.connector.unknown"));
        }
    }
    Ok(settings_field_values(
        &state,
        &settings_state,
        &connector_id,
    ))
}

/// Session-Keystore (Spec 5.2): Eintrag upserten — PARTIELL (None-Felder
/// bleiben, wie bei Form-Teilupdates). sid ist Pflicht (Identität).
#[tauri::command]
pub async fn connector_keystore_upsert(
    state: tauri::State<'_, ConnectorState>,
    id: String,
    sid: String,
    join_code: Option<String>,
    name: Option<String>,
    participant_id: Option<String>,
    pcode: Option<String>,
    result_id: Option<String>,
) -> Result<SessionEntry, String> {
    {
        let entries = state.entries.lock().expect("connector entries lock");
        if !entries.iter().any(|e| e.manifest.id == id) {
            return Err(err_key("error.connector.unknown"));
        }
    }
    if sid.trim().is_empty() {
        return Err(err_key("error.connector.keystore_sid_missing"));
    }
    Ok(keystore_upsert(
        &keystore_paths(&state).file(&id),
        &sid,
        join_code,
        name,
        participant_id,
        pcode,
        result_id,
    ))
}

/// Session-Keystore: alle Einträge (neueste zuerst).
#[tauri::command]
pub async fn connector_keystore_list(
    state: tauri::State<'_, ConnectorState>,
    id: String,
) -> Result<Vec<SessionEntry>, String> {
    {
        let entries = state.entries.lock().expect("connector entries lock");
        if !entries.iter().any(|e| e.manifest.id == id) {
            return Err(err_key("error.connector.unknown"));
        }
    }
    Ok(keystore_list(&keystore_load(
        &keystore_paths(&state).file(&id),
    )))
}

/// Session-Keystore: find by sid — Basis des „Öffnen"-Buttons (Spec 5.2:
/// Eintrag vorhanden = join_code/Claim-Werte kennen).
#[tauri::command]
pub async fn connector_keystore_find(
    state: tauri::State<'_, ConnectorState>,
    id: String,
    sid: String,
) -> Result<Option<SessionEntry>, String> {
    {
        let entries = state.entries.lock().expect("connector entries lock");
        if !entries.iter().any(|e| e.manifest.id == id) {
            return Err(err_key("error.connector.unknown"));
        }
    }
    Ok(keystore_find(
        &keystore_load(&keystore_paths(&state).file(&id)),
        &sid,
    ))
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
    params: Option<BTreeMap<String, String>>,
) -> Result<FetchResult, String> {
    let mut params = params.unwrap_or_default();
    // Settings-Feld-Werte (Spec 5.1) als Basis-Platzhalter-Injektion:
    // deklarierte `{key}`-Platzhalter (z. B. {sclm_username}) resolven aus
    // den Settings; explizit übergebene params haben Vorrang.
    for (k, v) in settings_field_values(&state, &settings_state, &id) {
        params.entry(k).or_insert(v);
    }
    let (manifest, view) = {
        let entries = state.entries.lock().expect("connector entries lock");
        let Some(st) = entries.iter().find(|e| e.manifest.id == id) else {
            return Err(err_key("error.connector.unknown"));
        };
        let view = resolve_fetch_view(&st.manifest, &view_id, &params)?;
        (st.manifest.clone(), view)
    };
    if view.view_type == connector::ViewType::Form {
        // Form-Views sind Write-Kontext (C3) — kein GET hier.
        return Err(err_key("error.connector.form_not_fetchable"));
    }

    // TTL-Cache: fetch nur bei Ablauf oder force (F5/Manual-Button). Der
    // Cache-Schlüssel schließt die Parameter ein — zwei Sessions kollidieren
    // nicht im Cache.
    let poll_secs = view.poll_seconds.unwrap_or(DEFAULT_POLL_SECONDS).max(1);
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let cache_key = (
        id.clone(),
        format!("{view_id}?{}", canonical_params(&params)),
    );
    if !force {
        let cache = state.cache.lock().expect("cache lock");
        if let Some(c) = cache.get(&cache_key) {
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

    // HTTP-Core: Endpoint-Platzhalter + Param-Header + Settings-Auth-Header.
    let (status, body) = fetch_request(
        &manifest,
        &view.endpoint,
        &view.headers.clone().unwrap_or_default(),
        &params,
        auth_header_opt(&manifest, &settings_state, &id),
    )
    .await?;
    let mapped = map_view(&view, &body)?;
    {
        let mut cache = state.cache.lock().expect("cache lock");
        cache.insert(
            cache_key,
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

/// Header-Auth-Header aus den Settings (Wert verlässt das Backend nie bis
/// zum HTTP-Request) — None, wenn nicht deklariert/nicht gesetzt.
fn auth_header_opt(
    manifest: &connector::ConnectorManifest,
    settings_state: &crate::app_settings::SettingsState,
    id: &str,
) -> Option<(String, String)> {
    let auth = manifest.auth.as_ref()?;
    if auth.auth_type != connector::AuthType::Header {
        return None;
    }
    let header = auth.header.clone()?;
    let source = auth.value_source.clone()?;
    let s = settings_state.inner.lock().expect("settings lock");
    s.connector_auth_value(id, &source)
        .map(|v| (header, v.to_owned()))
}

/// Fetch-Ziel-Auflösung (tauri-frei, testbar): Views zuerst, dann
/// Flow-Screens (SF1), sonst Flow-Autocomplete-Handshake (SF1-Fix
/// t_031c54e9, D2): der FlowRenderer sucht mit synthetischer Screen-ID
/// `<flow>:<field>` und übergibt den Such-Endpoint der AutocompleteRule
/// als `params.__flow_ep` (inkl. `{q}`-Platzhalter). Generischer
/// Handshake — kein SCLM-Hardcoding: Endpoint wird gefüllt
/// (percent-encoded), die Rohantwort durchgereicht (Renderer liest
/// body[]).
pub(crate) fn resolve_fetch_view(
    manifest: &connector::ConnectorManifest,
    view_id: &str,
    params: &BTreeMap<String, String>,
) -> Result<connector::View, String> {
    if let Some(v) = manifest.views.iter().find(|v| v.id == view_id) {
        return Ok(v.clone());
    }
    if let Some(s) = find_flow_screen(manifest, view_id) {
        if s.screen_type == connector::flow::ScreenType::Form {
            // Form-Screens sind Write-Kontext (C3) — kein GET hier.
            return Err(err_key("error.connector.form_not_fetchable"));
        }
        return Ok(screen_as_view(s));
    }
    let Some(ep) = params.get("__flow_ep") else {
        return Err(err_key("error.connector.view_unknown"));
    };
    let mut endpoint = ep.clone();
    for (k, v) in params {
        if k == "__flow_ep" {
            continue;
        }
        let ph = format!("{{{k}}}");
        if endpoint.contains(&ph) {
            endpoint = endpoint.replace(&ph, &percent_encode(v));
        }
    }
    Ok(connector::View {
        id: view_id.to_owned(),
        view_type: connector::ViewType::Table,
        title: connector::manifest::LocalizedText {
            de: "Autocomplete".into(),
            en: "Autocomplete".into(),
        },
        endpoint,
        mapping: Default::default(),
        poll_seconds: None,
        form: None,
        params: None,
        headers: None,
    })
}

/// Flow-Screen → generische View-Form (SF1): Screens teilen die Fetch-Form
/// mit Views (endpoint/headers/params/mapping/poll_seconds) — der Rest des
/// Cores (Cache, Mapping, HTTP) bleibt typgleich. Form-Screens werden NICHT
/// überführt (Write-Kontext); Listen/Ergebnisse → Table, Rest → Detail.
fn screen_as_view(s: &connector::flow::Screen) -> connector::View {
    use connector::flow::ScreenType;
    let view_type = match s.screen_type {
        ScreenType::List | ScreenType::Results => connector::ViewType::Table,
        ScreenType::Detail | ScreenType::CodeEntry | ScreenType::Wait | ScreenType::Form => {
            connector::ViewType::Detail
        }
    };
    connector::View {
        id: s.id.clone(),
        view_type,
        title: s.title.clone(),
        endpoint: s.endpoint.clone().unwrap_or_default(),
        mapping: s.mapping.clone(),
        poll_seconds: s.poll_seconds,
        form: None,
        params: s.params.clone(),
        headers: s.headers.clone(),
    }
}

/// Generischer Form-Submit (Formular-View): validiert Pflichtfelder, füllt
/// `{param}`-Platzhalter in path + body_template, sendet mit Header-Auth.
/// Antwort: `{ status, body }`. Write nur USER-initiiert (Button) — hier
/// gibt es kein Polling, kein Hintergrundaufruf.
#[tauri::command]
pub async fn connector_form_submit(
    state: tauri::State<'_, ConnectorState>,
    settings_state: tauri::State<'_, crate::app_settings::SettingsState>,
    id: String,
    form_id: String,
    values: BTreeMap<String, serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let (manifest, form) = {
        let entries = state.entries.lock().expect("connector entries lock");
        let Some(st) = entries.iter().find(|e| e.manifest.id == id) else {
            return Err(err_key("error.connector.unknown"));
        };
        // Views zuerst, dann Flow-Screens mit form-Block (SF1).
        let f = match st.manifest.views.iter().find(|v| v.id == form_id) {
            Some(v) => match &v.form {
                Some(f) => f.clone(),
                None => return Err(err_key("error.connector.form_not_fetchable")),
            },
            None => {
                let Some(s) = find_flow_screen(&st.manifest, &form_id) else {
                    return Err(err_key("error.connector.view_unknown"));
                };
                let Some(f) = &s.form else {
                    return Err(err_key("error.connector.form_not_fetchable"));
                };
                f.clone()
            }
        };
        (st.manifest.clone(), f)
    };
    let (status, resp_body) =
        form_submit_core(&manifest, &form, values, &settings_state, &id).await?;
    // Writes invalidieren den Cache des Connectors — die Listen zeigen den
    // neuen Stand beim nächsten Refresh (Polling/Manual).
    state.cache.lock().expect("cache lock").clear();
    Ok(json!({ "status": status, "body": resp_body }))
}

/// Kern des Form-Submits (tauri-frei, auch für E2E-Tests nutzbar):
/// Default-Quelle, Pflichtfelder, Pfad-/Body-Fill, Feld-Header, HTTP.
pub(crate) async fn form_submit_core(
    manifest: &connector::ConnectorManifest,
    form: &connector::manifest::FormView,
    mut values: BTreeMap<String, serde_json::Value>,
    settings_state: &crate::app_settings::SettingsState,
    id: &str,
) -> Result<(u16, serde_json::Value), String> {
    // Settings-Feld-Werte (Spec 5.1) als Füll-Basis: `{key}`-Platzhalter in
    // Pfad/Body resolven aus den Settings (Vorrang: explizit gesendete Werte).
    for (k, v) in declared_setting_values(manifest, settings_state, id) {
        values
            .entry(k)
            .or_insert_with(|| serde_json::Value::String(v));
    }
    // `default_source`-Felder werden VOR der Pflichtprüfung aus der
    // Settings-Whitelist gefüllt (z. B. Beitreten mit dem Standard-
    // Benutzername) — leeres Feld ist also KEIN Fehler, wenn die Quelle
    // einen Wert hat.
    for field in &form.fields {
        if let Some(source) = field.default_source.as_deref().map(str::trim) {
            let empty = values
                .get(&field.key)
                .map(|v| {
                    v.as_str()
                        .map(|s| s.trim().is_empty())
                        .unwrap_or(v.is_null())
                })
                .unwrap_or(true);
            if empty {
                let s = settings_state.inner.lock().expect("settings lock");
                if let Some(v) = s.whitelisted_setting(source) {
                    values.insert(field.key.clone(), serde_json::Value::String(v));
                }
            }
        }
    }
    for field in &form.fields {
        if field.required {
            let missing = values
                .get(&field.key)
                .map(|v| {
                    v.as_str()
                        .map(|s| s.trim().is_empty())
                        .unwrap_or(v.is_null())
                })
                .unwrap_or(true);
            if missing {
                return Err(err_keyp(
                    "error.connector.field_required",
                    json!({ "key": field.key }),
                ));
            }
        }
    }
    // Leere optionale Felder verwerfen, BEVOR der Body gefüllt wird: der
    // Platzhalter bleibt unaufgelöst und `strip_unfilled` entfernt den
    // Schlüssel — „leer = unverändert" gilt so auch für Teil-Updates und
    // Server-Defaults (ein leeres `""` würde sonst als Wert ankommen).
    drop_empty_optional_values(&form.fields, &mut values);
    // Pfad: ROHE Werte einsetzen (kein JSON-Quoting — ein Session-Code im
    // Pfad darf nicht als `"abc"` landen).
    let path = fill_form_path(&form.path, &values)?;
    // Body: JSON-bewusst einsetzen — parsebare Werte (true/false/Zahlen/
    // Listen wie ["id"]) bleiben strukturiert, Text wird gequoted. Werte, die
    // nicht gefüllt sind, entfernen wir aus dem Objekt (leere optionale
    // Felder schicken keine `{platzhalter}`-Müll-Strings an die API).
    let mut body = form.body_template.clone();
    apply_fill(&mut body, &values);
    strip_unfilled(&mut body);
    // Header aus Feldern mit `header`-Deklaration (z. B. Session-Code als
    // X-SCLM-Code — pro Formular, nicht global).
    let mut field_headers: Vec<(String, String)> = Vec::new();
    for field in &form.fields {
        if let Some(name) = field
            .header
            .as_deref()
            .map(str::trim)
            .filter(|h| !h.is_empty())
        {
            if let Some(v) = values.get(&field.key).and_then(|v| v.as_str()) {
                field_headers.push((name.to_owned(), v.to_owned()));
            }
        }
    }
    submit_form_request(
        manifest,
        form.method,
        &path,
        &body,
        &field_headers,
        auth_header_opt(manifest, settings_state, id),
    )
    .await
}

/// Flow-Screen-Auflösung (SF3): Screen-IDs können sich zwischen Flows
/// wiederholen (`participants` im Teilnehmer- UND Ersteller-Flow) — der
/// Cache-Key ist die view_id, deshalb qualifiziert der FlowRenderer als
/// `<flow>:<screen>`. Unqualifizierte IDs bleiben kompatibel (erster
/// Treffer, SF1/SF2-Verhalten).
pub(crate) fn find_flow_screen<'m>(
    manifest: &'m connector::ConnectorManifest,
    screen_id: &str,
) -> Option<&'m connector::flow::Screen> {
    if let Some((flow_id, sid)) = screen_id.split_once(':') {
        return manifest
            .flows
            .iter()
            .flatten()
            .find(|f| f.id == flow_id)
            .and_then(|f| f.screens.iter().find(|s| s.id == sid));
    }
    manifest
        .flows
        .iter()
        .flatten()
        .flat_map(|f| &f.screens)
        .find(|s| s.id == screen_id)
}

/// Generischer Screen-Action-Submit (SF2-Flow): führt eine `actions`-Action
/// eines Flow-Screens aus (method+path+body+headers aus der Action-Deklaration,
/// Werte aus dem Flow-Zustand des Renderers). Antwort: `{ status, body }`.
/// Write nur USER-initiiert (Button) — kein Polling, kein Hintergrundaufruf.
#[tauri::command]
pub async fn connector_screen_action(
    state: tauri::State<'_, ConnectorState>,
    settings_state: tauri::State<'_, crate::app_settings::SettingsState>,
    id: String,
    screen_id: String,
    action_id: String,
    values: BTreeMap<String, serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let manifest = {
        let entries = state.entries.lock().expect("connector entries lock");
        let Some(st) = entries.iter().find(|e| e.manifest.id == id) else {
            return Err(err_key("error.connector.unknown"));
        };
        st.manifest.clone()
    };
    let action = {
        let screen = find_flow_screen(&manifest, &screen_id)
            .ok_or_else(|| err_key("error.connector.view_unknown"))?;
        screen
            .actions
            .iter()
            .flatten()
            .find(|a| a.id == action_id)
            .cloned()
            .ok_or_else(|| err_key("error.connector.view_unknown"))?
    };
    let method = action
        .method
        .ok_or_else(|| err_key("error.connector.form_not_fetchable"))?;
    let path_template = action
        .path
        .ok_or_else(|| err_key("error.connector.form_not_fetchable"))?;
    // Pfad: ROHE Werte einsetzen (wie Formular-Submit).
    let path = fill_form_path(&path_template, &values)
        .map_err(|k| err_keyp("error.connector.param_required", json!({ "key": k })))?;
    // Body: JSON-bewusst einsetzen (Listen bleiben strukturiert), unfüllte
    // Keys entfernen.
    let mut body = action.body_template.clone().unwrap_or(json!({}));
    apply_fill(&mut body, &values);
    strip_unfilled(&mut body);
    // Header-Templates der Action: Platzhalter aus den Werten (stringifiziert).
    let mut string_values: BTreeMap<String, String> = BTreeMap::new();
    for (k, v) in &values {
        if let Some(s) = v.as_str() {
            string_values.insert(k.clone(), s.to_owned());
        } else if !v.is_null() {
            string_values.insert(k.clone(), v.to_string());
        }
    }
    // Settings-Feld-Werte (Spec 5.1) als Basis für Header-/Pfad-Platzhalter
    // (Vorrang: aus dem Flow-Zustand übergebene Werte).
    for (k, v) in declared_setting_values(&manifest, &settings_state, &id) {
        string_values.entry(k).or_insert(v);
    }
    let mut headers: Vec<(String, String)> = Vec::new();
    for (name, template) in action.headers.iter().flatten() {
        let value = fill_params(template, &string_values)
            .map_err(|p| err_keyp("error.connector.param_required", json!({ "key": p })))?;
        headers.push((name.clone(), value));
    }
    let (status, resp_body) = submit_form_request(
        &manifest,
        method,
        &path,
        &body,
        &headers,
        auth_header_opt(&manifest, &settings_state, &id),
    )
    .await?;
    // Writes invalidieren den Cache des Connectors.
    state.cache.lock().expect("cache lock").clear();
    Ok(json!({ "status": status, "body": resp_body }))
}

/// `{param}`-Platzhalter in einem String auflösen. Fehlt der Wert → Err(key).
fn fill_params(template: &str, params: &BTreeMap<String, String>) -> Result<String, String> {
    let mut out = template.to_owned();
    let mut rest = template;
    loop {
        let Some(start) = rest.find('{') else { break };
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else { break };
        let key = &after[..end];
        let value = params.get(key).map(|s| s.as_str()).unwrap_or("");
        if value.is_empty() {
            return Err(key.to_owned());
        }
        out = out.replace(&format!("{{{key}}}"), value);
        rest = &after[end + 1..];
    }
    Ok(out)
}

/// Erster unaufgelöster `{platzhalter}` (für Pfad-Fills im Formular).
fn find_unfilled(s: &str) -> Option<String> {
    let start = s.find('{')?;
    let after = &s[start + 1..];
    let end = after.find('}')?;
    Some(after[..end].to_owned())
}

/// Prozent-Encoding für Endpoint-Fills im Flow-Autocomplete (D2): Wert
/// landet im Query-Teil der URL — Leerzeichen/& etc. dürfen roh nicht
/// durchreichen. Unreserved-Zeichen (RFC 3986) bleiben roh.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Stabile Cache-Schlüssel-Ordnung (BTreeMap ist bereits sortiert).
fn canonical_params(params: &BTreeMap<String, String>) -> String {
    params
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// Leere Werte für nicht-required Felder verwerfen, bevor der Body gefüllt
/// wird: der Platzhalter bleibt unaufgelöst und `strip_unfilled` entfernt
/// den Schlüssel — „leer = unverändert" (Teil-Updates, Server-Defaults).
fn drop_empty_optional_values(
    fields: &[connector::manifest::FormField],
    values: &mut BTreeMap<String, serde_json::Value>,
) {
    for field in fields.iter().filter(|f| !f.required) {
        let empty = values
            .get(&field.key)
            .map(|v| match v {
                serde_json::Value::String(s) => s.trim().is_empty(),
                serde_json::Value::Null => true,
                _ => false,
            })
            .unwrap_or(false);
        if empty {
            values.remove(&field.key);
        }
    }
}

/// JSON-bewusster Template-Fill: Ein String, der EXAKT aus einem Platzhalter
/// besteht, ERSETZT den Knoten durch den (ggf. geparsten) Wert — Listen/
/// Objekte/Booleans/Zahlen bleiben strukturiert. Text mit eingebetteten
/// Platzhaltern wird serialisiert interpoliert; reiner Text wird gequoted.
fn apply_fill(target: &mut serde_json::Value, values: &BTreeMap<String, serde_json::Value>) {
    match target {
        serde_json::Value::String(s) => {
            let whole = s.strip_prefix('{').and_then(|r| r.strip_suffix('}'));
            if let Some(key) = whole {
                if key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    if let Some(v) = values.get(key) {
                        let parsed = match v {
                            serde_json::Value::String(raw) => {
                                serde_json::from_str::<serde_json::Value>(raw)
                                    .unwrap_or(serde_json::Value::String(raw.to_owned()))
                            }
                            other => other.clone(),
                        };
                        *target = parsed;
                        return;
                    }
                }
            }
            let mut out = s.clone();
            for (k, v) in values {
                if let Some(raw) = v.as_str() {
                    let parsed = serde_json::from_str::<serde_json::Value>(raw)
                        .unwrap_or(serde_json::Value::String(raw.to_owned()));
                    out = out.replace(&format!("{{{k}}}"), &parsed.to_string());
                } else {
                    out = out.replace(&format!("{{{k}}}"), &v.to_string());
                }
            }
            *s = out;
        }
        serde_json::Value::Array(items) => {
            for item in items {
                apply_fill(item, values);
            }
        }
        serde_json::Value::Object(map) => {
            for (_, val) in map.iter_mut() {
                apply_fill(val, values);
            }
        }
        _ => {}
    }
}

/// Nicht gefüllte `{platzhalter}`-Werte aus einem gefüllten Body entfernen
/// (leere optionale Felder schicken keine Müll-Strings an die API). Erst
/// rekursiv abwärts räumen, DANN auf eigener Ebene reten — sonst fliegt ein
/// ganzes Unterobjekt raus, obwohl nur EIN Feld darin unaufgelöst war.
fn strip_unfilled(target: &mut serde_json::Value) {
    match target {
        serde_json::Value::Object(map) => {
            for (_, v) in map.iter_mut() {
                strip_unfilled(v);
            }
            map.retain(|_, v| !string_has_unfilled(v));
        }
        serde_json::Value::Array(items) => {
            for v in items.iter_mut() {
                strip_unfilled(v);
            }
            items.retain(|v| !string_has_unfilled(v));
        }
        _ => {}
    }
}

fn string_has_unfilled(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::String(s) => {
            s.contains('{') && s.contains('}') && find_unfilled(s).is_some()
        }
        serde_json::Value::Array(items) => items.iter().any(string_has_unfilled),
        serde_json::Value::Object(map) => map.values().any(string_has_unfilled),
        _ => false,
    }
}

/// Formular-Pfad: ROHE Werte einsetzen (kein JSON-Quoting — ein Session-Code
/// im Pfad darf nicht als `"abc"` landen). Unaufgelöster Platzhalter → Err.
fn fill_form_path(
    path: &str,
    values: &BTreeMap<String, serde_json::Value>,
) -> Result<String, String> {
    let mut out = path.to_owned();
    for (k, v) in values {
        out = out.replace(&format!("{{{k}}}"), v.as_str().unwrap_or(&v.to_string()));
    }
    if let Some(p) = find_unfilled(&out) {
        return Err(p);
    }
    Ok(out)
}

/// HTTP-Get-Core des Fetch (getestbar außerhalb Tauri): Endpoint-Platzhalter
/// + Param-Header + Settings-Auth-Header. Liefert (status, body-json).
pub(crate) async fn fetch_request(
    manifest: &connector::ConnectorManifest,
    endpoint: &str,
    header_templates: &BTreeMap<String, String>,
    params: &BTreeMap<String, String>,
    auth_header: Option<(String, String)>,
) -> Result<(u16, serde_json::Value), String> {
    let endpoint = fill_params(endpoint, params)
        .map_err(|p| err_keyp("error.connector.param_required", json!({ "key": p })))?;
    let mut req = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| err_keyp("error.connector.http", json!({ "detail": e.to_string() })))?
        .get(format!(
            "{}{}",
            manifest.base_url.trim_end_matches('/'),
            endpoint
        ));
    if let Some((header, value)) = auth_header {
        req = req.header(header.as_str(), value);
    }
    for (name, template) in header_templates {
        let value = fill_params(template, params)
            .map_err(|p| err_keyp("error.connector.param_required", json!({ "key": p })))?;
        req = req.header(name.as_str(), value);
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
        err_keyp(
            "error.connector.invalid_json",
            json!({ "detail": e.to_string() }),
        )
    })?;
    Ok((status, body))
}

/// HTTP-Core des Form-Submit (getestbar außerhalb Tauri): Pfad-Fill
/// (roh), JSON-Body-Fill, Feld-Header + Settings-Auth-Header.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn submit_form_request(
    manifest: &connector::ConnectorManifest,
    method: connector::manifest::HttpMethod,
    path: &str,
    body: &serde_json::Value,
    field_headers: &[(String, String)],
    auth_header: Option<(String, String)>,
) -> Result<(u16, serde_json::Value), String> {
    let mut req = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| err_keyp("error.connector.http", json!({ "detail": e.to_string() })))?
        .request(
            match method {
                connector::manifest::HttpMethod::Get => reqwest::Method::GET,
                connector::manifest::HttpMethod::Post => reqwest::Method::POST,
                connector::manifest::HttpMethod::Patch => reqwest::Method::PATCH,
                connector::manifest::HttpMethod::Put => reqwest::Method::PUT,
                connector::manifest::HttpMethod::Delete => reqwest::Method::DELETE,
            },
            format!("{}{}", manifest.base_url.trim_end_matches('/'), path),
        );
    if let Some((header, value)) = auth_header {
        req = req.header(header.as_str(), value);
    }
    for (name, value) in field_headers {
        req = req.header(name.as_str(), value);
    }
    if !matches!(
        method,
        connector::manifest::HttpMethod::Get | connector::manifest::HttpMethod::Delete
    ) {
        req = req
            .header("Content-Type", "application/json")
            .body(body.to_string());
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
    let resp_body: serde_json::Value =
        serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text));
    Ok((status, resp_body))
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
                match hits.len() {
                    0 => {}
                    // Mehrere Treffer (z. B. `participants[].name`) → Array
                    // ins Detail (SCLM: Teilnehmer-/Loot-Listen).
                    1 => {
                        out.insert(field.clone(), hits[0].clone());
                    }
                    _ => {
                        out.insert(
                            field.clone(),
                            serde_json::Value::Array(hits.into_iter().cloned().collect()),
                        );
                    }
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
            title: connector::manifest::LocalizedText {
                de: "T".into(),
                en: "T".into(),
            },
            endpoint: "/x".into(),
            mapping: mapping
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            poll_seconds: None,
            form: None,
            params: None,
            headers: None,
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

    // ---- resolve_fetch_view: Flow-Autocomplete-Handshake (SF1-Fix D2) ----

    fn flow_manifest(view_ids: &[&str], screen_ids: &[&str]) -> connector::ConnectorManifest {
        let views: Vec<connector::View> = view_ids
            .iter()
            .map(|id| {
                let mut v = view(connector::ViewType::Table, &[]);
                v.id = (*id).to_owned();
                v
            })
            .collect();
        let screens = screen_ids
            .iter()
            .map(|id| connector::flow::Screen {
                id: (*id).to_owned(),
                screen_type: connector::flow::ScreenType::List,
                title: connector::manifest::LocalizedText {
                    de: "S".into(),
                    en: "S".into(),
                },
                endpoint: Some("/api/session/{code}".into()),
                mapping: Default::default(),
                params: Some(vec![connector::manifest::Param {
                    key: "code".into(),
                    label: connector::manifest::LocalizedText {
                        de: "c".into(),
                        en: "c".into(),
                    },
                    required: None,
                }]),
                headers: None,
                poll_seconds: None,
                form: None,
                pins: None,
                when: None,
                on_submit: None,
                on_select: None,
                exit: None,
                autocomplete: None,
                multi: None,
                actions: None,
            })
            .collect();
        connector::ConnectorManifest {
            manifest_version: 1,
            id: "sclm".into(),
            name: connector::manifest::LocalizedText {
                de: "S".into(),
                en: "S".into(),
            },
            base_url: "https://sclm.site".into(),
            auth: None,
            placement: connector::manifest::Placement {
                kind: connector::manifest::PlacementKind::Own,
                label: Some(connector::manifest::LocalizedText {
                    de: "SCLM".into(),
                    en: "SCLM".into(),
                }),
                hotkey_slot: true,
                target: None,
            },
            views: views,
            sync: None,
            settings_fields: Vec::new(),
            flows: Some(vec![connector::flow::Flow {
                id: "participant".into(),
                title: connector::manifest::LocalizedText {
                    de: "T".into(),
                    en: "T".into(),
                },
                entry: screen_ids.first().copied().unwrap_or("").into(),
                screens,
            }]),
        }
    }

    #[test]
    fn flow_autocomplete_uses_flow_ep_endpoint() {
        // synthetische Screen-ID + __flow_ep → Endpoint wird gefüllt,
        // Rohantwort-Mapping (leer) durchgereicht — kein view_unknown.
        let m = flow_manifest(&["sessions"], &["code"]);
        let mut params = BTreeMap::new();
        params.insert("__flow_ep".into(), "/api/items/search?q={q}".into());
        params.insert("q".into(), "cutlass black".into());
        let v = resolve_fetch_view(&m, "participant:loot_id", &params).unwrap();
        assert_eq!(v.endpoint, "/api/items/search?q=cutlass%20black");
        assert!(v.mapping.is_empty());
    }

    #[test]
    fn flow_autocomplete_without_flow_ep_still_unknown() {
        let m = flow_manifest(&["sessions"], &["code"]);
        let params = BTreeMap::new();
        let err = resolve_fetch_view(&m, "ghost:id", &params).unwrap_err();
        assert!(err.contains("view_unknown"), "{err}");
    }

    #[test]
    fn flow_screen_resolves_via_screen_id() {
        let m = flow_manifest(&["sessions"], &["code"]);
        let v = resolve_fetch_view(&m, "code", &BTreeMap::new()).unwrap();
        assert_eq!(v.endpoint, "/api/session/{code}");
    }

    /// SF3: Flow-qualifizierte Screen-IDs — zwei Flows mit gleichem
    /// Screen-ID-Namen resolven auf den RICHTIGEN Screen (Cache-Key ist
    /// die view_id, Kollisionen wären sonst stillschweigend falsch).
    #[test]
    fn flow_qualified_screen_id_resolves_right_flow() {
        let mut m = flow_manifest(&["sessions"], &["code"]);
        let mut second = connector::flow::Flow {
            id: "creator".into(),
            title: m.flows.as_ref().unwrap()[0].title.clone(),
            entry: "code".into(),
            screens: vec![{
                let mut s = m.flows.as_ref().unwrap()[0].screens[0].clone();
                s.endpoint = Some("/api/other/{code}".into());
                s
            }],
        };
        m.flows.as_mut().unwrap().push(second);
        // qualified → creator-Screen
        let v = resolve_fetch_view(&m, "creator:code", &BTreeMap::new()).unwrap();
        assert_eq!(v.endpoint, "/api/other/{code}");
        // qualified → participant-Screen
        let v = resolve_fetch_view(&m, "participant:code", &BTreeMap::new()).unwrap();
        assert_eq!(v.endpoint, "/api/session/{code}");
        // unqualifiziert → erster Treffer (Fallback, SF1/SF2-Kompatibilität)
        let v = resolve_fetch_view(&m, "code", &BTreeMap::new()).unwrap();
        assert_eq!(v.endpoint, "/api/session/{code}");
        // unbekannter Flow → view_unknown
        let err = resolve_fetch_view(&m, "ghost:code", &BTreeMap::new()).unwrap_err();
        assert!(err.contains("view_unknown"), "{err}");
    }

    #[test]
    fn percent_encode_unreserved_only() {
        assert_eq!(percent_encode("cutlass black"), "cutlass%20black");
        assert_eq!(percent_encode("a&b=c/d"), "a%26b%3Dc%2Fd");
        assert_eq!(percent_encode("AB-9_~."), "AB-9_~.");
        assert_eq!(percent_encode(""), "");
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
    fn detail_multi_hit_becomes_array() {
        let v = view(
            connector::ViewType::Detail,
            &[("participants", "participants[].name")],
        );
        let body = json!({ "participants": [ { "name": "A" }, { "name": "B" } ] });
        let out = map_view(&v, &body).unwrap();
        assert_eq!(out["participants"], json!(["A", "B"]));
    }

    #[test]
    fn parse_and_validate_size_and_errors() {
        assert!(parse_and_validate("{\"x\":1}", false).is_err());
        assert!(parse_and_validate("not json", false).is_err());
    }

    #[test]
    fn fill_params_resolves_and_requires_values() {
        let mut p = BTreeMap::new();
        p.insert("code".to_owned(), "AB12CD".to_owned());
        assert_eq!(
            fill_params("/api/session/{code}", &p).unwrap(),
            "/api/session/AB12CD"
        );
        assert_eq!(fill_params("/plain", &p).unwrap(), "/plain");
        assert_eq!(
            fill_params("/api/session/{missing}", &p).unwrap_err(),
            "missing"
        );
    }

    #[test]
    fn body_fill_keeps_json_structure_and_strips_unfilled() {
        let mut values = BTreeMap::new();
        values.insert(
            "loot_ids".to_owned(),
            serde_json::Value::String("[\"a1\",\"b2\"]".to_owned()),
        );
        values.insert("name".to_owned(), serde_json::Value::String("Sabre".into()));
        values.insert("qty".to_owned(), serde_json::json!(3));
        let tpl = json!({
            "loot_ids": "{loot_ids}",
            "name": "{name}",
            "qty": "{qty}",
            "empty_opt": "{missing}",
            "nested": { "keep": "{name}", "drop": "{missing}" }
        });
        let mut out = tpl;
        apply_fill(&mut out, &values);
        strip_unfilled(&mut out);
        assert_eq!(out["loot_ids"], json!(["a1", "b2"]));
        assert_eq!(out["name"], json!("Sabre"));
        assert_eq!(out["qty"], json!(3));
        assert!(out.get("empty_opt").is_none());
        assert!(out["nested"].get("drop").is_none());
        assert_eq!(out["nested"]["keep"], json!("Sabre"));
    }

    #[test]
    fn empty_optional_value_is_dropped_not_sent() {
        // Realer UI-Pfad (Review t_d5ca9277 Runde 2): Svelte-Bindungen liefern
        // für leere Felder "" (bzw. null), NICHT einen fehlenden Key — ein
        // leeres optionales Feld darf trotzdem nicht als "" in den Body.
        let ff = |key: &str, required: bool| -> connector::manifest::FormField {
            serde_json::from_value(json!({
                "key": key,
                "label": { "en": key, "de": key },
                "type": "text",
                "required": required
            }))
            .unwrap()
        };
        let fields = vec![
            ff("name", true),
            ff("max_wishes", false),
            ff("waived", false),
        ];
        let mut values = BTreeMap::new();
        values.insert("name".to_owned(), serde_json::Value::String("X".into()));
        values.insert(
            "max_wishes".to_owned(),
            serde_json::Value::String(String::new()),
        );
        values.insert("waived".to_owned(), serde_json::Value::Null);
        drop_empty_optional_values(&fields, &mut values);
        assert!(!values.contains_key("max_wishes"));
        assert!(!values.contains_key("waived"));
        assert!(values.contains_key("name"));
        // Fill + Strip: Key ist danach weg (statt `""` an die API zu senden).
        let mut body = json!({
            "name": "{name}",
            "max_wishes": "{max_wishes}",
            "waived": "{waived}"
        });
        apply_fill(&mut body, &values);
        strip_unfilled(&mut body);
        assert_eq!(body, json!({ "name": "X" }));
    }

    #[test]
    fn form_path_fill_is_raw_not_json_quoted() {
        // Form-Pfad-Fill: roher Wert (Regression: `"abc"` im Pfad wäre tot);
        // unaufgelöster Platzhalter ist ein Fehler.
        let mut values = BTreeMap::new();
        values.insert(
            "sid".to_owned(),
            serde_json::Value::String("abc123".to_owned()),
        );
        assert_eq!(
            fill_form_path("/api/session/{sid}/raffle", &values).unwrap(),
            "/api/session/abc123/raffle"
        );
        assert_eq!(
            fill_form_path("/api/session/{rid}", &values).unwrap_err(),
            "rid"
        );
    }

    // ---- SCLM v2 Karte A (Spec 5.1): deklarierte Settings-Werte ---------------

    fn settings_state_with(id: &str, pairs: &[(&str, &str)]) -> crate::app_settings::SettingsState {
        let tmp = tempfile::tempdir().unwrap();
        // path auf tmp ziehen: persist_settings würde sonst die echte Nutzer-
        // config treffen — in Tests bleibt es bei In-Memory-Setzung.
        let mut s = settings::Settings::with_path(tmp.path().join("config.toml"));
        for (k, v) in pairs {
            s.set_connector_setting_value(id, k, Some((*v).to_owned()));
        }
        crate::app_settings::SettingsState {
            inner: std::sync::Mutex::new(s),
        }
    }

    /// Manifest-Ziegel mit settings_fields (nur Felder relevant).
    fn manifest_with_setting_fields(fields: &[&str]) -> connector::ConnectorManifest {
        let text = format!(
            r#"{{"manifest_version":1,"id":"sclm","name":{{"de":"S","en":"S"}},
                "base_url":"https://sclm.site",
                "placement":{{"kind":"own","label":{{"de":"S","en":"S"}}}},
                "settings_fields": [{}],
                "views": []}}"#,
            fields
                .iter()
                .map(|k| format!(r#"{{"key":"{k}","label":{{"de":"L","en":"L"}},"type":"text"}}"#))
                .collect::<Vec<_>>()
                .join(",")
        );
        connector::ConnectorManifest::parse(text.as_bytes()).expect("valid fixture manifest")
    }

    #[test]
    fn declared_setting_values_only_returns_declared_keys() {
        let m = manifest_with_setting_fields(&["sclm_username"]);
        let st = settings_state_with(
            "sclm",
            &[("sclm_username", "Semih"), ("wild_key", "geschmuggelt")],
        );
        let out = declared_setting_values(&m, &st, "sclm");
        assert_eq!(out.len(), 1, "nur deklarierte Keys");
        assert_eq!(out["sclm_username"], "Semih");
        assert!(!out.contains_key("wild_key"));
        // Anderer Connector → keine Werte.
        assert!(declared_setting_values(&m, &st, "anders").is_empty());
        // Deklariert, aber nie gesetzt → fehlt (leer startend).
        let m2 = manifest_with_setting_fields(&["sclm_username", "andere"]);
        let out2 = declared_setting_values(&m2, &st, "sclm");
        assert_eq!(out2.len(), 1);
        assert!(!out2.contains_key("andere"));
    }

    #[test]
    fn keystore_core_reachable_via_connector_reexports() {
        // Commands nutzen den keystore-Einstieg über connector.rs (Re-Export) —
        // Compile-/Rauchnachweis, dass die Bridges stimmen.
        let mut sessions = Vec::new();
        let e = keystore_upsert_entry(
            &mut sessions,
            "sid",
            Some("JC".into()),
            None,
            None,
            None,
            None,
            7,
        );
        assert_eq!(e.join_code, "JC");
        assert_eq!(keystore_find(&sessions, "sid").unwrap().added, 7);
        assert_eq!(keystore_list(&sessions).len(), 1);
    }
}
