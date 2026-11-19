//! Strata-Mining-Modus (M7-K2, t_bb3303d0) — Tauri-Commands über
//! `strata_client` (M7-K1, t_e38faa51).
//!
//! * **Key-Handling:** der `celd_`-Key lebt NIE in der Settings-Config —
//!   nur die `key_ref` (Settings) + der Wert in der Keystore-Datei
//!   `strata-key.json` (0o600, `strata_client::keystore`). Setzen/Entfernen
//!   geht über [`strata_set_key`]; der Snapshot/Logs tragen nur `_set`-Booles.
//! * **Cache-First:** alle Reads laufen über den geteilten SQLite-Cache
//!   (TTLs im strata-client) — 1 Request pro TTL-Fenster, die 120/60s-Quote
//!   ist damit nie ein Thema.
//! * **Fehler:** `strata_client::error_key` mappt jeden ProviderError auf
//!   einen `error.strata.*`-i18n-Key (JSON-Contract T2) — das Frontend
//!   resolviert über die Locale-Fallback-Kette.
//!
//! Attribution (ToS-Pflicht) reist in JEDEM Response-Objekt mit und wird
//! von der UI als Footer gezeigt — die Commands kürzen sie nie weg.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;

use crate::app_settings::SettingsState;
use crate::i18n_err::err_key;

const DEFAULT_KEY_REF: &str = strata_client::keystore::DEFAULT_KEY_REF;

/// Geteilter Zustand für die Strata-Commands: der im Setup geöffnete
/// SQLite-Cache (geteilt mit TradeState — ein Cache-Handle, eine DB) und
/// der Keystore-Pfad (`app_local_data_dir/strata-key.json`).
pub struct StrataState {
    pub cache: Arc<cache::Cache>,
    pub keystore_path: PathBuf,
}

/// Status-Sicht für die UI — bool-only, der Key selbst verlässt den
/// Keystore nie.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct StrataStatus {
    pub enabled: bool,
    pub key_set: bool,
    /// Keystore-Bezeichner (kein Secret — der Name des Eintrags).
    pub key_ref: String,
    /// Welle-2-Vorbelegung: Konzentrations-Tier-Schwelle (1–6).
    pub tier_default: u32,
}

fn key_ref_of(s: &settings::Settings) -> String {
    s.strata_key_ref().unwrap_or(DEFAULT_KEY_REF).to_owned()
}

fn status(state: &tauri::State<StrataState>, s: &settings::Settings) -> StrataStatus {
    let key_ref = key_ref_of(s);
    let key_set = strata_client::keystore::resolve_key(&state.keystore_path, &key_ref).is_some();
    StrataStatus {
        enabled: s.strata_enabled(),
        key_set,
        key_ref,
        tier_default: s.strata_default_tier_threshold(),
    }
}

fn strata_error(e: &provider::ProviderError) -> String {
    strata_client::error_key(e)
}

/// Client pro Call gebaut (Key-Wechsel greift sofort); der Cache ist
/// geteilt — gecachte Responses überleben den Client-Bau kostenlos.
fn client(
    state: &StrataState,
    s: &settings::Settings,
) -> Result<strata_client::StrataClient, String> {
    let key_ref = key_ref_of(s);
    let key = strata_client::StrataClient::resolve_key(&state.keystore_path, &key_ref)
        .map_err(|e| strata_error(&e))?;
    strata_client::StrataClient::new(state.cache.clone(), &key).map_err(|e| strata_error(&e))
}

/// Settings-Lock-Helfer: alle Reads gehen über den geteilten SettingsState
/// (keine zweite Load-Instanz — Settings-State-Divergenz-Gefahr).
fn snapshot_of(settings: &tauri::State<SettingsState>) -> settings::Settings {
    settings.inner.lock().expect("settings lock").clone()
}

/// Status für die Mining-UI (Key-Missing-State + Settings-Anzeige).
#[tauri::command]
pub fn strata_status(
    state: tauri::State<StrataState>,
    settings: tauri::State<SettingsState>,
) -> StrataStatus {
    let s = snapshot_of(&settings);
    status(&state, &s)
}

/// Set (Some) oder entfernt (None) den persönlichen `celd_`-Key. Leere/
/// Whitespace-Eingaben sind KEIN Write (Upsert-Vertrag des Keystores) —
/// die UI deaktiviert den Set-Button bei leerem Feld. Persistiert die
/// Settings mit dem Default-`key_ref`, damit der Eintrag gefunden wird.
#[tauri::command]
pub fn strata_set_key(
    state: tauri::State<StrataState>,
    settings: tauri::State<SettingsState>,
    key: Option<String>,
) -> Result<StrataStatus, String> {
    // Leere/Whitespace-Eingabe: kein Write (kein Fake-Erfolg) — die UI
    // deaktiviert den Set-Button bei leerem Feld, hier nochmal defensiv.
    if key
        .as_deref()
        .map(str::trim)
        .map(str::is_empty)
        .unwrap_or(false)
    {
        return Err(err_key("error.strata.bad_key"));
    }
    {
        let mut s = settings.inner.lock().expect("settings lock");
        s.set_strata_key_ref(Some(DEFAULT_KEY_REF.to_owned()));
        crate::app_settings::persist_settings(&s);
    }
    let value = key.as_deref().map(str::trim);
    let wrote = strata_client::keystore::set_key(&state.keystore_path, DEFAULT_KEY_REF, value);
    if !wrote {
        return Err(err_key("error.strata.bad_key"));
    }
    let s = snapshot_of(&settings);
    Ok(status(&state, &s))
}

/// `GET /api/public/ores` (cache-first) — Erz-Katalog für die Suche.
#[tauri::command]
pub async fn strata_ores(
    state: tauri::State<'_, StrataState>,
    settings: tauri::State<'_, SettingsState>,
) -> Result<strata_client::OresResponse, String> {
    let c = client(&state, &snapshot_of(&settings))?;
    c.ores().await.map_err(|e| strata_error(&e))
}

/// `GET /api/public/locations` (cache-first) — Ort-Katalog für die Suche.
#[tauri::command]
pub async fn strata_locations(
    state: tauri::State<'_, StrataState>,
    settings: tauri::State<'_, SettingsState>,
) -> Result<strata_client::LocationsResponse, String> {
    let c = client(&state, &snapshot_of(&settings))?;
    c.locations().await.map_err(|e| strata_error(&e))
}

/// `GET /api/public/ore-locations/{ore}?sort=rank|concentration` — Orte pro
/// Erz, gruppiert nach space/surface/cave/exploration. `sort` kommt als
/// Snake-Case-Param vom Frontend (`rank` | `concentration`).
#[tauri::command]
pub async fn strata_ore_locations(
    state: tauri::State<'_, StrataState>,
    settings: tauri::State<'_, SettingsState>,
    ore: String,
    sort: String,
) -> Result<strata_client::OreLocationsResponse, String> {
    let sort = match sort.trim() {
        "concentration" => strata_client::OreSort::Concentration,
        _ => strata_client::OreSort::Rank,
    };
    let c = client(&state, &snapshot_of(&settings))?;
    c.ore_locations(ore.trim(), sort)
        .await
        .map_err(|e| strata_error(&e))
}

/// `GET /api/public/location-ores/{location}` (cache-first) — Erze am Ort,
/// nach Konzentration absteigend (Ort-Detail, M7-F2 — reine Info, kein Pin).
#[tauri::command]
pub async fn strata_location_ores(
    state: tauri::State<'_, StrataState>,
    settings: tauri::State<'_, SettingsState>,
    location: String,
) -> Result<strata_client::LocationOresResponse, String> {
    let c = client(&state, &snapshot_of(&settings))?;
    c.location_ores(location.trim())
        .await
        .map_err(|e| strata_error(&e))
}

/// `GET /api/public/items` (cache-first, TTL 7d) — Item-Katalog für den
/// Crafting-Modus (M8-K2).
#[tauri::command]
pub async fn strata_items(
    state: tauri::State<'_, StrataState>,
    settings: tauri::State<'_, SettingsState>,
) -> Result<strata_client::ItemsResponse, String> {
    let c = client(&state, &snapshot_of(&settings))?;
    c.items().await.map_err(|e| strata_error(&e))
}

/// `GET /api/public/craft-cost/{item}` (cache-first, TTL 7d) — Materialliste
/// + Craft-Zeit eines Items. `{item}` ist die UUID (id vor Slug — die UI
/// reicht `Item.id` durch, nie den Slug).
#[tauri::command]
pub async fn strata_craft_cost(
    state: tauri::State<'_, StrataState>,
    settings: tauri::State<'_, SettingsState>,
    item: String,
) -> Result<strata_client::CraftCostResponse, String> {
    let c = client(&state, &snapshot_of(&settings))?;
    c.craft_cost(item.trim())
        .await
        .map_err(|e| strata_error(&e))
}

/// Keystore-Pfad-Konvention: `app_local_data_dir/strata-key.json` (neben
/// `cache.sqlite3`) — ein Verzeichnis für alle App-Daten.
pub fn keystore_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    app.path()
        .app_local_data_dir()
        .ok()
        .map(|d| d.join("strata-key.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// M7-K2: der Status trägt NIE den Key — nur Booles + key_ref-Name.
    #[test]
    fn status_is_bool_only() {
        let s = serde_json::to_value(StrataStatus {
            enabled: true,
            key_set: true,
            key_ref: "default".to_owned(),
            tier_default: 4,
        })
        .unwrap();
        let map = s.as_object().unwrap();
        assert!(map.contains_key("enabled") && map.contains_key("key_set"));
        assert!(!serde_json::to_string(&s).unwrap().contains("celd_"));
    }

    /// Sort-Param-Normalisierung: unbekannte Werte fallen auf `rank`
    /// (API-Default) — kein 500er für Typos.
    #[test]
    fn sort_param_defaults_to_rank() {
        // Der Match in strata_ore_locations ist inline; hier der Contract:
        // "concentration" → Concentration, alles andere → Rank.
        let sort = |v: &str| match v.trim() {
            "concentration" => strata_client::OreSort::Concentration,
            _ => strata_client::OreSort::Rank,
        };
        assert_eq!(sort("concentration"), strata_client::OreSort::Concentration);
        assert_eq!(sort("rank"), strata_client::OreSort::Rank);
        assert_eq!(sort(" Rubbish "), strata_client::OreSort::Rank);
    }

    /// M8-K2: Craft-Cost-Einheiten werden NIE gemischt — der UI-Vertrag
    /// (farblich getrennte SCU/gems/items, keine Summen über Einheiten hinweg)
    /// stützt sich auf die CraftUnit-Diskriminierung aus K1.
    #[test]
    fn craft_cost_units_stay_discriminated() {
        use strata_client::{CraftCostEntry, CraftUnit};
        let e = |unit: CraftUnit| CraftCostEntry {
            material: "X".into(),
            ore_id: None,
            quantity: 1.0,
            unit,
            min_quality: None,
        };
        assert_ne!(e(CraftUnit::Scu), e(CraftUnit::Gems));
        assert_ne!(e(CraftUnit::Gems), e(CraftUnit::Items));
        assert_ne!(e(CraftUnit::Scu), e(CraftUnit::Items));
    }
}
