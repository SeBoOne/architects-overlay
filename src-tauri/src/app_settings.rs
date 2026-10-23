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

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, Ordering},
};

use serde::Serialize;
use settings::{CommoditySource, DetailVariant, Palette, Settings};

/// Shared settings handle. Loaded once at setup; every setter mutates and
/// immediately persists the same instance (single source of truth).
pub struct SettingsState {
    inner: Mutex<Settings>,
    /// Live commodity-source switch (0 = wiki, 1 = uex) shared with the
    /// composite provider — see `providers::SourceSwitch`. Kept in sync with
    /// the persisted value by every setter.
    pub source_switch: Arc<AtomicU8>,
}

impl SettingsState {
    /// Load from the standard config path; a broken/missing config falls back
    /// to defaults (a broken config must not prevent the overlay from
    /// starting — same policy as the API-key load in `setup_trade`).
    pub fn load() -> SettingsState {
        let settings = settings::Settings::load().unwrap_or_default();
        SettingsState {
            source_switch: Arc::new(AtomicU8::new(match settings.commodity_source() {
                CommoditySource::Wiki => 0,
                CommoditySource::Uex => 1,
            })),
            inner: Mutex::new(settings),
        }
    }

    /// Current commodity price source (for the composite provider's boot
    /// switch; live flips go through `source_switch`).
    pub fn commodity_source(&self) -> CommoditySource {
        self.inner.lock().expect("settings lock").commodity_source()
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
    let mut s = state.inner.lock().expect("settings lock");
    s.set_commodity_source(source);
    state.persist(&s);
    state.source_switch.store(
        match source {
            CommoditySource::Wiki => 0,
            CommoditySource::Uex => 1,
        },
        Ordering::SeqCst,
    );
    Ok(SettingsSnapshot::from_settings(&s))
}
