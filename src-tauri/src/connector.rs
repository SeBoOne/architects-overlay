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
};

/// Connector-Manifeste sind klein; 1 MiB Obergrenze (gleiche Geist wie
/// Locale-Importe — verhindern, dass ein Riesen-Manifest Boot/Poll verstopft).
pub use connector::MAX_MANIFEST_BYTES;

/// Standard-TTL-Cache-Dauer (ms) für Views ohne `poll_seconds`? — NEIN:
/// `poll_seconds` steuert das Polling-Intervall der UI; der Cache-TTL ist
/// derselbe Wert (Refresh vor Ablauf = cachefrisch, Force = echte Abfrage).
const DEFAULT_POLL_SECONDS: u64 = 300;

/// Cache-TTL in ms aus `poll_seconds` (Härtung t_3cfc7842 Finding 1): die
/// Multiply war unchecked — ein Remote-gesteuertes Manifest mit absurdem
/// poll_seconds panikte im Debug-Profil (`u64 Mul overflow`) und lief im
/// Release-Profil still wrapped (TTL-Bedingung damit falsch). saturating_mul
/// macht den Deckel dicht: der Cache bleibt dann praktisch unbegrenzt gültig
/// (force erzwang immer den Frischlauf); der Validator (check_scan /
/// check_view / check_flows) lehnt poll_seconds > 10^7 s ohnehin ab.
fn poll_ttl_ms(poll_secs: u64) -> u64 {
    poll_secs.saturating_mul(1000)
}

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
        if let Some(auth) = manifest.get("auth").and_then(|a| a.get("value_source"))
            && let Some(key) = auth.as_str()
        {
            auth_set.insert(key.to_owned(), s.connector_auth_value(id, key).is_some());
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
/// `include_secrets`: true nur für Backend-Platzhalter-Injektion (Werte
/// verlassen den Rust-Kern nie nach außen); false für jede Ausgabe ans
/// Frontend — Felder mit `secret:true` werden dann gefiltert (Secret-Vertrag:
/// das Frontend sieht nie Klartwerte, nur `_set`-Booles).
pub(crate) fn settings_field_values(
    state: &ConnectorState,
    settings_state: &crate::app_settings::SettingsState,
    id: &str,
    include_secrets: bool,
) -> BTreeMap<String, String> {
    let declared: Vec<(String, bool)> = {
        let entries = state.entries.lock().expect("connector entries lock");
        entries
            .iter()
            .find(|e| e.manifest.id == id)
            .map(|e| {
                e.manifest
                    .settings_fields
                    .iter()
                    .map(|f| (f.key.clone(), f.secret))
                    .collect()
            })
            .unwrap_or_default()
    };
    let s = settings_state.inner.lock().expect("settings lock");
    declared
        .iter()
        .filter_map(|(k, secret)| {
            if !include_secrets && *secret {
                return None;
            }
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
/// Secret-Vertrag (Review R1): Felder mit `secret:true` werden aus der
/// Command-Ausgabe gefiltert — Klartwerte verlassen das Backend nie; die
/// Backend-seitige `{key}`-Platzhalter-Injektion (fetch/flow/form) resolviert
/// Secrets weiterhin intern über `include_secrets = true`.
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
        false,
    ))
}

/// Session-Keystore (Spec 5.2): Eintrag upserten — PARTIELL (None-Felder
/// bleiben, wie bei Form-Teilupdates). sid ist Pflicht (Identität).
/// Tauri-Command-Signatur = API-Vertrag zum Frontend (flache Params).
#[allow(clippy::too_many_arguments)]
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

// ---- SCLM v2 Karte B (Spec 5.3): „Meine …“-Sessionscan ---------------------

/// Ein Scan-Treffer (Landlisten-Eintrag). `has_saved_code` wird pro Aufruf
/// frisch aus dem Keystore berechnet (nicht cached): die Liste zeigt den
/// aktuellen Keystore-Stand, auch wenn die Sitzungsdaten noch cached sind.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScannedRaffle {
    pub sid: String,
    pub name: String,
    pub raffle_done: bool,
    pub has_saved_code: bool,
}

/// Antwort des Scan-Commands. `skipped` = Detail-Fetch-Fehler (Spec 5.3 #5:
/// Session überspringen + log::warn, Liste bleibt verfügbar).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScanOutcome {
    pub scan_id: String,
    /// false = Match-Key hat keinen gespeicherten Wert → UI zeigt
    /// Leerzustand-Hinweis (Items leer, kein HTTP-Lauf).
    pub username_set: bool,
    pub items: Vec<ScannedRaffle>,
    pub skipped: usize,
    pub cached: bool,
    pub fetched_at_ms: u64,
}

/// Flag-Truthiness wie der Flow-Condition-Core (SQLite-/JSON-Toleranz):
/// bool true, Zahl ≠ 0, Strings "1"/"true" gelten als erledigt.
pub(crate) fn truthy_flag(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        serde_json::Value::String(s) => matches!(s.as_str(), "1" | "true" | "True"),
        _ => false,
    }
}

/// Kandidaten-Auswahl (Spec 5.3): alle offenen (`done_field` falsch — auch
/// bei fehlendem Feld) + die ersten `done_cap` erledigten. Eingangsreihenfolge
/// (neueste zuerst) bleibt erhalten. Gibt Listen-Indizes zurück.
pub(crate) fn select_scan_candidates(
    items: &[serde_json::Value],
    done_field: &str,
    done_cap: usize,
) -> Vec<usize> {
    let mut out = Vec::new();
    let mut done_taken = 0usize;
    for (i, item) in items.iter().enumerate() {
        if truthy_flag(item.get(done_field).unwrap_or(&serde_json::Value::Null)) {
            if done_taken < done_cap {
                done_taken += 1;
                out.push(i);
            }
        } else {
            out.push(i);
        }
    }
    out
}

/// ci-EXAKTER Name-Vergleich (Entscheidung „Name-Match exakt ci“): trim +
/// Unicode-lowercase; kein substring, kein Präfix. Non-String-Namen ignorieren,
/// kaputter Dot-Path = kein Treffer (detail-Fehler wird ohnehin übersprungen).
pub(crate) fn scan_name_matches(
    detail: &serde_json::Value,
    match_path: &str,
    username: &str,
) -> bool {
    let user = username.trim().to_lowercase();
    if user.is_empty() {
        return false;
    }
    let hits = match connector::dot_path(detail, match_path) {
        Ok(h) => h,
        Err(_) => return false,
    };
    hits.iter()
        .filter_map(|v| v.as_str())
        .any(|n| n.trim().to_lowercase() == user)
}

/// String-Wert eines Listen-Felds (Zahlen werden String-belegt — sids sind
/// bei SCLM Strings, aber API-Formen variieren).
fn list_field_string(item: &serde_json::Value, field: &str) -> Option<String> {
    match item.get(field)? {
        serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn scan_cache_body(items: &[ScannedRaffle], skipped: usize) -> serde_json::Value {
    json!({
        "items": items.iter().map(|it| json!({"sid": it.sid, "name": it.name, "raffle_done": it.raffle_done})).collect::<Vec<_>>(),
        "skipped": skipped
    })
}

/// Landliste des own-Modes: Manifest-Scan ausführen (TTL-Cache analog zu
/// Views — Username-Wert Teil des Cache-Schlüssels, Username-Änderung =
/// Miss). username_set=false → Leerzustand ohne jeden HTTP-Lauf.
#[tauri::command]
pub async fn connector_scan_match(
    state: tauri::State<'_, ConnectorState>,
    settings_state: tauri::State<'_, crate::app_settings::SettingsState>,
    id: String,
    force: bool,
) -> Result<ScanOutcome, String> {
    let manifest = {
        let entries = state.entries.lock().expect("connector entries lock");
        let Some(st) = entries.iter().find(|e| e.manifest.id == id) else {
            return Err(err_key("error.connector.unknown"));
        };
        st.manifest.clone()
    };
    let scan = manifest
        .scan
        .clone()
        .ok_or_else(|| err_key("error.connector.scan_missing"))?;
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    scan_inner(
        &state,
        &settings_state,
        &manifest,
        &scan,
        &id,
        force,
        now_ms,
    )
    .await
}

/// Kern des Scans (tauri-frei, mit injectierter Uhr testbar).
pub(crate) async fn scan_inner(
    state: &ConnectorState,
    settings_state: &crate::app_settings::SettingsState,
    manifest: &connector::ConnectorManifest,
    scan: &connector::ScanSpec,
    id: &str,
    force: bool,
    now_ms: u64,
) -> Result<ScanOutcome, String> {
    use futures::StreamExt;

    // Match-Wert aus den Settings (Backend-Sicht: auch secret:true-Werte sind
    // vergleichbar — der Wert selbst verlässt das Backend nie, nur verglichen
    // wird er, ausgegeben wird er nie).
    let username = settings_field_values(state, settings_state, id, true)
        .get(&scan.match_setting_key)
        .cloned()
        .unwrap_or_default();
    if username.trim().is_empty() {
        return Ok(ScanOutcome {
            scan_id: scan.id.clone(),
            username_set: false,
            items: Vec::new(),
            skipped: 0,
            cached: false,
            fetched_at_ms: now_ms,
        });
    }

    // Poll-Cache (Akzeptanzkarte B): zweite Anfrage innerhalb des Intervalls
    // läuft ohne neuen HTTP-Lauf. Username im Schlüssel: geänderter Name
    // erzwingt Miss. has_saved_code wird NIE aus dem Cache gelesen (Keystore
    // kann sich zwischen Polls ändern) — immer frisch berechnet.
    let poll_secs = scan.poll_seconds.unwrap_or(DEFAULT_POLL_SECONDS).max(30);
    let cache_key = (
        id.to_owned(),
        format!("scan:{}?u={}", scan.id, username.trim()),
    );
    let keystore_sids: std::collections::BTreeSet<String> =
        keystore_load(&keystore_paths(state).file(id))
            .into_iter()
            .map(|e| e.sid)
            .collect();
    let attach_saved = |items: Vec<(String, String, bool)>| -> Vec<ScannedRaffle> {
        items
            .into_iter()
            .map(|(sid, name, raffle_done)| ScannedRaffle {
                has_saved_code: keystore_sids.contains(&sid),
                sid,
                name,
                raffle_done,
            })
            .collect()
    };
    if !force {
        let cache = state.cache.lock().expect("cache lock");
        if let Some(c) = cache.get(&cache_key)
            && now_ms.saturating_sub(c.fetched_at_ms) < poll_ttl_ms(poll_secs)
        {
            let cached_items: Vec<ScannedRaffle> = attach_saved(
                c.body
                    .get("items")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .map(|it| {
                                (
                                    it.get("sid")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or_default()
                                        .to_owned(),
                                    it.get("name")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or_default()
                                        .to_owned(),
                                    truthy_flag(
                                        it.get("raffle_done").unwrap_or(&serde_json::Value::Null),
                                    ),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            );
            return Ok(ScanOutcome {
                scan_id: scan.id.clone(),
                username_set: true,
                skipped: c.body.get("skipped").and_then(|v| v.as_u64()).unwrap_or(0) as usize,
                items: cached_items,
                cached: true,
                fetched_at_ms: c.fetched_at_ms,
            });
        }
    }

    // 1. Listen-Endpunkt (ohne Session-Auth; Manifest-Header-Auth greift wie
    //    bei Views, wenn der Connector sie deklariert).
    let (status, list_body) = fetch_request(
        manifest,
        &scan.list_endpoint,
        &Default::default(),
        &Default::default(),
        auth_header_opt(manifest, settings_state, id),
    )
    .await?;
    if status >= 400 {
        return Err(err_keyp(
            "error.connector.http",
            json!({ "detail": format!("list endpoint: HTTP {status}") }),
        ));
    }
    let list: &Vec<serde_json::Value> = list_body.as_array().ok_or_else(|| {
        err_keyp(
            "error.connector.mapping",
            json!({"detail": "scan list endpoint must return a JSON array"}),
        )
    })?;

    // 2. Kandidaten: alle offen + die letzten done_cap erledigte (Liste ist
    //    neueste-zuerst, cut auf die ersten done_cap Treffer).
    let raw_candidates = select_scan_candidates(list, &scan.done_field, scan.done_cap);
    // 2a. Härtung (t_3cfc7842 Findings 2+3, Reviewer-Sonden am Head 5530a54):
    //   - sid-Dedup: eine ferne Liste kann denselben `id_field`-Wert twice
    //     liefern (Reviewer-Fixture: sids ["D1","D1"]) -> doppelte Keys im
    //     Svelte {#each ... (it.sid)}. Ersten Auftritt behalten (Liste ist
    //     neueste-zuerst), Duplikat überspringen — spart zudem den zweiten
    //     Detail-Fetch.
    //   - code-Feld-Sanitisierung: der code-Wert kommt aus der FERNEN Liste
    //     und wird roh in den Detail-Endpfad gefüllt (Reviewer-Sonde:
    //     join_code="../../admin?x=1" -> Server sah /admin?x=1). Zulässig ist
    //     nur der Code-Zeichenvorrat [A-Za-z0-9_-]; alles andere: Skip + warn
    //     (percent-Encoding allein würde `/` und `..` unverändert durchlassen).
    let mut seen_ids: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut candidates: Vec<usize> = Vec::with_capacity(raw_candidates.len());
    // Von der Vorfilterung verworfene Kandidaten (dupes/unsafe Kaputter codes)
    // zählen in den skipped-Zähler hinein — die UI soll sehen, dass die ferne
    // Liste nicht vollständig verarbeitbar war.
    let mut pre_skipped = 0usize;
    for idx in raw_candidates {
        let item = &list[idx];
        let sid_ok = match list_field_string(item, &scan.id_field) {
            Some(sid) => seen_ids.insert(sid),
            None => true, // ohne sid keine {#each}-Key-Kollision; Detail-Pfad hängt nicht an sid
        };
        if !sid_ok {
            log::warn!(
                "connector scan '{}': skip candidate #{idx} (duplicate id_field '{}' value)",
                scan.id,
                scan.id_field
            );
            pre_skipped += 1;
            continue;
        }
        match list_field_string(item, &scan.code_field) {
            Some(code) if scan_code_is_safe(&code) => candidates.push(idx),
            Some(_) => {
                log::warn!(
                    "connector scan '{}': skip candidate #{idx} (code_field '{}' carries unsafe characters)",
                    scan.id,
                    scan.code_field
                );
                pre_skipped += 1;
            }
            None => {
                log::warn!(
                    "connector scan '{}': skip candidate #{idx} (missing code field '{}')",
                    scan.id,
                    scan.code_field
                );
                pre_skipped += 1;
            }
        }
    }

    // 3. Detail-Fetches parallel, begrenzt (Spec: concurrency 6, harte Klammer
    //    16). Fehler einer Detail-Session: überspringen + log::warn — die Liste
    //    bleibt verfügbar (Spec 5.3 #5).
    let conc = scan.concurrency.clamp(1, 16);
    type DetailOutcome = (usize, Result<(u16, serde_json::Value), String>);
    let detail_results: Vec<DetailOutcome> =
        futures::stream::iter(candidates.into_iter().map(|idx| {
            let manifest = manifest.clone();
            let scan = scan.clone();
            let item = list[idx].clone();
            let settings_auth = auth_header_opt(&manifest, settings_state, id);
            async move {
                // code_field ist hier bereits presence+charset-geprüft (2a).
                let code = list_field_string(&item, &scan.code_field).unwrap_or_default();
                let mut params: BTreeMap<String, String> = BTreeMap::new();
                params.insert("code".to_owned(), code);
                let r = fetch_request(
                    &manifest,
                    &scan.detail_endpoint,
                    &scan.headers.clone().unwrap_or_default(),
                    &params,
                    settings_auth,
                )
                .await;
                (idx, r)
            }
        }))
        .buffer_unordered(conc)
        .collect()
        .await;

    // Vorfilterung (2a) + Detail-Fehler zusammen in skipped (UI-Hinweis #5).
    let mut skipped = pre_skipped;
    let mut hit_idx: Vec<usize> = Vec::new();
    for (idx, res) in detail_results {
        match res {
            Ok((status, body)) if status < 400 => {
                if scan_name_matches(&body, &scan.match_path, &username) {
                    hit_idx.push(idx);
                }
            }
            Ok((status, _)) => {
                log::warn!(
                    "connector scan '{}': skip candidate #{idx} (HTTP {status})",
                    scan.id
                );
                skipped += 1;
            }
            Err(e) => {
                log::warn!("connector scan '{}': skip candidate #{idx} ({e})", scan.id);
                skipped += 1;
            }
        }
    }
    hit_idx.sort(); // Kandidatenreihenfolge (neueste zuerst) wiederherstellen

    // 4. Treffer-Liste bauen + Cache füllen.
    let mut plain: Vec<(String, String, bool)> = Vec::new();
    for &idx in &hit_idx {
        let item = &list[idx];
        let sid = match list_field_string(item, &scan.id_field) {
            Some(s) => s,
            None => {
                log::warn!(
                    "connector scan '{}': candidate #{idx} missing id_field '{}'",
                    scan.id,
                    scan.id_field
                );
                skipped += 1;
                continue;
            }
        };
        let name = list_field_string(item, &scan.name_field).unwrap_or_default();
        let done = truthy_flag(
            item.get(&scan.done_field)
                .unwrap_or(&serde_json::Value::Null),
        );
        plain.push((sid, name, done));
    }
    let items = attach_saved(plain.clone());
    {
        let mut cache = state.cache.lock().expect("cache lock");
        cache.insert(
            cache_key,
            CachedFetch {
                fetched_at_ms: now_ms,
                status: 200,
                body: scan_cache_body(&items, skipped),
            },
        );
    }
    Ok(ScanOutcome {
        scan_id: scan.id.clone(),
        username_set: true,
        items,
        skipped,
        cached: false,
        fetched_at_ms: now_ms,
    })
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
    fetch_inner(
        &state,
        &settings_state,
        &id,
        &view_id,
        force,
        params.unwrap_or_default(),
    )
    .await
}

/// Fetch-Kern (tauri-frei, testbar — gleiche Semantik wie das Command):
/// Settings-Injektion, TTL-Cache, HTTP, Mapping, Keystore-Write.
pub(crate) async fn fetch_inner(
    state: &ConnectorState,
    settings_state: &crate::app_settings::SettingsState,
    id: &str,
    view_id: &str,
    force: bool,
    mut params: BTreeMap<String, String>,
) -> Result<FetchResult, String> {
    // Settings-Feld-Werte (Spec 5.1) als Basis-Platzhalter-Injektion:
    // deklarierte `{key}`-Platzhalter (z. B. {sclm_username}) resolven aus
    // den Settings; explizit übergebene params haben Vorrang. Secrets
    // (secret:true) resolven HIER weiter mit — sie werden nur fürs request-
    // Ziel verwendet und nie ans Frontend ausgegeben (Secret-Vertrag).
    for (k, v) in settings_field_values(state, settings_state, id, true) {
        params.entry(k).or_insert(v);
    }
    let (manifest, view) = {
        let entries = state.entries.lock().expect("connector entries lock");
        let Some(st) = entries.iter().find(|e| e.manifest.id == id) else {
            return Err(err_key("error.connector.unknown"));
        };
        let view = resolve_fetch_view(&st.manifest, view_id, &params)?;
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
        id.to_owned(),
        format!("{view_id}?{}", canonical_params(&params)),
    );
    if !force {
        let cache = state.cache.lock().expect("cache lock");
        if let Some(c) = cache.get(&cache_key)
            && now_ms.saturating_sub(c.fetched_at_ms) < poll_ttl_ms(poll_secs)
        {
            let mapped = map_view(&view, &c.body)?;
            write_screen_keystore(state, &manifest, view_id, &params, &mapped);
            return Ok(FetchResult {
                status: c.status,
                body: c.body.clone(),
                mapped,
                cached: true,
                fetched_at_ms: c.fetched_at_ms,
            });
        }
    }

    // HTTP-Core: Endpoint-Platzhalter + Param-Header + Settings-Auth-Header.
    let (status, body) = fetch_request(
        &manifest,
        &view.endpoint,
        &view.headers.clone().unwrap_or_default(),
        &params,
        auth_header_opt(&manifest, settings_state, id),
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
    write_screen_keystore(state, &manifest, view_id, &params, &mapped);
    Ok(FetchResult {
        status,
        body,
        mapped,
        cached: false,
        fetched_at_ms: now_ms,
    })
}

/// SCLM v2 (Spec 5.2): Flow-Screens mit `keystore_fields` schreiben ihren
/// gemappten Wert in die Keystore-Spalte, sobald der Fetch einen eindeutigen
/// Treffer liefert (z. B. results-Screen → result_id). sid-Convention:
/// `params["sid"]` (derselbe Platzhalter, den die Write-Endpunkte nutzen).
/// Fehlende/leere Werte schreiben NICHTS (null-safe: `result: null` lässt
/// die Spalte leer).
fn write_screen_keystore(
    state: &ConnectorState,
    manifest: &connector::ConnectorManifest,
    view_id: &str,
    params: &BTreeMap<String, String>,
    mapped: &serde_json::Value,
) {
    let Some(screen) = find_flow_screen(manifest, view_id) else {
        return;
    };
    let cols = match &screen.keystore_fields {
        Some(c) if !c.is_empty() => c,
        _ => return,
    };
    let sid = params
        .get("sid")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("");
    if sid.is_empty() {
        log::warn!(
            "connector keystore_fields: screen '{view_id}' ohne sid-Param — Keystore-Write übersprungen"
        );
        return;
    }
    // Eindeutiger Treffer: Detail-Objekt oder Table mit GENAU einer Zeile.
    // Mehrzeilige Listen sind nie ein Eintrag für den Session-Keystore.
    // `result: null` → leere Zeilenliste → nichts geschrieben (null-safe).
    let row: Option<&serde_json::Map<String, serde_json::Value>> = match mapped {
        serde_json::Value::Object(o) => Some(o),
        serde_json::Value::Array(a) if a.len() == 1 => a[0].as_object(),
        _ => None,
    };
    let Some(row) = row else { return };
    let file = keystore_paths(state).file(&manifest.id);
    for (col, state_key) in cols {
        let value = match row.get(state_key.as_str()) {
            Some(serde_json::Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_owned()),
            Some(serde_json::Value::Number(n)) => Some(n.to_string()),
            _ => None,
        };
        let Some(value) = value else { continue };
        match col.as_str() {
            "join_code" => {
                keystore_upsert(&file, sid, Some(value), None, None, None, None);
            }
            "name" => {
                keystore_upsert(&file, sid, None, Some(value), None, None, None);
            }
            "participant_id" => {
                keystore_upsert(&file, sid, None, None, Some(value), None, None);
            }
            "pcode" => {
                keystore_upsert(&file, sid, None, None, None, Some(value), None);
            }
            "result_id" => {
                keystore_upsert(&file, sid, None, None, None, None, Some(value));
            }
            other => {
                log::warn!("connector keystore_fields: unbekannte Spalte '{other}' (übersprungen)")
            }
        }
    }
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
            && let Some(v) = values.get(&field.key).and_then(|v| v.as_str())
        {
            field_headers.push((name.to_owned(), v.to_owned()));
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
    let (manifest, action) = {
        let entries = state.entries.lock().expect("connector entries lock");
        let Some(st) = entries.iter().find(|e| e.manifest.id == id) else {
            return Err(err_key("error.connector.unknown"));
        };
        let manifest = st.manifest.clone();
        let screen = find_flow_screen(&manifest, &screen_id)
            .ok_or_else(|| err_key("error.connector.view_unknown"))?;
        let action = screen
            .actions
            .iter()
            .flatten()
            .find(|a| a.id == action_id)
            .cloned()
            .ok_or_else(|| err_key("error.connector.view_unknown"))?;
        (manifest, action)
    };
    let (status, resp_body) =
        screen_action_core(&manifest, &action, values, &settings_state, &id).await?;
    // Writes invalidieren den Cache des Connectors.
    state.cache.lock().expect("cache lock").clear();
    Ok(json!({ "status": status, "body": resp_body }))
}

/// Kern des Screen-Action-Submits (tauri-frei, testbar): method+path aus der
/// Deklaration, Body JSON-bewusst gefüllt, Header-Templates aufgelöst.
/// pcode-Vertrag (Spec Abschnitt 3): deklarierte Codes wandern als Body-Werte
/// in die API — Header-Templates der Action enthalten bewusst KEINE pcode-
/// Platzhalter (der Kern erzwingt das nicht, aber die Manifests tun es).
pub(crate) async fn screen_action_core(
    manifest: &connector::ConnectorManifest,
    action: &connector::flow::ScreenAction,
    values: BTreeMap<String, serde_json::Value>,
    settings_state: &crate::app_settings::SettingsState,
    id: &str,
) -> Result<(u16, serde_json::Value), String> {
    let method = action
        .method
        .ok_or_else(|| err_key("error.connector.form_not_fetchable"))?;
    let path_template = action
        .path
        .as_deref()
        .ok_or_else(|| err_key("error.connector.form_not_fetchable"))?;
    // Pfad: ROHE Werte einsetzen (wie Formular-Submit).
    let path = fill_form_path(path_template, &values)
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
    for (k, v) in declared_setting_values(manifest, settings_state, id) {
        string_values.entry(k).or_insert(v);
    }
    let mut headers: Vec<(String, String)> = Vec::new();
    for (name, template) in action.headers.iter().flatten() {
        let value = fill_params(template, &string_values)
            .map_err(|p| err_keyp("error.connector.param_required", json!({ "key": p })))?;
        headers.push((name.clone(), value));
    }
    submit_form_request(
        manifest,
        method,
        &path,
        &body,
        &headers,
        auth_header_opt(manifest, settings_state, id),
    )
    .await
}

// ───────────────────────────────────── Flow-Öffnung / Auto-Beitritt (SCLM v2, Spec 5.4)

/// Ergebnis einer Flow-Öffnung (`connector_flow_open`).
#[derive(Debug, Clone, Serialize)]
pub struct FlowOpenOutcome {
    pub flow_id: String,
    /// Startscreen für den Renderer: `done_screen` wenn das Event bereits
    /// gelaufen ist (auto-Ergebnis), sonst `open_screen`.
    pub screen: String,
    /// Flow-Zustand: open.mapping-Werte + Einstiegs-Code + participant_id +
    /// pcode (als Pins für nachfolgende Screens).
    pub state: BTreeMap<String, serde_json::Value>,
    /// true: Teilnehmer wurde neu per POST angelegt (Server-pcode erhalten).
    pub joined: bool,
    /// true: 409-Weg — Name schon vorhanden, id aus Fehlerbody übernommen.
    pub claimed: bool,
    /// true: pcode wurde beim Öffnen NEU erzeugt (Claim herrenloser Teilnehmer
    /// ohne Keystore-pcode); er bindet beim ersten wishes/raffle-Body.
    pub pcode_generated: bool,
    /// Statuscode der Session-Lastung (Diagnose, kein Geheimnis).
    pub status: u16,
}

/// Codes (join/pcode) dürfen nur alphanumerische Zeichen + `-`/`_` tragen —
/// Guardrail gegen Einschleusung in Endpunkt-Pfade (kein SCLM-Wissen nötig).
fn safe_code_shape(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 64
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn value_to_string(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_owned()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Client-seitig erzeugter pcode (Spec 6): ≥16 Zeichen, url-safe,
/// verwechslungsfrei geschrieben (kein l/1/I/O/0). 24 Zeichen aus CSPRNG.
fn generate_pcode() -> String {
    const ALPHA: &[u8] = b"abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut buf = [0u8; 24];
    if getrandom::fill(&mut buf).is_err() {
        // Notlage (CSPRNG nicht verfügbar — praktisch unmöglich): Zeit-/Prozess-
        // Entropie pro Zeichen verstreut. Kein Default-RNG, keine Vorhersagekette.
        use std::hash::{Hash, Hasher};
        let seed = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            std::time::SystemTime::now().hash(&mut h);
            std::process::id().hash(&mut h);
            h.finish()
        };
        for (i, b) in buf.iter_mut().enumerate() {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (seed, i).hash(&mut h);
            *b = (h.finish() & 0xff) as u8;
        }
    }
    buf.iter()
        .map(|b| ALPHA[(*b as usize) % ALPHA.len()] as char)
        .collect()
}

/// ci-EXAKTER Name-Vergleich gegen die Teilnehmerliste derselben Session
/// (gleiche Semantik wie die Landlisten-Zuordnung, Spec 5.4 Punkt 2).
pub(crate) fn flow_participant_match(
    detail: &serde_json::Value,
    match_path: &str,
    id_path: &str,
    username: &str,
) -> Option<String> {
    let user = username.trim().to_lowercase();
    if user.is_empty() {
        return None;
    }
    let names = connector::dot_path(detail, match_path).ok()?;
    let ids = connector::dot_path(detail, id_path).ok()?;
    for (n, id) in names.iter().zip(ids.iter()) {
        if let Some(s) = n.as_str()
            && s.trim().to_lowercase() == user
            && let Some(i) = value_to_string(id)
        {
            return Some(i);
        }
    }
    None
}

/// open.mapping → Flow-Zustand: ein Treffer → Skalar, mehrere → echtes Array,
/// kein Treffer → null (null-safe, Spec Abschnitt 3).
fn map_open_state(
    body: &serde_json::Value,
    mapping: &BTreeMap<String, String>,
) -> BTreeMap<String, serde_json::Value> {
    let mut out = BTreeMap::new();
    for (k, path) in mapping {
        match connector::dot_path(body, path) {
            Ok(hits) if hits.len() == 1 => {
                out.insert(k.clone(), hits[0].clone());
            }
            Ok(hits) if !hits.is_empty() => {
                out.insert(
                    k.clone(),
                    serde_json::Value::Array(hits.into_iter().cloned().collect()),
                );
            }
            _ => {
                out.insert(k.clone(), serde_json::Value::Null);
            }
        }
    }
    out
}

/// Auto-Beitritt-Kern (tauri-frei, testbar). Ablauf nach Spec 5.4:
/// 1. Username-Check VOR jedem Request (fehlender Name → Abbruch, keine
///    Requests, kein Rate-Schaden).
/// 2. Session-Lastung via session_endpoint + Headers ({param}/{sid}/Settings).
/// 3. mapping → Flow-Zustand (sid Pflicht).
/// 4. Keystore-Upsert join_code+sid+Sessionname (Karte-A-Mechanik).
/// 5. ci-Vergleich participants → id übernehmen, sonst POST (join_body-Default
///    {"name": "{match_setting_key}"}); 409 → id aus Fehlerbody (Claim).
/// 6. pcode: Keystore-Wert wiederverwenden; fehlt er → generieren (≥16 url-safe).
///    pcode wandert NIE in Header — er liegt im Keystore/state und geht erst
///    in wishes/raffle-BODYS (Manifest-Vertrag).
/// 7. done_path wahr → done_screen (auto-Ergebnis), sonst open_screen.
pub(crate) async fn flow_open_core(
    manifest: &connector::ConnectorManifest,
    flow: &connector::flow::Flow,
    keystore_file: &std::path::Path,
    code: &str,
    settings_state: &crate::app_settings::SettingsState,
    id: &str,
) -> Result<FlowOpenOutcome, String> {
    let o = flow
        .open
        .as_ref()
        .ok_or_else(|| err_key("error.connector.flow_no_open"))?;
    let settings_values = declared_setting_values(manifest, settings_state, id);
    let username = settings_values
        .get(&o.match_setting_key)
        .cloned()
        .unwrap_or_default();
    if username.trim().is_empty() {
        // Spec 6 Punkt 4: klar abbrechen, KEIN Request, keine leeren POSTs.
        return Err(err_key("error.connector.flow_username_missing"));
    }
    let code = code.trim();
    if !safe_code_shape(code) {
        return Err(err_keyp(
            "error.connector.param_invalid",
            json!({ "key": o.param }),
        ));
    }

    // 1) Session lasten
    let mut params = settings_values.clone();
    params.insert(o.param.clone(), code.to_owned());
    let (status, body) = fetch_request(
        manifest,
        &o.session_endpoint,
        &o.headers.clone().unwrap_or_default(),
        &params,
        auth_header_opt(manifest, settings_state, id),
    )
    .await?;
    if status == 404 {
        return Err(err_key("error.connector.flow_session_not_found"));
    }
    if status >= 400 {
        return Err(err_keyp(
            "error.connector.http",
            json!({ "detail": format!("session: HTTP {status}") }),
        ));
    }

    // 2) mapping → Flow-Zustand
    let mut state = map_open_state(&body, &o.mapping);
    let sid = state.get("sid").and_then(value_to_string).ok_or_else(|| {
        err_keyp(
            "error.connector.mapping",
            json!({ "detail": "open mapping ohne sid" }),
        )
    })?;
    let session_name = body
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_owned();
    state.insert(o.param.clone(), serde_json::Value::String(code.to_owned()));

    // 3) Keystore: join_code+sid+Sessionname merken (Spalte sid = Schlüssel).
    let entry = keystore_upsert(
        keystore_file,
        &sid,
        Some(code.to_owned()),
        Some(session_name),
        None,
        None,
        None,
    );

    // 4) Auto-Beitritt
    let mut vals = params.clone();
    vals.insert("sid".to_owned(), sid.clone());
    let mut joined = false;
    let mut claimed = false;
    let mut resp_pcode: Option<String> = None;
    let pid =
        if let Some(found) = flow_participant_match(&body, &o.match_path, &o.id_path, &username) {
            found
        } else {
            // Selbst eintragen (POST). Body-Default: {"name": "{match_setting_key}"}.
            let mut jbody = o
                .join_body
                .clone()
                .unwrap_or_else(|| json!({ "name": format!("{{{}}}", o.match_setting_key) }));
            let jvalues: BTreeMap<String, serde_json::Value> = vals
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect();
            apply_fill(&mut jbody, &jvalues);
            strip_unfilled(&mut jbody);
            let path = fill_form_path(&o.join_endpoint, &jvalues)
                .map_err(|k| err_keyp("error.connector.param_required", json!({ "key": k })))?;
            let mut headers: Vec<(String, String)> = Vec::new();
            for (name, template) in o.join_headers.iter().flatten() {
                let value = fill_params(template, &vals)
                    .map_err(|p| err_keyp("error.connector.param_required", json!({ "key": p })))?;
                headers.push((name.clone(), value));
            }
            let (jstatus, jresp) = submit_form_request(
                manifest,
                connector::manifest::HttpMethod::Post,
                &path,
                &jbody,
                &headers,
                auth_header_opt(manifest, settings_state, id),
            )
            .await?;
            if jstatus == 409 {
                // Duplikat: id aus dem Fehlerbody (Spec Abschnitt 3) → Claim-Pfad.
                claimed = true;
                jresp
                    .get("id")
                    .and_then(value_to_string)
                    .ok_or_else(|| err_keyp("error.connector.flow_claim_id_missing", json!({})))?
            } else if jstatus < 400 {
                joined = true;
                resp_pcode = jresp.get("pcode").and_then(value_to_string);
                jresp.get("id").and_then(value_to_string).ok_or_else(|| {
                    err_keyp(
                        "error.connector.mapping",
                        json!({ "detail": "join-antwort ohne id" }),
                    )
                })?
            } else {
                return Err(err_keyp(
                    "error.connector.http",
                    json!({ "detail": format!("participants: HTTP {jstatus}") }),
                ));
            }
        };

    // 5) pcode-Regel (Spec 3/6): Keystore-Wert hat Vorrang (dauerhaft),
    //    Server-pcode bei Neuerstellung danach, sonst frisch erzeugen.
    let mut pcode_generated = false;
    let pcode = if joined {
        match resp_pcode.or_else(|| entry.pcode.clone()) {
            Some(p) if !p.trim().is_empty() => p,
            _ => {
                pcode_generated = true;
                generate_pcode()
            }
        }
    } else if entry
        .pcode
        .as_deref()
        .map(|p| !p.trim().is_empty())
        .unwrap_or(false)
    {
        entry.pcode.clone().unwrap_or_default()
    } else {
        pcode_generated = true;
        generate_pcode()
    };

    // 6) Keystore: participant_id + pcode (partielles Upsert).
    let _ = keystore_upsert(
        keystore_file,
        &sid,
        None,
        None,
        Some(pid.clone()),
        Some(pcode.clone()),
        None,
    );

    state.insert("participant_id".to_owned(), serde_json::Value::String(pid));
    state.insert("pcode".to_owned(), serde_json::Value::String(pcode));

    // 7) Auto-Weiche: Event schon gelaufen → direkt Ergebnis-Screen.
    let done = truthy_flag(state.get(&o.done_path).unwrap_or(&serde_json::Value::Null));
    let screen = if done {
        o.done_screen.clone()
    } else {
        o.open_screen.clone()
    };

    Ok(FlowOpenOutcome {
        flow_id: flow.id.clone(),
        screen,
        state,
        joined,
        claimed,
        pcode_generated,
        status,
    })
}

/// Flow per Session-Code öffnen + Auto-Beitritt (SCLM v2, Spec 5.4/Requirement 6).
/// Frontend ruft das nach der Suchzeilen-Eingabe (einziger Einstieg) oder über
/// den Öffnen-Button der Landliste (Saved-join_code aus dem Keystore).
#[tauri::command]
pub async fn connector_flow_open(
    state: tauri::State<'_, ConnectorState>,
    settings_state: tauri::State<'_, crate::app_settings::SettingsState>,
    id: String,
    flow_id: String,
    code: String,
) -> Result<FlowOpenOutcome, String> {
    let (manifest, flow) = {
        let entries = state.entries.lock().expect("connector entries lock");
        let Some(st) = entries.iter().find(|e| e.manifest.id == id) else {
            return Err(err_key("error.connector.unknown"));
        };
        let Some(flow) = st
            .manifest
            .flows
            .iter()
            .flatten()
            .find(|f| f.id == flow_id && f.open.is_some())
            .cloned()
        else {
            return Err(err_key("error.connector.flow_no_open"));
        };
        (st.manifest.clone(), flow)
    };
    flow_open_core(
        &manifest,
        &flow,
        &keystore_paths(&state).file(&id),
        &code,
        &settings_state,
        &id,
    )
    .await
}

/// `{param}`-Platzhalter in einem String auflösen. Fehlt der Wert → Err(key).
fn fill_params(template: &str, params: &BTreeMap<String, String>) -> Result<String, String> {
    let mut out = template.to_owned();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
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

/// Code-Zeichenvorrat für fern-gelieferte `code_field`-Werte im Scan
/// (Härtung t_3cfc7842 Finding 2): nur [A-Za-z0-9_-], nicht leer. Der Wert
/// landet ungeteilt im Detail-Endpfad (`/api/session/{code}`) — `/`, `..`,
/// `?`, `#` oder Whitespace würden einen fremden Endpunkt ansteuern
/// (Reviewer-Sonde: Server sah /admin?x=1). Header-Inject (CRLF) ist damit
/// gleichfalls ausgeschlossen.
fn scan_code_is_safe(code: &str) -> bool {
    !code.is_empty()
        && code
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
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
            if let Some(key) = whole
                && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && let Some(v) = values.get(key)
            {
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
    // upsert_entry wird nur in Tests gebraucht — bewusst nicht als Re-Export
    // in connector.rs (sonst unused-import-Warnung im Nicht-Test-Build).
    use crate::connector_keystore::upsert_entry as keystore_upsert_entry;
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
                keystore_fields: None,
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
            views,
            sync: None,
            settings_fields: Vec::new(),
            scan: None,
            flows: Some(vec![connector::flow::Flow {
                id: "participant".into(),
                title: connector::manifest::LocalizedText {
                    de: "T".into(),
                    en: "T".into(),
                },
                entry: screen_ids.first().copied().unwrap_or("").into(),
                screens,
                open: None,
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
        let second = connector::flow::Flow {
            id: "creator".into(),
            title: m.flows.as_ref().unwrap()[0].title.clone(),
            entry: "code".into(),
            screens: vec![{
                let mut s = m.flows.as_ref().unwrap()[0].screens[0].clone();
                s.endpoint = Some("/api/other/{code}".into());
                s
            }],
            open: None,
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
        manifest_with_secret_setting_fields(&fields.iter().map(|k| (*k, false)).collect::<Vec<_>>())
    }

    /// Manifest-Ziegel mit settings_fields inkl. secret-Flag je Key.
    fn manifest_with_secret_setting_fields(
        fields: &[(&str, bool)],
    ) -> connector::ConnectorManifest {
        let text = format!(
            r#"{{"manifest_version":1,"id":"sclm","name":{{"de":"S","en":"S"}},
                "base_url":"https://sclm.site",
                "placement":{{"kind":"own","label":{{"de":"S","en":"S"}}}},
                "settings_fields": [{}],
                "views": []}}"#,
            fields
                .iter()
                .map(|(k, secret)| {
                    format!(
                        r#"{{"key":"{k}","label":{{"de":"L","en":"L"}},"type":"text","secret":{secret}}}"#
                    )
                })
                .collect::<Vec<_>>()
                .join(",")
        );
        connector::ConnectorManifest::parse(text.as_bytes()).expect("valid fixture manifest")
    }

    /// ConnectorState-Ziegel nur mit Manifest-Einträgen (settings_field_values
    /// liest ausschließlich entries + settings, kein IO).
    fn test_connector_state(manifests: Vec<connector::ConnectorManifest>) -> ConnectorState {
        ConnectorState {
            dir: std::env::temp_dir(),
            entries: std::sync::Mutex::new(
                manifests
                    .into_iter()
                    .map(|m| StoredConnector {
                        raw: String::new(),
                        manifest: m,
                        value: serde_json::Value::Null,
                    })
                    .collect(),
            ),
            cache: std::sync::Mutex::new(HashMap::new()),
        }
    }

    #[test]
    fn settings_field_values_filters_secrets_for_frontend_and_keeps_for_backend() {
        // Review R1-Sonde: secret:true-Wert darf nicht in die Command-Ausgabe
        // (connector_get_settings_fields nutzt include_secrets=false),
        // Backend-Platzhalter-Injektion (fetch/flow, include_secrets=true)
        // resolviert Secrets weiterhin intern.
        let m = manifest_with_secret_setting_fields(&[("sclm_username", false), ("tok", true)]);
        let st = settings_state_with("sclm", &[("sclm_username", "Semih"), ("tok", "s3cret")]);
        let cs = test_connector_state(vec![m]);

        // Frontend-Sicht (Command-Mindestziel {key: value} für Nicht-Secrets):
        let out = settings_field_values(&cs, &st, "sclm", false);
        assert_eq!(out.len(), 1, "nur das Nicht-Secret-Feld");
        assert_eq!(out["sclm_username"], "Semih");
        assert!(!out.contains_key("tok"), "secret:true-Wert gefiltert");
        assert!(
            !out.values().any(|v| v == "s3cret"),
            "Klartext des Secrets nirgends in der Ausgabe"
        );

        // Backend-Sicht (Platzhalter-Injektion bleibt funktional):
        let in_backend = settings_field_values(&cs, &st, "sclm", true);
        assert_eq!(in_backend.len(), 2);
        assert_eq!(in_backend["tok"], "s3cret");
        assert_eq!(in_backend["sclm_username"], "Semih");
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

    // ---- SCLM v2 Karte B (Spec 5.3): „Meine …“-Scan --------------------------
    // Fake-Sessionfixtures statt HTTP: Cache-Treffer- und Leerzustand-Pfad
    // erreichen scan_inner ohne jeden Netzwerkzugriff (Return VOR fetch_request).

    /// Manifest-Ziegel mit deklariertem scan (identische Feldnamen wie
    /// examples/sclm.json — aber Test-fix, keine Live-API).
    fn scan_manifest_fixture() -> (connector::ConnectorManifest, connector::ScanSpec) {
        let text = r#"{
            "manifest_version": 1, "id": "sclm", "name": {"de": "S", "en": "S"},
            "base_url": "https://sclm.site",
            "placement": {"kind": "own", "label": {"de": "S", "en": "S"}},
            "settings_fields": [{"key": "sclm_username", "label": {"de": "L", "en": "L"}, "type": "text"}],
            "scan": {
                "id": "my_raffles",
                "title": {"de": "Meine Verlosungen", "en": "My Raffles"},
                "list_endpoint": "/api/sessions",
                "done_field": "raffle_done",
                "code_field": "join_code",
                "id_field": "id",
                "name_field": "name",
                "detail_endpoint": "/api/session/{code}",
                "match_path": "participants[].name",
                "match_setting_key": "sclm_username",
                "done_cap": 2,
                "concurrency": 6,
                "poll_seconds": 300
            },
            "views": []
        }"#;
        let m =
            connector::ConnectorManifest::parse(text.as_bytes()).expect("scan fixture manifest");
        let scan = m.scan.clone().expect("scan block");
        (m, scan)
    }

    #[test]
    fn scan_truthy_flag_tolerates_json_forms() {
        for raw in ["true", "1", "\"1\"", "\"true\"", "\"True\"", "2"] {
            let v: serde_json::Value = serde_json::from_str(raw).unwrap();
            assert!(truthy_flag(&v), "{raw} muss als erledigt gelten");
        }
        for raw in ["false", "0", "\"0\"", "\"false\"", "null", "\"\"", "{}"] {
            let v: serde_json::Value = serde_json::from_str(raw).unwrap();
            assert!(!truthy_flag(&v), "{raw} muss als offen gelten");
        }
    }

    #[test]
    fn scan_candidates_keep_all_open_and_cut_done_at_cap() {
        // Eingangsreihenfolge neueste zuerst, done_cap = 2:
        // idx0 offen, idx1 erledigt, idx2 offen (fehlendes Feld = offen),
        // idx3 erledigt (2. genommener), idx4+5 erledigt → abgeschnitten.
        let list: Vec<serde_json::Value> = vec![
            json!({"id": "A", "raffle_done": false}),
            json!({"id": "B", "raffle_done": true}),
            json!({"id": "C"}),
            json!({"id": "D", "raffle_done": "1"}),
            json!({"id": "E", "raffle_done": true}),
            json!({"id": "F", "raffle_done": 1}),
        ];
        let got = select_scan_candidates(&list, "raffle_done", 2);
        assert_eq!(got, vec![0, 1, 2, 3]);
        // done_cap=0 ist invalid (Validator), aber defensively: nur Offene.
        assert_eq!(select_scan_candidates(&list, "raffle_done", 0), vec![0, 2]);
    }

    #[test]
    fn scan_name_match_is_ci_exact_not_substring() {
        let detail = json!({"participants": [
            {"name": "Semih"},
            {"name": " OTHER "},
            {"name": 42},
            {"nick": "Semih"}
        ]});
        assert!(scan_name_matches(&detail, "participants[].name", "semih"));
        assert!(scan_name_matches(
            &detail,
            "participants[].name",
            "  SEMIH  "
        ));
        assert!(scan_name_matches(&detail, "participants[].name", "other"));
        assert!(!scan_name_matches(&detail, "participants[].name", "semi"));
        assert!(!scan_name_matches(&detail, "participants[].name", "semih2"));
        assert!(!scan_name_matches(&detail, "participants[].name", ""));
        // Kaputter Dot-Path = kein Treffer (Detail-Fehler wird ohnehin übersprungen).
        assert!(!scan_name_matches(&detail, "participants[].nope", "semih"));
    }

    #[test]
    fn scan_empty_username_returns_without_http() {
        let (m, scan) = scan_manifest_fixture();
        let state = test_connector_state(vec![m.clone()]);
        let settings = settings_state_with("sclm", &[]);
        let out = futures::executor::block_on(scan_inner(
            &state, &settings, &m, &scan, "sclm", false, 1_000,
        ))
        .expect("leerer Username ist kein Fehler");
        assert!(!out.username_set);
        assert!(out.items.is_empty());
        assert!(!out.cached);
        // Kein HTTP: Cache bleibt leer (kein fetch_request erreicht wurde).
        assert!(state.cache.lock().unwrap().is_empty());
    }

    #[test]
    fn scan_cache_hit_within_poll_skips_http() {
        let (m, scan) = scan_manifest_fixture();
        let state = test_connector_state(vec![m.clone()]);
        let settings = settings_state_with("sclm", &[("sclm_username", "Semih")]);
        // Erster Scan wurde bei 10_000 ms geladen (Cache-Body wie scan_cache_body).
        {
            let mut cache = state.cache.lock().unwrap();
            cache.insert(
                ("sclm".to_owned(), "scan:my_raffles?u=Semih".to_owned()),
                CachedFetch {
                    fetched_at_ms: 10_000,
                    status: 200,
                    body: json!({
                        "items": [{"sid": "S1", "name": "Event", "raffle_done": true}],
                        "skipped": 1
                    }),
                },
            );
        }
        let out = futures::executor::block_on(scan_inner(
            &state,
            &settings,
            &m,
            &scan,
            "sclm",
            false,
            10_000 + 299_000,
        ))
        .expect("Cache-Treffer");
        assert!(out.cached);
        assert_eq!(out.fetched_at_ms, 10_000);
        assert_eq!(out.skipped, 1);
        assert_eq!(out.items.len(), 1);
        assert_eq!(out.items[0].sid, "S1");
        assert!(out.items[0].raffle_done);
        // has_saved_code kommt NIE aus dem Cache (Keystore leer → fresh false).
        assert!(!out.items[0].has_saved_code);
        // Akzeptanzkriterium: zweiter Lauf innerhalb des Intervalls ohne HTTP —
        // Cache-Größe unverändert, fetch_request nie erreicht.
        assert_eq!(state.cache.lock().unwrap().len(), 1);
    }

    #[test]
    fn scan_username_change_busts_cache_key() {
        let (_m, scan) = scan_manifest_fixture();
        // Key enthält den Username — anderer Name kann nicht denselben Treffer
        // lesen (Schlüsselbildung wie in scan_inner).
        let key_for = |u: &str| format!("scan:{}?u={}", scan.id, u);
        assert_ne!(key_for("Semih"), key_for("Anderer"));
        // Der Username wird getrimmt in den Schlüssel — " Semih " und "Semih"
        // sind derselbe Cache-Raum (sondert: UI kann Whitespace enthalten).
        assert_eq!(key_for(" Semih ".trim()), key_for("Semih"));
    }
    // ---- Härtung t_3cfc7842 (Reviewer-Findings aus t_90382459, Kommentar 609) ----
    // Muster aus der Review-Kopie prev_reviewer_probe.rs übernommen: gezählte
    // Fake-API (std TcpListener) statt von Hand gelegtem Cache. Die Sonden des
    // Reviewers bewiesen den Fehler (nur eprintln); die Tests hier nehmen
    // dieselben adversatorischen Fixtures mit eigenen Literalen und asserten
    // das gehärtete Verhalten.
    mod hardening {
        use super::*;
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::Mutex;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::thread;

        struct Mock {
            port: u16,
            list: AtomicUsize,
            detail: AtomicUsize,
            codes: Mutex<Vec<String>>,
            all_paths: Mutex<Vec<String>>,
        }

        fn route(m: &Mock, path: &str) -> (u16, String) {
            match path {
                "/api/list_traversal" => {
                    m.list.fetch_add(1, Ordering::SeqCst);
                    // Reviewer-Fixture (eigenes Literal): join_code trägt
                    // Pfad-Traversal — remote-gesteuert.
                    m.codes.lock().unwrap().push("../../admin?x=1".into());
                    (
                        200,
                        r#"[{"id":"T1","name":"Traversal","raffle_done":false,"join_code":"../../admin?x=1"}]"#.to_string(),
                    )
                }
                "/api/list_dupes" => {
                    m.list.fetch_add(1, Ordering::SeqCst);
                    // Reviewer-Fixture (eigenes Literal): gleicher sid twice
                    // -> doppelte {#each}-Keys in der Svelte-Liste.
                    m.codes.lock().unwrap().push("C1".into());
                    (
                        200,
                        r#"[{"id":"D1","name":"Erste","raffle_done":false,"join_code":"C1"},
                          {"id":"D1","name":"Zweiter gleicher sid","raffle_done":false,"join_code":"C1"}]"#
                            .to_string(),
                    )
                }
                "/api/session/C1" => {
                    m.detail.fetch_add(1, Ordering::SeqCst);
                    (
                        200,
                        r#"{"id":"D1","participants":[{"name":"Semih"}]}"#.to_string(),
                    )
                }
                "/__counts" => (
                    200,
                    format!(
                        "{{\"list\":{},\"detail\":{},\"codes\":{},\"all_paths\":{}}}",
                        m.list.load(Ordering::SeqCst),
                        m.detail.load(Ordering::SeqCst),
                        serde_json::to_string(&m.codes.lock().unwrap().clone()).unwrap_or_default(),
                        serde_json::to_string(&m.all_paths.lock().unwrap().clone())
                            .unwrap_or_default()
                    ),
                ),
                _ => {
                    // Jeder fremde Pfad (z. B. ein aus dem code-Feld gesteuerter)
                    // zählt als Detail-Versuch — Traversal bleibt sichtbar.
                    if path.starts_with("/api/session/") {
                        m.detail.fetch_add(1, Ordering::SeqCst);
                    }
                    (404, r#"{"error":"nf"}"#.to_string())
                }
            }
        }

        /// Jeder Test bekommt eigenen Server + eigene Zähler (Tests laufen parallel).
        fn mock() -> &'static Mock {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let m: &'static Mock = Box::leak(Box::new(Mock {
                port,
                list: AtomicUsize::new(0),
                detail: AtomicUsize::new(0),
                codes: Mutex::new(Vec::new()),
                all_paths: Mutex::new(Vec::new()),
            }));
            thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { continue };
                    thread::spawn(move || {
                        let mut buf = [0u8; 4096];
                        let n = stream.read(&mut buf).unwrap_or(0);
                        let head = String::from_utf8_lossy(&buf[..n]).to_string();
                        let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                        m.all_paths.lock().unwrap().push(path.clone());
                        let (status, body) = route(m, &path);
                        let reply = format!(
                            "HTTP/1.0 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(reply.as_bytes());
                        let _ = stream.flush();
                    });
                }
            });
            m
        }

        fn counts(base: &str) -> serde_json::Value {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                reqwest::Client::new()
                    .get(format!("{base}/__counts"))
                    .send()
                    .await
                    .unwrap()
                    .json::<serde_json::Value>()
                    .await
                    .unwrap()
            })
        }

        fn hardening_manifest(
            base: &str,
            list_endpoint: &str,
            poll_seconds: u64,
        ) -> connector::ConnectorManifest {
            let text = format!(
                r#"{{
                    "manifest_version": 1, "id": "sclm", "name": {{"de": "S", "en": "S"}},
                    "base_url": "{base}",
                    "placement": {{"kind": "own", "label": {{"de": "S", "en": "S"}}}},
                    "settings_fields": [{{"key": "sclm_username", "label": {{"de": "L", "en": "L"}}, "type": "text"}}],
                    "scan": {{
                        "id": "hard_scan",
                        "title": {{"de": "H", "en": "H"}},
                        "list_endpoint": "{list_endpoint}",
                        "done_field": "raffle_done",
                        "code_field": "join_code",
                        "id_field": "id",
                        "name_field": "name",
                        "detail_endpoint": "/api/session/{{code}}",
                        "match_path": "participants[].name",
                        "match_setting_key": "sclm_username",
                        "done_cap": 2,
                        "concurrency": 2,
                        "poll_seconds": {poll_seconds}
                    }},
                    "views": []
                }}"#
            );
            connector::ConnectorManifest::parse(text.as_bytes()).expect("hardening manifest")
        }

        fn scan_case(
            manifest: &connector::ConnectorManifest,
            tmp: &tempfile::TempDir,
        ) -> (ConnectorState, crate::app_settings::SettingsState) {
            let conn_dir = tmp.path().join("connectors");
            std::fs::create_dir_all(&conn_dir).unwrap();
            std::fs::write(
                conn_dir.join("sclm.json"),
                serde_json::to_string_pretty(manifest).unwrap(),
            )
            .unwrap();
            let state = ConnectorState::load(conn_dir);
            let mut settings = settings::Settings::with_path(tmp.path().join("config.toml"));
            settings.set_connector_setting_value("sclm", "sclm_username", Some("Semih".into()));
            let settings_state = crate::app_settings::SettingsState {
                inner: Mutex::new(settings),
            };
            (state, settings_state)
        }

        // ---- Finding 2: code_field-Sanitisierung ------------------------------------
        #[test]
        fn scan_code_is_safe_literals() {
            // legale Literal-Muster (SCLM-Codeformat)
            assert!(scan_code_is_safe("AB-9_z"));
            assert!(scan_code_is_safe("c1"));
            assert!(scan_code_is_safe("K9x-2_Q"));
            // Reviewer-Sonden-Literal + Varianten, die der Deckel abfängt
            assert!(!scan_code_is_safe("../../admin?x=1"));
            assert!(!scan_code_is_safe("C1/../admin"));
            assert!(!scan_code_is_safe("C1%2F..%2Fadmin")); // doppelt encodiert
            assert!(!scan_code_is_safe("C 1"));
            assert!(!scan_code_is_safe("C1#frag"));
            assert!(!scan_code_is_safe("C1\r\nX-Evil: 1")); // Header-Inject
            assert!(!scan_code_is_safe("C1ünïcode"));
            assert!(!scan_code_is_safe(""));
        }

        #[test]
        fn scan_hardening_code_field_traversal_is_skipped() {
            let m = mock();
            let base = format!("http://127.0.0.1:{}", m.port);
            let manifest = hardening_manifest(&base, "/api/list_traversal", 300);
            let scan = manifest.scan.clone().unwrap();
            let tmp = tempfile::tempdir().unwrap();
            let (state, settings_state) = scan_case(&manifest, &tmp);
            let rt = tokio::runtime::Runtime::new().unwrap();
            let out = rt
                .block_on(scan_inner(
                    &state,
                    &settings_state,
                    &manifest,
                    &scan,
                    "sclm",
                    false,
                    5_000,
                ))
                .unwrap();
            let c = counts(&base);
            let all: Vec<String> = c["all_paths"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap_or_default().to_string())
                .collect();
            // Reviewer-Sonde sah /admin?x=1 auf Serverseite — jetzt: nie.
            assert!(!all.iter().any(|p| p.contains("admin")), "{all:?}");
            assert!(
                all.iter()
                    .all(|p| p.starts_with("/api/list_traversal") || p == "/__counts"),
                "Detail-Fetch trotz unsafe code_field: {all:?}"
            );
            // Skip + warn: Kandidat zählt als skipped, Liste bleibt leer.
            assert_eq!(out.skipped, 1, "{out:?}");
            assert!(out.items.is_empty(), "{out:?}");
            assert_eq!(c["detail"].as_u64(), Some(0), "{c}");
        }

        // ---- Finding 3: sid-Dedup ----------------------------------------------------
        #[test]
        fn scan_hardening_duplicate_sid_is_deduped() {
            let m = mock();
            let base = format!("http://127.0.0.1:{}", m.port);
            let manifest = hardening_manifest(&base, "/api/list_dupes", 300);
            let scan = manifest.scan.clone().unwrap();
            let tmp = tempfile::tempdir().unwrap();
            let (state, settings_state) = scan_case(&manifest, &tmp);
            let rt = tokio::runtime::Runtime::new().unwrap();
            let out = rt
                .block_on(scan_inner(
                    &state,
                    &settings_state,
                    &manifest,
                    &scan,
                    "sclm",
                    false,
                    7_000,
                ))
                .unwrap();
            let sids: Vec<String> = out.items.iter().map(|i| i.sid.clone()).collect();
            // Reviewer-Sonde lieferte ["D1","D1"] (doppelte {#each}-Keys) — jetzt genau einmal.
            assert_eq!(sids, vec!["D1".to_string()], "{out:?}");
            // Das Duplikat zählt als skipped (UI-Hinweis: Liste nicht voll verarbeitbar).
            assert_eq!(out.skipped, 1, "{out:?}");
            // Dedup greift bereits in der Kandidatenauswahl -> nur ein Detail-Fetch.
            let c = counts(&base);
            assert_eq!(c["detail"].as_u64(), Some(1), "{c}");
        }

        // ---- Finding 1: poll_seconds-Deckel ------------------------------------------
        #[test]
        fn scan_hardening_poll_overflow_does_not_panic() {
            let m = mock();
            let base = format!("http://127.0.0.1:{}", m.port);
            // Reviewer-Sonden-Literal: u64::MAX / 500 * 1000 überläuft unchecked.
            let manifest = hardening_manifest(&base, "/api/list_dupes", u64::MAX / 500);
            let scan = manifest.scan.clone().unwrap();
            let tmp = tempfile::tempdir().unwrap();
            let (state, settings_state) = scan_case(&manifest, &tmp);
            let rt = tokio::runtime::Runtime::new().unwrap();
            let first = rt
                .block_on(scan_inner(
                    &state,
                    &settings_state,
                    &manifest,
                    &scan,
                    "sclm",
                    false,
                    5_000,
                ))
                .unwrap();
            assert!(!first.cached, "erster Lauf muss fresh sein: {first:?}");
            // Zweiter Aufruf trifft den Cache-Zweig — dort panizierte die
            // Debug-Multiply (connector.rs:767 am Head 5530a54).
            let second = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                rt.block_on(scan_inner(
                    &state,
                    &settings_state,
                    &manifest,
                    &scan,
                    "sclm",
                    false,
                    6_000,
                ))
            }));
            match second {
                Err(_) => panic!("poll_ttl_ms überläuft weiterhin im Debug-Profil"),
                Ok(Ok(o)) => {
                    assert!(o.cached, "saturating-TTL verweigert Cache-Treffer: {o:?}");
                    assert_eq!(o.fetched_at_ms, 5_000, "{o:?}");
                }
                Ok(Err(e)) => panic!("zweiter Scan-Aufruf fehlschlägt: {e}"),
            }
        }

        #[test]
        fn poll_ttl_ms_is_saturating_not_wrapping() {
            // Reviewer-Sonden-Literal u64::MAX/500: unchecked *1000 wrapte im
            // Release-Profil -> TTL-Bedingung falsch. saturating: klemmt auf MAX.
            assert_eq!(poll_ttl_ms(u64::MAX / 500), u64::MAX);
            assert_eq!(poll_ttl_ms(u64::MAX), u64::MAX);
            // normales Intervall bleibt exakt.
            assert_eq!(poll_ttl_ms(300), 300_000);
        }
    }

    // ---- SCLM v2 Karte C (Spec 5.4): Flow-Öffnung / Auto-Beitritt ----------

    #[test]
    fn examples_sclm_manifest_validates_with_open_flow() {
        // Karte C hat den participant-Flow umgeschrieben (open-Block,
        // enabled_if, keystore_fields, wait-Aktion): das Beispiel muss dem
        // echten Manifest-Validator standhalten (demGate von connector_import).
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/sclm.json");
        let raw = std::fs::read(path).expect("examples/sclm.json ist lesbar");
        let report = connector::validate_manifest(&raw);
        assert!(
            report.errors.is_empty(),
            "Manifest-Fehler: {:?}",
            report.errors
        );
        // v2-Struktur ist wirklich drin (nicht nur "irgendein" grünes Manifest):
        let m = connector::ConnectorManifest::parse(&raw).expect("parst");
        let part = m
            .flows
            .as_ref()
            .unwrap()
            .iter()
            .find(|f| f.id == "participant")
            .expect("participant-Flow");
        let o = part.open.as_ref().expect("open-Block am participant-Flow");
        assert_eq!(o.match_setting_key, "sclm_username");
        assert_eq!(o.done_screen, "results");
        let ids: Vec<&str> = part.screens.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["wishes", "wait", "results"],
            "code_entry/list-entry Screens raus (Requirement 1)"
        );
        let results = part.screens.iter().find(|s| s.id == "results").unwrap();
        assert_eq!(
            results
                .keystore_fields
                .as_ref()
                .and_then(|k| k.get("result_id"))
                .map(String::as_str),
            Some("rid"),
            "result_id-Keystore-Schalter am results-Screen"
        );
        let acts: Vec<&connector::flow::ScreenAction> =
            results.actions.as_ref().unwrap().iter().collect();
        for a in acts {
            assert!(
                a.enabled_if.is_some(),
                "collect/waive sind one-way geschaltet (enabled_if)"
            );
        }
        let wait = part.screens.iter().find(|s| s.id == "wait").unwrap();
        assert!(
            wait.actions
                .as_ref()
                .unwrap()
                .iter()
                .any(|a| a.goto.as_deref() == Some("wishes")),
            "wait trägt „Wünsche ändern“ (Requirement 8)"
        );
    }

    /// reqwest braucht einen Tokio-Reactor (net+time) — reine futures::block_on
    /// reicht für HTTP-Tests nicht. Eigene Current-Thread-Runtime pro Test.
    fn block_on_async<T: Send + 'static>(fut: impl std::future::Future<Output = T> + Send) -> T {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(fut)
    }

    /// Minimal-HTTP-Fixture: antwortet auf (method, path) mit fixem
    /// status+body und protokolliert jeden Treffer (headers/body). Prüft
    /// Join/Claim-Logik und die pcode-Body-Regel gegen echten reqwest-
    /// Durchlauf — nicht nur gegen nachgebaute Logik.
    struct MockHit {
        method: String,
        path: String,
        headers: Vec<(String, String)>,
        body: String,
    }

    fn start_mock_api(
        routes: Vec<(&str, &str, u16, &str)>,
    ) -> (String, std::sync::mpsc::Receiver<MockHit>) {
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::TcpListener;
        use std::sync::mpsc;
        use std::thread;
        let listener = TcpListener::bind("127.0.0.1:0").expect("mock bind");
        let port = listener.local_addr().unwrap().port();
        let routes: Vec<(String, String, u16, String)> = routes
            .into_iter()
            .map(|(m, p, s, b)| (m.to_owned(), p.to_owned(), s, b.to_owned()))
            .collect();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().expect("mock clone"));
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    continue;
                }
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap_or("").to_owned();
                let path = parts.next().unwrap_or("").to_owned();
                let mut headers: Vec<(String, String)> = Vec::new();
                let mut content_length = 0usize;
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
                        break;
                    }
                    if let Some((k, v)) = h.split_once(':') {
                        if k.trim().eq_ignore_ascii_case("content-length") {
                            content_length = v.trim().parse().unwrap_or(0);
                        }
                        headers.push((k.trim().to_owned(), v.trim().to_owned()));
                    }
                }
                let mut body = String::new();
                if content_length > 0 {
                    let mut buf = vec![0u8; content_length];
                    let _ = reader.read_exact(&mut buf);
                    body = String::from_utf8_lossy(&buf).to_string();
                }
                let _ = tx.send(MockHit {
                    method: method.clone(),
                    path: path.clone(),
                    headers,
                    body,
                });
                let (status, resp) = routes
                    .iter()
                    .find(|(m, p, _, _)| *m == method && *p == path)
                    .map(|(_, _, s, b)| (*s, b.clone()))
                    .unwrap_or((500, r#"{"error":"no route"}"#.to_owned()));
                let head = format!(
                    "HTTP/1.1 {status} OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    resp.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
            }
        });
        (format!("http://127.0.0.1:{port}"), rx)
    }

    fn mock_hits(rx: &std::sync::mpsc::Receiver<MockHit>) -> Vec<MockHit> {
        let mut out = Vec::new();
        while let Ok(h) = rx.try_recv() {
            out.push(h);
        }
        out
    }

    ///participant-Flow-Fixture im v2-Schema (open-Block + wishes/wait/results),
    /// base_url zeigt auf die Mock-API. Bewusst identische Feldstruktur wie
    /// examples/sclm.json (Entwurksnah, aber test-fix).
    fn open_flow_manifest_fixture(base: &str) -> connector::ConnectorManifest {
        // Aus Python-dict generiertes, valides JSON ({BASE} wird Injektionsort).
        let text = r#"{
 "manifest_version": 1,
 "id": "sclm",
 "name": {
  "de": "S",
  "en": "S"
 },
 "base_url": "{BASE}",
 "placement": {
  "kind": "own",
  "label": {
   "de": "SCLM",
   "en": "SCLM"
  }
 },
 "settings_fields": [
  {
   "key": "sclm_username",
   "label": {
    "de": "N",
    "en": "N"
   },
   "type": "text"
  }
 ],
 "views": [],
 "flows": [
  {
   "id": "participant",
   "title": {
    "de": "T",
    "en": "T"
   },
   "entry": "wishes",
   "open": {
    "session_endpoint": "/api/session/{code}",
    "headers": {
     "X-SCLM-Code": "{code}"
    },
    "mapping": {
     "sid": "id",
     "name": "name",
     "max_wishes": "max_wishes",
     "raffle_mode": "raffle_mode",
     "raffle_done": "raffle_done"
    },
    "match_path": "participants[].name",
    "id_path": "participants[].id",
    "match_setting_key": "sclm_username",
    "join_endpoint": "/api/session/{sid}/participants",
    "join_headers": {
     "X-SCLM-Code": "{code}"
    },
    "open_screen": "wishes",
    "done_screen": "results"
   },
   "screens": [
    {
     "id": "wishes",
     "type": "list",
     "title": {
      "de": "w",
      "en": "w"
     },
     "endpoint": "/api/session/{code}",
     "params": [
      {
       "key": "code",
       "label": {
        "de": "c",
        "en": "c"
       }
      }
     ],
     "multi": {
      "max_path": "max_wishes"
     },
     "pins": [
      "loot_id"
     ],
     "actions": [
      {
       "id": "confirm",
       "title": {
        "de": "k",
        "en": "k"
       },
       "method": "post",
       "path": "/api/session/{sid}/wishes",
       "params": [
        {
         "key": "code",
         "label": {
          "de": "c",
          "en": "c"
         }
        }
       ],
       "headers": {
        "X-SCLM-Code": "{code}"
       },
       "body_template": {
        "participant_id": "{participant_id}",
        "loot_ids": "{loot_id}",
        "mode": "{raffle_mode}",
        "pcode": "{pcode}"
       },
       "goto": "wait"
      }
     ]
    },
    {
     "id": "wait",
     "type": "wait",
     "title": {
      "de": "w",
      "en": "w"
     },
     "params": [
      {
       "key": "sid",
       "label": {
        "de": "s",
        "en": "s"
       }
      }
     ],
     "exit": {
      "on": "raffle_done",
      "op": "equals",
      "value": true,
      "then": "results"
     },
     "poll_seconds": 15,
     "actions": [
      {
       "id": "edit_wishes",
       "title": {
        "de": "Ändern",
        "en": "Edit"
       },
       "goto": "wishes"
      }
     ]
    },
    {
     "id": "results",
     "type": "results",
     "title": {
      "de": "r",
      "en": "r"
     },
     "endpoint": "/api/session/{sid}/raffle/result_for/{participant_id}",
     "params": [
      {
       "key": "sid",
       "label": {
        "de": "s",
        "en": "s"
       }
      },
      {
       "key": "participant_id",
       "label": {
        "de": "p",
        "en": "p"
       }
      }
     ],
     "mapping": {
      "rid": "result.id",
      "won": "result.won",
      "waived": "result.waived",
      "collected": "result.collected",
      "item_name": "result.item_name"
     },
     "keystore_fields": {
      "result_id": "rid"
     },
     "poll_seconds": 60,
     "actions": [
      {
       "id": "collect",
       "title": {
        "de": "Abgeholt",
        "en": "Collected"
       },
       "method": "patch",
       "path": "/api/session/{sid}/raffle/{rid}",
       "params": [
        {
         "key": "sid",
         "label": {
          "de": "s",
          "en": "s"
         }
        },
        {
         "key": "rid",
         "label": {
          "de": "r",
          "en": "r"
         }
        }
       ],
       "headers": {
        "X-SCLM-Code": "{sid}"
       },
       "body_template": {
        "collected": true,
        "pcode": "{pcode}"
       },
       "enabled_if": {
        "on": "collected",
        "op": "not_equals",
        "value": true
       }
      },
      {
       "id": "waive",
       "title": {
        "de": "Verzichten",
        "en": "Waive"
       },
       "method": "patch",
       "path": "/api/session/{sid}/raffle/{rid}",
       "params": [
        {
         "key": "sid",
         "label": {
          "de": "s",
          "en": "s"
         }
        },
        {
         "key": "rid",
         "label": {
          "de": "r",
          "en": "r"
         }
        }
       ],
       "headers": {
        "X-SCLM-Code": "{sid}"
       },
       "body_template": {
        "waived": true,
        "pcode": "{pcode}"
       },
       "enabled_if": {
        "on": "waived",
        "op": "not_equals",
        "value": true
       }
      }
     ]
    }
   ]
  }
 ]
}"#
        .replace("{BASE}", base);
        serde_json::from_str(&text).expect("fixture parst")
    }

    fn tmp_keystore_file(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("ao-c-flow-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmp dir");
        dir.join("connector_keystore.json")
    }

    fn open_flow_of(m: &connector::ConnectorManifest) -> connector::flow::Flow {
        m.flows.as_ref().unwrap()[0].clone()
    }

    // flow_open_block-Helper entfernt (tote Test-Bequemlichkeit ohne Aufrufer;
    // die Flow-Tests bauen ihre Manifeste direkt über flow_manifest-Builder).

    #[test]
    fn flow_open_new_join_stores_keystore_and_state() {
        let (base, rx) = start_mock_api(vec![
            (
                "GET",
                "/api/session/ABC123",
                200,
                r#"{"id":"sid9","name":"Jumptown #4","max_wishes":3,"raffle_mode":"wish","raffle_done":false,"participants":[]}"#,
            ),
            (
                "POST",
                "/api/session/sid9/participants",
                201,
                r#"{"id":"p7","pcode":"srvpc123"}"#,
            ),
        ]);
        let m = open_flow_manifest_fixture(&base);
        let st = settings_state_with("sclm", &[("sclm_username", "Semih")]);
        let file = tmp_keystore_file("join");
        let out = block_on_async(flow_open_core(
            &m,
            &open_flow_of(&m),
            &file,
            "ABC123",
            &st,
            "sclm",
        ))
        .expect("open läuft durch");
        assert!(out.joined);
        assert!(!out.claimed);
        assert!(!out.pcode_generated, "Server-pcode hat Vorrang");
        assert_eq!(out.screen, "wishes");
        assert_eq!(out.state["sid"], json!("sid9"));
        assert_eq!(out.state["participant_id"], json!("p7"));
        assert_eq!(out.state["pcode"], json!("srvpc123"));
        assert_eq!(out.state["code"], json!("ABC123"));
        assert_eq!(out.state["max_wishes"], json!(3));
        assert_eq!(out.state["raffle_mode"], json!("wish"));
        let hits = mock_hits(&rx);
        assert_eq!(hits.len(), 2, "GET dann POST");
        assert_eq!(hits[1].method, "POST");
        // Join-Body trägt den Settings-Namen (Default-Body).
        let jbody: serde_json::Value = serde_json::from_str(&hits[1].body).unwrap();
        assert_eq!(jbody["name"], json!("Semih"));
        // pcode-Regel (Spec 3): nie im Header/URL — nur potenziell im Body.
        let pcode_leaks = hits[1]
            .headers
            .iter()
            .any(|(k, v)| k.to_lowercase().contains("pcode") || v == "srvpc123")
            || hits[1].path.contains("srvpc123");
        assert!(!pcode_leaks, "pcode nie in Header/URL");
        // Session-GET trägt den Einstiegs-Code als Header.
        assert!(
            hits[0]
                .headers
                .iter()
                .any(|(k, v)| k.eq_ignore_ascii_case("x-sclm-code") && v == "ABC123")
        );
        assert_eq!(
            hits[1].path, "/api/session/sid9/participants",
            "sid aus mapping"
        );
        // Keystore (Karte-A-Mechanik): join_code+sid+Name+participant_id+pcode.
        let e = keystore_find(&keystore_load(&file), "sid9").expect("Eintrag");
        assert_eq!(e.join_code, "ABC123");
        assert_eq!(e.name, "Jumptown #4");
        assert_eq!(e.participant_id.as_deref(), Some("p7"));
        assert_eq!(e.pcode.as_deref(), Some("srvpc123"));
    }

    #[test]
    fn flow_open_registered_participant_no_post_ci_match() {
        // ci-EXAKT: Settings "  SeBo  " trifft Teilnehmer "sebo" — kein POST.
        let (base, rx) = start_mock_api(vec![(
            "GET",
            "/api/session/ABC123",
            200,
            r#"{"id":"sid9","name":"Raid","max_wishes":3,"raffle_mode":"wish","raffle_done":false,
              "participants":[{"id":"pZ","name":"OTHER"},{"id":"pX","name":"sebo"}]}"#,
        )]);
        let m = open_flow_manifest_fixture(&base);
        let st = settings_state_with("sclm", &[("sclm_username", "  SeBo  ")]);
        let file = tmp_keystore_file("reg");
        let out = block_on_async(flow_open_core(
            &m,
            &open_flow_of(&m),
            &file,
            "ABC123",
            &st,
            "sclm",
        ))
        .expect("open läuft durch");
        assert!(!out.joined && !out.claimed);
        assert_eq!(out.state["participant_id"], json!("pX"));
        assert!(out.pcode_generated, "Keystore-pcode fehlt → selbst erzeugt");
        let gen_pcode = out.state["pcode"].as_str().unwrap();
        assert!(gen_pcode.len() >= 16, "generierter pcode ≥16 Zeichen");
        let hits = mock_hits(&rx);
        assert_eq!(hits.len(), 1, "bestehender Name → kein POST");
        // Erzeugter pcode ist gespeichert (Wiederverwendung beim nächsten Öffnen).
        let e = keystore_find(&keystore_load(&file), "sid9").expect("Eintrag");
        assert_eq!(e.pcode.as_deref(), Some(gen_pcode));
        assert_eq!(e.participant_id.as_deref(), Some("pX"));
    }

    #[test]
    fn flow_open_409_claim_takes_id_and_reuses_keystore_pcode() {
        let (base, rx) = start_mock_api(vec![
            (
                "GET",
                "/api/session/ABC123",
                200,
                r#"{"id":"sid9","name":"Raid","max_wishes":3,"raffle_mode":"wish","raffle_done":false,"participants":[]}"#,
            ),
            (
                "POST",
                "/api/session/sid9/participants",
                409,
                r#"{"error":"Name already exists","id":"pY"}"#,
            ),
        ]);
        let m = open_flow_manifest_fixture(&base);
        let st = settings_state_with("sclm", &[("sclm_username", "Semih")]);
        let file = tmp_keystore_file("claim");
        let flow = open_flow_of(&m);
        let out1 = block_on_async(flow_open_core(&m, &flow, &file, "ABC123", &st, "sclm"))
            .expect("Claim-Pfad läuft durch");
        assert!(out1.claimed && !out1.joined);
        assert_eq!(
            out1.state["participant_id"],
            json!("pY"),
            "id aus 409-Fehlerbody"
        );
        assert!(out1.pcode_generated);
        let gen_pcode = out1.state["pcode"].as_str().unwrap().to_owned();
        assert!(gen_pcode.len() >= 16);
        assert_eq!(out1.screen, "wishes");
        // Zweites Öffnen derselben Session: pcode wird wiederverwendet.
        let out2 = block_on_async(flow_open_core(&m, &flow, &file, "ABC123", &st, "sclm"))
            .expect("zweiter open");
        assert_eq!(out2.state["pcode"].as_str().unwrap(), gen_pcode);
        assert!(
            !out2.pcode_generated,
            "Keystore-pcode gewinnt, kein Neu-Generieren"
        );
        let hits = mock_hits(&rx);
        assert_eq!(
            hits.len(),
            4,
            "zwei Öffnungen = 2 GET + 2 POST (jedes Mal 409)"
        );
        assert!(hits[1].path.contains("/participants"));
        assert!(
            !hits[1].path.contains(&gen_pcode),
            "generierter pcode nie in URL/Header"
        );
        assert!(hits[1].headers.iter().all(|(_, v)| v != &gen_pcode));
    }

    #[test]
    fn flow_open_missing_username_aborts_without_requests() {
        // Spec 6 Punkt 4: kein Rate-Schaden, keine leeren POSTs.
        let (base, rx) = start_mock_api(vec![]);
        let m = open_flow_manifest_fixture(&base);
        let st = settings_state_with("sclm", &[("sclm_username", "   ")]);
        let file = tmp_keystore_file("nouser");
        let err = block_on_async(flow_open_core(
            &m,
            &open_flow_of(&m),
            &file,
            "ABC123",
            &st,
            "sclm",
        ))
        .expect_err("fehlender Username → Abbruch");
        assert!(err.contains("flow_username_missing"), "{err}");
        assert!(mock_hits(&rx).is_empty(), "null HTTP-Requests");
        assert!(!file.exists(), "kein Keystore-Schaden");
    }

    #[test]
    fn flow_open_rejects_bad_code_shape_without_requests() {
        let (base, rx) = start_mock_api(vec![]);
        let m = open_flow_manifest_fixture(&base);
        let st = settings_state_with("sclm", &[("sclm_username", "Semih")]);
        let file = tmp_keystore_file("badcode");
        for bad in ["../admin", "abc 123", "", &"x".repeat(80)[..]] {
            let err = block_on_async(flow_open_core(
                &m,
                &open_flow_of(&m),
                &file,
                bad,
                &st,
                "sclm",
            ))
            .expect_err("Form-Guardrail greift");
            assert!(err.contains("param_invalid"), "{bad}: {err}");
        }
        assert!(mock_hits(&rx).is_empty(), "Guardrail vor jedem Request");
    }

    #[test]
    fn flow_open_raffle_done_opens_results_directly() {
        // Auto-Ergebnis (Requirement 7): schon gelaufene Ziehung → done_screen.
        let (base, _rx) = start_mock_api(vec![(
            "GET",
            "/api/session/ABC123",
            200,
            r#"{"id":"sid9","name":"Raid","max_wishes":3,"raffle_mode":"wish","raffle_done":true,
              "participants":[{"id":"pX","name":"Semih"}]}"#,
        )]);
        let m = open_flow_manifest_fixture(&base);
        let st = settings_state_with("sclm", &[("sclm_username", "Semih")]);
        let file = tmp_keystore_file("done");
        let out = block_on_async(flow_open_core(
            &m,
            &open_flow_of(&m),
            &file,
            "ABC123",
            &st,
            "sclm",
        ))
        .expect("open läuft durch");
        assert_eq!(out.screen, "results");
        assert!(!out.joined);
    }

    #[test]
    fn flow_open_session_404_maps_to_clear_error() {
        let (base, rx) = start_mock_api(vec![(
            "GET",
            "/api/session/NOPE99",
            404,
            r#"{"error":"not found"}"#,
        )]);
        let m = open_flow_manifest_fixture(&base);
        let st = settings_state_with("sclm", &[("sclm_username", "Semih")]);
        let file = tmp_keystore_file("404");
        let err = block_on_async(flow_open_core(
            &m,
            &open_flow_of(&m),
            &file,
            "NOPE99",
            &st,
            "sclm",
        ))
        .expect_err("404 → Fehler");
        assert!(err.contains("flow_session_not_found"), "{err}");
        let hits = mock_hits(&rx);
        assert_eq!(
            hits.len(),
            1,
            "nur die Session-Lastung, kein Join-POST danach"
        );
        assert!(!file.exists(), "unbekannte Session → kein Keystore-Eintrag");
    }

    #[test]
    fn wishes_action_sends_real_array_and_pcode_only_in_body() {
        // Akzeptanz "wishes-Array-Typ" + "pcode nie im Header, immer Body".
        let (base, rx) = start_mock_api(vec![(
            "POST",
            "/api/session/sid9/wishes",
            201,
            r#"{"ok":true,"wishes":[{"id":"w1","loot_id":"L1"}]}"#,
        )]);
        let m = open_flow_manifest_fixture(&base);
        let action = m.flows.as_ref().unwrap()[0].screens[0]
            .actions
            .as_ref()
            .unwrap()[0]
            .clone();
        let st = settings_state_with("sclm", &[]);
        let mut values: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        values.insert("sid".into(), json!("sid9"));
        values.insert("code".into(), json!("ABC123"));
        values.insert("participant_id".into(), json!("p7"));
        values.insert("raffle_mode".into(), json!("no_interest"));
        values.insert("pcode".into(), json!("genPcode23456789abcdef"));
        values.insert("loot_id".into(), json!(["L1", "L2", "L3"]));
        let (status, _body) = block_on_async(screen_action_core(&m, &action, values, &st, "sclm"))
            .expect("action läuft durch");
        assert_eq!(status, 201);
        let hits = mock_hits(&rx);
        assert_eq!(hits.len(), 1);
        let sent: serde_json::Value = serde_json::from_str(&hits[0].body).unwrap();
        assert_eq!(
            sent["loot_ids"],
            json!(["L1", "L2", "L3"]),
            "echtes JSON-Array, keine Stringified-Liste"
        );
        assert_eq!(
            sent["mode"],
            json!("no_interest"),
            "mode folgt dem Session-raffle_mode"
        );
        assert_eq!(sent["pcode"], json!("genPcode23456789abcdef"));
        assert!(
            !hits[0].path.contains("genPcode23456789abcdef"),
            "pcode nicht in URL"
        );
        assert!(
            hits[0].headers.iter().all(|(k, v)| {
                !k.to_lowercase().contains("pcode") && v != "genPcode23456789abcdef"
            }),
            "pcode nicht in Headern"
        );
        assert!(
            hits[0]
                .headers
                .iter()
                .any(|(k, v)| k.eq_ignore_ascii_case("x-sclm-code") && v == "ABC123")
        );
    }

    #[test]
    fn fetch_inner_keystore_fields_writes_result_id_on_unique_hit() {
        // Spec 5.2: results-Screen schreibt result_id in die Keystore-Spalte,
        // sobald der Fetch einen eindeutigen Treffer liefert.
        let (base, _rx) = start_mock_api(vec![(
            "GET",
            "/api/session/sid9/raffle/result_for/p7",
            200,
            r#"{"result":{"id":"r9","won":true,"waived":false,"collected":false,"item_name":"Cutlass Black"}}"#,
        )]);
        let m = open_flow_manifest_fixture(&base);
        let st = settings_state_with("sclm", &[]);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("ao-c-fetch-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmp dir");
        let cs = ConnectorState {
            dir: dir.join("connectors"),
            entries: std::sync::Mutex::new(vec![StoredConnector {
                raw: String::new(),
                manifest: m.clone(),
                value: serde_json::Value::Null,
            }]),
            cache: std::sync::Mutex::new(HashMap::new()),
        };
        let mut params: BTreeMap<String, String> = BTreeMap::new();
        params.insert("sid".into(), "sid9".into());
        params.insert("participant_id".into(), "p7".into());
        params.insert("code".into(), "ABC123".into());
        let res = block_on_async(fetch_inner(
            &cs,
            &st,
            "sclm",
            "participant:results",
            true,
            params.clone(),
        ))
        .expect("fetch läuft durch");
        let rows = res.mapped.as_array().expect("results gemappt als Zeilen");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["rid"], json!("r9"));
        let file = keystore_paths(&cs).file("sclm");
        let e = keystore_find(&keystore_load(&file), "sid9").expect("Eintrag");
        assert_eq!(e.result_id.as_deref(), Some("r9"));
    }

    #[test]
    fn fetch_inner_keystore_fields_null_result_writes_nothing() {
        // result_for → 200 {\"result\": null}: Spalte bleibt leer (null-safe).
        let (base, _rx) = start_mock_api(vec![(
            "GET",
            "/api/session/sid9/raffle/result_for/p7",
            200,
            r#"{"result":null}"#,
        )]);
        let m = open_flow_manifest_fixture(&base);
        let st = settings_state_with("sclm", &[]);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("ao-c-fetch-null-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmp dir");
        let cs = ConnectorState {
            dir: dir.join("connectors"),
            entries: std::sync::Mutex::new(vec![StoredConnector {
                raw: String::new(),
                manifest: m.clone(),
                value: serde_json::Value::Null,
            }]),
            cache: std::sync::Mutex::new(HashMap::new()),
        };
        let mut params: BTreeMap<String, String> = BTreeMap::new();
        params.insert("sid".into(), "sid9".into());
        params.insert("participant_id".into(), "p7".into());
        let res = block_on_async(fetch_inner(
            &cs,
            &st,
            "sclm",
            "participant:results",
            true,
            params,
        ))
        .expect("fetch läuft durch");
        assert_eq!(
            res.mapped.as_array().map(|a| a.len()),
            Some(0),
            "kein Ergebnis → keine Zeile"
        );
        let file = keystore_paths(&cs).file("sclm");
        assert!(
            !file.exists() || keystore_find(&keystore_load(&file), "sid9").is_none(),
            "null-Ergebnis schreibt nichts"
        );
    }

    #[test]
    fn generate_pcode_is_long_urlsafe_and_unique() {
        let a = generate_pcode();
        let b = generate_pcode();
        assert!(a.len() >= 16 && b.len() >= 16);
        assert_ne!(a, b, "CSPRNG, keine Kette");
        assert!(
            a.chars()
                .all(|c| c.is_ascii_alphanumeric() && !matches!(c, 'l' | '1' | 'I' | 'O' | '0'))
        );
    }
}
