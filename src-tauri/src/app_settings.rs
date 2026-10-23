//! # Settings IPC commands
//!
//! Bridges the `settings` crate (config.toml persistence) to the palette
//! frontend. The frontend reads the full settings snapshot once at boot
//! (to apply `data-palette` / `data-detail-variant`) and writes individual
//! values from the Config-View. Every setter persists immediately — there is
//! no save button, matching the Config-View contract (t_163074f3).
//!
//! Secrets: the UEX API key is never returned to the frontend, only its
//! "set / not set" status.

use std::sync::Mutex;

use serde::Serialize;
use settings::{CommoditySource, DetailVariant, Palette, Settings};

/// Shared settings handle. Loaded once at setup; every setter mutates and
/// immediately persists the same instance (single source of truth).
pub struct SettingsState {
    inner: Mutex<Settings>,
    /// Live commodity-source switch shared with the composite provider —
    /// see `providers::SourceSwitch`. Kept in sync with the persisted value
    /// by every setter.
    pub source_switch: crate::providers::SourceSwitch,
}

impl SettingsState {
    /// Load from the standard config path; a broken/missing config falls back
    /// to defaults (a broken config must not prevent the overlay from
    /// starting — same policy as the API-key load in `setup_trade`).
    pub fn load() -> SettingsState {
        let settings = settings::Settings::load().unwrap_or_default();
        SettingsState {
            source_switch: crate::providers::SourceSwitch::new(settings.commodity_source()),
            inner: Mutex::new(settings),
        }
    }

    /// The LIVE part of `settings_set_commodity_source`: mutate the settings
    /// snapshot AND flip the live source switch the composite provider reads
    /// on every price fetch. The tauri command adds only the disk persist on
    /// top — one shared code path so the regression wiring test
    /// (lib.rs `wiring_tests`) exercises exactly the setter's flip sequence.
    pub(crate) fn set_commodity_source_live(&self, source: CommoditySource) {
        self.inner
            .lock()
            .expect("settings lock")
            .set_commodity_source(source);
        self.source_switch.set(source);
    }

    fn persist(&self, settings: &Settings) {
        if let Err(e) = settings.save() {
            log::warn!("settings: persist failed: {e}");
        }
    }
}

/// Frontend-facing snapshot. The API key is intentionally only a boolean —
/// the raw value never leaves the backend.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub palette: String,
    pub detail_variant: String,
    pub uex_api_key_set: bool,
    /// Primary commodity price source ("wiki" | "uex"); wiki first, UEX
    /// fallback. Exposed so the Config-View can A/B the sources.
    pub commodity_source: String,
}

impl SettingsSnapshot {
    fn from_settings(s: &Settings) -> Self {
        SettingsSnapshot {
            palette: s.palette().as_str().to_owned(),
            detail_variant: s.detail_variant().as_str().to_owned(),
            uex_api_key_set: s.uex_api_key().is_some(),
            commodity_source: s.commodity_source().as_str().to_owned(),
        }
    }
}

/// Read the current settings snapshot.
#[tauri::command]
pub fn settings_get(state: tauri::State<SettingsState>) -> SettingsSnapshot {
    let s = state.inner.lock().expect("settings lock");
    SettingsSnapshot::from_settings(&s)
}

/// Set the colour palette (amber | mfcd | bloom). Persists immediately.
#[tauri::command]
pub fn settings_set_palette(
    state: tauri::State<SettingsState>,
    name: String,
) -> Result<SettingsSnapshot, String> {
    let palette = Palette::from_name(&name).ok_or_else(|| format!("unknown palette: {name}"))?;
    let mut s = state.inner.lock().expect("settings lock");
    s.set_palette(palette);
    state.persist(&s);
    Ok(SettingsSnapshot::from_settings(&s))
}

/// Set the price-detail variant (c | a). Persists immediately.
#[tauri::command]
pub fn settings_set_detail_variant(
    state: tauri::State<SettingsState>,
    name: String,
) -> Result<SettingsSnapshot, String> {
    let variant =
        DetailVariant::from_name(&name).ok_or_else(|| format!("unknown variant: {name}"))?;
    let mut s = state.inner.lock().expect("settings lock");
    s.set_detail_variant(variant);
    state.persist(&s);
    Ok(SettingsSnapshot::from_settings(&s))
}

/// Set (Some) or remove (None) the UEX API key. Persists immediately.
/// The key itself is never echoed back.
#[tauri::command]
pub fn settings_set_uex_api_key(
    state: tauri::State<SettingsState>,
    key: Option<String>,
) -> SettingsSnapshot {
    let mut s = state.inner.lock().expect("settings lock");
    s.set_uex_api_key(key.filter(|k| !k.trim().is_empty()));
    state.persist(&s);
    SettingsSnapshot::from_settings(&s)
}

/// Set the primary commodity price source (wiki | uex). Persists immediately
/// AND flips the live source switch the composite provider reads on every
/// price fetch — the next price request already uses the new source.
#[tauri::command]
pub fn settings_set_commodity_source(
    state: tauri::State<SettingsState>,
    name: String,
) -> Result<SettingsSnapshot, String> {
    let source = CommoditySource::from_name(&name)
        .ok_or_else(|| format!("unknown commodity source: {name}"))?;
    state.set_commodity_source_live(source);
    let s = state.inner.lock().expect("settings lock");
    state.persist(&s);
    Ok(SettingsSnapshot::from_settings(&s))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression (t_c3d46cff): the switch handed to the composite provider
    /// (lib.rs clones `state.source_switch`) MUST be the same instance the
    /// setter flips — a clone shares the inner Arc, a fresh switch would
    /// freeze the boot value until the next app restart.
    #[test]
    fn live_source_switch_flips_without_restart() {
        let state = SettingsState::load();
        let reader = state.source_switch.clone();
        // what settings_set_commodity_source does on a live flip:
        state.source_switch.set(CommoditySource::Uex);
        assert_eq!(reader.get(), CommoditySource::Uex);
        state.source_switch.set(CommoditySource::Wiki);
        assert_eq!(reader.get(), CommoditySource::Wiki);
    }
}
