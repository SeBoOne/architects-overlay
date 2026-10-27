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

    /// C2a (t_6565fe8b): the canonical shortcut strings. overlay::init reads
    /// them at startup; the setter commands write them. Single source of
    /// truth for both the OS registration and C2b's compositor binds.
    pub fn shortcut_combos(&self) -> (String, String) {
        let s = self.inner.lock().expect("settings lock");
        (
            s.shortcut_toggle().to_owned(),
            s.shortcut_click_through().to_owned(),
        )
    }

    fn persist(&self, settings: &Settings) {
        if let Err(e) = settings.save() {
            log::warn!("settings: persist failed: {e}");
        }
    }
}

/// Frontend-facing snapshot. The API key is intentionally only a boolean —
/// the raw value never leaves the backend.
///
/// Wire contract (t_9b385831, v0.3.3): field names serialize as written
/// (snake_case). The frontend reads them snake_case (`cfg.uex_api_key_set`,
/// `cfg.commodity_source`, `cfg.detail_variant`). A `rename_all = "camelCase"`
/// here silently renamed those three fields to `uexApiKeySet`/
/// `commoditySource`/`detailVariant` on the wire: the frontend saw
/// `undefined` for every underscore field — the API-key status was forever
/// "nicht gesetzt" and the price-source chips never highlighted, even though
/// the backend persisted both correctly. The serialization test below pins
/// the exact key set; single-word fields (palette, scale) never broke, which
/// is why the failure looked partial.
#[derive(Debug, Serialize)]
pub struct SettingsSnapshot {
    pub palette: String,
    pub detail_variant: String,
    pub uex_api_key_set: bool,
    /// Primary commodity price source ("wiki" | "uex"); wiki first, UEX
    /// fallback. Exposed so the Config-View can A/B the sources.
    pub commodity_source: String,
    /// K2 (t_5fe8eb4c, P12): font/icon scale in % (50..200, default 100).
    pub scale: u16,
    /// C2a (t_6565fe8b): kanonische, persistierte Kürzel (Soll — das, was
    /// der Nutzer gestellt hat und was überlebt).
    pub shortcut_toggle: String,
    pub shortcut_click_through: String,
    /// C2a: Ist — meldet, ob das OS die globalen Kürzel WIRKLICH registriert
    /// hat (Wayland: false, dort greift der Compositor-Pfad; C2b bindet aus
    /// den Soll-Werten). Soll-Ist wie beim Preisquellen-Feedback (K6).
    pub global_shortcuts_active: bool,
}

impl SettingsSnapshot {
    fn from_settings(s: &Settings, global_shortcuts_active: bool) -> Self {
        SettingsSnapshot {
            palette: s.palette().as_str().to_owned(),
            detail_variant: s.detail_variant().as_str().to_owned(),
            uex_api_key_set: s.uex_api_key().is_some(),
            commodity_source: s.commodity_source().as_str().to_owned(),
            scale: s.scale(),
            shortcut_toggle: s.shortcut_toggle().to_owned(),
            shortcut_click_through: s.shortcut_click_through().to_owned(),
            global_shortcuts_active,
        }
    }
}

/// C2a pure validation stage shared by both shortcut commands: settings-crate
/// rules (≥1 Mod + 1 Taste, no Escape, internal conflict list) PLUS the
/// cross-field rule — the two fields must not end up with the same combo.
/// Kept tauri-free so it is unit-testable without a live app.
fn validate_shortcut_input(
    combo: &str,
    peer_combo: &str,
    peer_label: &str,
) -> Result<String, String> {
    let canonical = settings::validate_shortcut_combo(combo)?;
    if canonical == peer_combo {
        return Err(format!(
            "Diese Kombination ist bereits das {peer_label}-Kürzel — zwei Kürzel derselben Taste doppelt vergeben geht nicht."
        ));
    }
    Ok(canonical)
}

/// Read the current settings snapshot.
#[tauri::command]
pub fn settings_get(state: tauri::State<SettingsState>, app: tauri::AppHandle) -> SettingsSnapshot {
    let s = state.inner.lock().expect("settings lock");
    SettingsSnapshot::from_settings(&s, crate::overlay::global_shortcuts_active(&app))
}

/// Set the colour palette (amber | mfcd | bloom). Persists immediately.
#[tauri::command]
pub fn settings_set_palette(
    state: tauri::State<SettingsState>,
    app: tauri::AppHandle,
    name: String,
) -> Result<SettingsSnapshot, String> {
    let palette = Palette::from_name(&name).ok_or_else(|| format!("unknown palette: {name}"))?;
    let mut s = state.inner.lock().expect("settings lock");
    s.set_palette(palette);
    state.persist(&s);
    Ok(SettingsSnapshot::from_settings(
        &s,
        crate::overlay::global_shortcuts_active(&app),
    ))
}

/// Set the price-detail variant (c | a). Persists immediately.
#[tauri::command]
pub fn settings_set_detail_variant(
    state: tauri::State<SettingsState>,
    app: tauri::AppHandle,
    name: String,
) -> Result<SettingsSnapshot, String> {
    let variant =
        DetailVariant::from_name(&name).ok_or_else(|| format!("unknown variant: {name}"))?;
    let mut s = state.inner.lock().expect("settings lock");
    s.set_detail_variant(variant);
    state.persist(&s);
    Ok(SettingsSnapshot::from_settings(
        &s,
        crate::overlay::global_shortcuts_active(&app),
    ))
}

/// Set (Some) or remove (None) the UEX API key. Persists immediately.
/// The key itself is never echoed back.
#[tauri::command]
pub fn settings_set_uex_api_key(
    state: tauri::State<SettingsState>,
    app: tauri::AppHandle,
    key: Option<String>,
) -> SettingsSnapshot {
    let mut s = state.inner.lock().expect("settings lock");
    s.set_uex_api_key(key.filter(|k| !k.trim().is_empty()));
    state.persist(&s);
    SettingsSnapshot::from_settings(&s, crate::overlay::global_shortcuts_active(&app))
}

/// Set the primary commodity price source (wiki | uex). Persists immediately
/// AND flips the live source switch the composite provider reads on every
/// price fetch — the next price request already uses the new source.
#[tauri::command]
pub fn settings_set_commodity_source(
    state: tauri::State<SettingsState>,
    app: tauri::AppHandle,
    name: String,
) -> Result<SettingsSnapshot, String> {
    let source = CommoditySource::from_name(&name)
        .ok_or_else(|| format!("unknown commodity source: {name}"))?;
    state.set_commodity_source_live(source);
    let s = state.inner.lock().expect("settings lock");
    state.persist(&s);
    Ok(SettingsSnapshot::from_settings(
        &s,
        crate::overlay::global_shortcuts_active(&app),
    ))
}

/// K2 (t_5fe8eb4c, P12): Set the overlay font/icon scale in % — the F12-View
/// chips expose 80/90/100/110/125, the command accepts any whole percent
/// `SCALE_MIN..SCALE_MAX`. Out-of-range is rejected (clean note for the UI);
/// a hand-edited config value on the load side is instead normalized by the
/// settings crate (`norm_scale`), so the app always boots. Persists
/// immediately and returns the fresh snapshot; the frontend re-applies it
/// live (`--ui-scale` → Shell zoom), like the palette.
#[tauri::command]
pub fn settings_set_scale(
    state: tauri::State<SettingsState>,
    app: tauri::AppHandle,
    pct: u16,
) -> Result<SettingsSnapshot, String> {
    if pct < settings::SCALE_MIN || pct > settings::SCALE_MAX {
        return Err(format!("scale out of range: {pct}"));
    }
    let mut s = state.inner.lock().expect("settings lock");
    s.set_scale(pct);
    state.persist(&s);
    Ok(SettingsSnapshot::from_settings(
        &s,
        crate::overlay::global_shortcuts_active(&app),
    ))
}

/// C2a (t_6565fe8b): Overlay-Toggle-Kürzel setzen. Ablauf (bewusst in
/// dieser Reihenfolge — der OS-Pfad entscheidet, ob persistiert wird):
/// 1. rein prüfen (settings-Crate-Regeln + Doppelvergabe gegen das andere
///    Feld) — Fehler hier ändern NICHTS.
/// 2. `overlay::rebind_shortcut`: deregistriere alt → registriere neu.
///    Scheitert die Registrierung (Belegung durch eine andere App — der
///    Windows-Fall), wird der alte Wert re-registriert und der Fehler
///    sichtbar ans Settings-Feld zurückgegeben; der persistierte Wert
///    bleibt unberührt (= alter Kürzel bleibt aktiv).
/// 3. erst jetzt: persistieren + frischer Snapshot (Soll-Ist-Zeilen im UI).
#[tauri::command]
pub fn settings_set_shortcut_toggle(
    state: tauri::State<SettingsState>,
    app: tauri::AppHandle,
    combo: String,
) -> Result<SettingsSnapshot, String> {
    let peer = state
        .inner
        .lock()
        .expect("settings lock")
        .shortcut_click_through()
        .to_owned();
    let canonical = validate_shortcut_input(&combo, &peer, "Click-through")?;
    crate::overlay::rebind_shortcut(&app, crate::overlay::ShortcutKind::Toggle, &canonical)?;
    let mut s = state.inner.lock().expect("settings lock");
    s.set_shortcut_toggle(canonical);
    state.persist(&s);
    Ok(SettingsSnapshot::from_settings(
        &s,
        crate::overlay::global_shortcuts_active(&app),
    ))
}

/// C2a: Click-through-Kürzel setzen — identisches Protokoll wie
/// `settings_set_shortcut_toggle`, nur anderes Feld (und anderer Peer).
#[tauri::command]
pub fn settings_set_shortcut_click_through(
    state: tauri::State<SettingsState>,
    app: tauri::AppHandle,
    combo: String,
) -> Result<SettingsSnapshot, String> {
    let peer = state
        .inner
        .lock()
        .expect("settings lock")
        .shortcut_toggle()
        .to_owned();
    let canonical = validate_shortcut_input(&combo, &peer, "Overlay-Toggle")?;
    crate::overlay::rebind_shortcut(&app, crate::overlay::ShortcutKind::ClickThrough, &canonical)?;
    let mut s = state.inner.lock().expect("settings lock");
    s.set_shortcut_click_through(canonical);
    state.persist(&s);
    Ok(SettingsSnapshot::from_settings(
        &s,
        crate::overlay::global_shortcuts_active(&app),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression (t_9b385831, v0.3.3 P1): the snapshot's JSON keys are the
    /// frontend contract. The frontend reads `cfg.uex_api_key_set`,
    /// `cfg.commodity_source`, `cfg.detail_variant` (snake_case). A stray
    /// `rename_all = "camelCase"` made every underscore field `undefined` in
    /// the UI — API-key status stuck at "nicht gesetzt", source chips never
    /// active — while single-word fields (palette, scale) kept working.
    #[test]
    fn snapshot_json_keys_match_frontend_snake_case_contract() {
        let snap = SettingsSnapshot::from_settings(
            &Settings::with_path(std::path::PathBuf::from("/dev/null")),
            true,
        );
        let keys: Vec<String> = {
            let v = serde_json::to_value(&snap).unwrap();
            let map = v.as_object().expect("snapshot serializes as object");
            map.keys().cloned().collect()
        };
        for required in [
            "palette",
            "detail_variant",
            "uex_api_key_set",
            "commodity_source",
            "scale",
            // C2a (t_6565fe8b): dieselbe Snake-Case-Vertrag gilt für die
            // Shortcut-Felder — camelCase würde sie im UI unsichtbar machen.
            "shortcut_toggle",
            "shortcut_click_through",
            "global_shortcuts_active",
        ] {
            assert!(
                keys.iter().any(|k| k == required),
                "frontend expects key '{required}', wire has {keys:?}"
            );
        }
        // No camelCase aliases may reappear: they read as undefined in the UI.
        for forbidden in ["uexApiKeySet", "commoditySource", "detailVariant"] {
            assert!(
                !keys.iter().any(|k| k == forbidden),
                "wire key '{forbidden}' is invisible to the frontend (snake_case contract)"
            );
        }
    }

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

    /// C2a (t_6565fe8b): cross-field rule of the shortcut setter — the two
    /// fields may not end up on the same combo; settings-crate rules pass
    /// through untranslated (German, shown 1:1 in the Settings field).
    #[test]
    fn shortcut_pair_validation() {
        // ok: fresh canonical combo, peer untouched
        assert_eq!(
            validate_shortcut_input("ctrl+alt+k", "Ctrl+Shift+O", "Click-through").unwrap(),
            "Ctrl+Alt+K"
        );
        // Doppelvergabe: identical to the other field's active combo
        let err =
            validate_shortcut_input("Ctrl+Shift+O", "Ctrl+Shift+O", "Click-through").unwrap_err();
        assert!(
            err.contains("Click-through"),
            "names the colliding field: {err}"
        );
        // internal conflict from the settings crate surfaces verbatim
        let err = validate_shortcut_input("Ctrl+F2", "Ctrl+Shift+O", "Click-through").unwrap_err();
        assert!(err.contains("F2"), "conflict list names F2: {err}");
        // structurally invalid (no main key)
        assert!(validate_shortcut_input("Ctrl+Shift", "Ctrl+Shift+O", "Click-through").is_err());
    }
}
