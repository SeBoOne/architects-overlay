//! # settings
//!
//! Persists user configuration in the OS App-Config-Dir (via `dirs`): UEX API
//! key now, hotkey/window mode later. The API key is stored **only locally** —
//! never shipped, never logged, written with 0600 permissions on Unix.
//!
//! File: `<config-dir>/architects-overlay/config.toml`
//!
//! ```toml
//! [uex]
//! api_key = "…"
//!
//! [ui]
//! palette = "amber"        # amber | mfcd | bloom
//! detail_variant = "c"     # c | a
//! scale = 100              # K2 P12: 50..200 % (Schrötwerte 80/90/100/110/125)
//!
//! [data]
//! commodity_source = "wiki" # wiki | uex  (commodity PRICE source; wiki
//!                           # first, UEX as fallback for gaps)
//! ```

use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// App sub-directory inside the OS config dir.
const APP_DIR: &str = "architects-overlay";
const CONFIG_FILE: &str = "config.toml";

/// K2 (t_5fe8eb4c, P12): Overlay font/icon scale in percent. Persisted in the
/// `ui` section; the frontend applies it as `zoom: var(--ui-scale)` on the
/// Shell. The UI offers 80/90/100/110/125 chips, the backend accepts any
/// value inside [`SCALE_MIN`..`SCALE_MAX`] — hand-edited out-of-range values
/// are normalized (`norm_scale`), never rejected, so the overlay boots.
pub const SCALE_DEFAULT: u16 = 100;
pub const SCALE_MIN: u16 = 50;
pub const SCALE_MAX: u16 = 200;

pub fn norm_scale(pct: u16) -> u16 {
    pct.clamp(SCALE_MIN, SCALE_MAX)
}

/// Colour palette of the overlay UI. Rendered as `data-palette` on `<html>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Palette {
    #[default]
    Amber,
    Mfcd,
    Bloom,
}

impl Palette {
    pub fn as_str(&self) -> &'static str {
        match self {
            Palette::Amber => "amber",
            Palette::Mfcd => "mfcd",
            Palette::Bloom => "bloom",
        }
    }

    pub fn from_name(name: &str) -> Option<Palette> {
        match name.trim().to_ascii_lowercase().as_str() {
            "amber" => Some(Palette::Amber),
            "mfcd" => Some(Palette::Mfcd),
            "bloom" => Some(Palette::Bloom),
            _ => None,
        }
    }
}

impl fmt::Display for Palette {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Price-detail layout variant in the search result view.
/// `C` = classic/compact default, `A` = alternative rendering (owned by the
/// parallel frontend task; this crate only carries the switch + CSS scope).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DetailVariant {
    #[default]
    C,
    A,
}

impl DetailVariant {
    pub fn as_str(&self) -> &'static str {
        match self {
            DetailVariant::C => "c",
            DetailVariant::A => "a",
        }
    }

    pub fn from_name(name: &str) -> Option<DetailVariant> {
        match name.trim().to_ascii_lowercase().as_str() {
            "c" => Some(DetailVariant::C),
            "a" => Some(DetailVariant::A),
            _ => None,
        }
    }
}

impl fmt::Display for DetailVariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Primary commodity PRICE source. `Wiki` (default): the wiki commodity
/// detail endpoint embeds the UEX terminal price table (`uex_prices.purchase`)
/// and is fetched first; UEX remains the fallback when the wiki row carries
/// no prices or the fetch fails. `Uex` reverts to the pure-UEX path (A/B
/// comparison switch).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CommoditySource {
    #[default]
    Wiki,
    Uex,
}

impl CommoditySource {
    pub fn as_str(&self) -> &'static str {
        match self {
            CommoditySource::Wiki => "wiki",
            CommoditySource::Uex => "uex",
        }
    }

    pub fn from_name(name: &str) -> Option<CommoditySource> {
        match name.trim().to_ascii_lowercase().as_str() {
            "wiki" => Some(CommoditySource::Wiki),
            "uex" => Some(CommoditySource::Uex),
            _ => None,
        }
    }
}

impl fmt::Display for CommoditySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RawConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    uex: Option<UexSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ui: Option<UiSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data: Option<DataSection>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct UexSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    api_key: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct DataSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    commodity_source: Option<CommoditySource>,
}

/// UI preferences (palette, detail variant). Fields are optional in the file
/// so older configs stay valid; missing values fall back to the enum default.
#[derive(Debug, Default, Serialize, Deserialize)]
struct UiSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    palette: Option<Palette>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    detail_variant: Option<DetailVariant>,
    // K2 (t_5fe8eb4c, P12): Schrift-/Icon-Skalierung in Prozent (F12-View).
    // Normalized via `norm_scale` on load so a hand-edited out-of-range value
    // cannot break the layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scale: Option<u16>,
}

/// Settings I/O failures.
#[derive(Debug)]
pub enum SettingsError {
    /// The OS config directory could not be resolved.
    NoConfigDir,
    /// Reading or writing the config file failed.
    Io(String),
    /// The config file exists but is not valid TOML/schema.
    Parse(String),
}

impl std::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsError::NoConfigDir => write!(f, "config directory not available"),
            SettingsError::Io(msg) => write!(f, "config file I/O error: {msg}"),
            SettingsError::Parse(msg) => write!(f, "invalid config file: {msg}"),
        }
    }
}

impl std::error::Error for SettingsError {}

pub type Result<T> = std::result::Result<T, SettingsError>;

/// The application settings. Load once, mutate, save.
#[derive(Debug)]
pub struct Settings {
    path: PathBuf,
    uex_api_key: Option<String>,
    palette: Palette,
    detail_variant: DetailVariant,
    commodity_source: CommoditySource,
    scale: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            path: PathBuf::from(CONFIG_FILE),
            uex_api_key: None,
            palette: Palette::default(),
            detail_variant: DetailVariant::default(),
            commodity_source: CommoditySource::default(),
            scale: SCALE_DEFAULT,
        }
    }
}

impl Settings {
    /// Path to the config file in the OS App-Config-Dir, resolved via `dirs`.
    /// May be explicitly injected (tests) via [`Settings::with_path`].
    pub fn config_path() -> Result<PathBuf> {
        let base = dirs::config_dir().ok_or(SettingsError::NoConfigDir)?;
        Ok(base.join(APP_DIR).join(CONFIG_FILE))
    }

    /// Load settings from the standard config path; missing file → defaults.
    pub fn load() -> Result<Settings> {
        let path = Self::config_path()?;
        Self::load_from(&path)
    }

    /// Load from an explicit path (missing file → defaults).
    pub fn load_from(path: &std::path::Path) -> Result<Settings> {
        let raw = match std::fs::read_to_string(path) {
            Ok(content) => toml::from_str::<RawConfig>(&content)
                .map_err(|e| SettingsError::Parse(e.to_string()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => RawConfig::default(),
            Err(e) => return Err(SettingsError::Io(e.to_string())),
        };
        Ok(Settings {
            path: path.to_path_buf(),
            uex_api_key: raw.uex.and_then(|u| u.api_key),
            palette: raw.ui.as_ref().and_then(|u| u.palette).unwrap_or_default(),
            detail_variant: raw
                .ui
                .as_ref()
                .and_then(|u| u.detail_variant)
                .unwrap_or_default(),
            commodity_source: raw
                .data
                .as_ref()
                .and_then(|d| d.commodity_source)
                .unwrap_or_default(),
            scale: raw
                .ui
                .as_ref()
                .and_then(|u| u.scale)
                .map(norm_scale)
                .unwrap_or(SCALE_DEFAULT),
        })
    }

    /// Settings handle pinned to a given file path (tests/embedded use).
    pub fn with_path(path: PathBuf) -> Settings {
        Settings {
            path,
            uex_api_key: None,
            palette: Palette::default(),
            detail_variant: DetailVariant::default(),
            commodity_source: CommoditySource::default(),
            scale: SCALE_DEFAULT,
        }
    }

    /// Configured UEX API key, or None.
    pub fn uex_api_key(&self) -> Option<&str> {
        self.uex_api_key.as_deref()
    }

    /// Set the UEX API key in memory (persist via [`Settings::save`]).
    pub fn set_uex_api_key(&mut self, key: Option<String>) {
        self.uex_api_key = key;
    }

    /// Active colour palette (default: Amber).
    pub fn palette(&self) -> Palette {
        self.palette
    }

    /// Set the colour palette in memory (persist via [`Settings::save`]).
    pub fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
    }

    /// Active price-detail variant (default: C).
    pub fn detail_variant(&self) -> DetailVariant {
        self.detail_variant
    }

    /// Set the price-detail variant in memory (persist via [`Settings::save`]).
    pub fn set_detail_variant(&mut self, variant: DetailVariant) {
        self.detail_variant = variant;
    }

    /// Primary commodity price source (default: Wiki — UEX fallback keeps
    /// gaps covered).
    pub fn commodity_source(&self) -> CommoditySource {
        self.commodity_source
    }

    /// Set the commodity price source in memory (persist via [`Settings::save`]).
    pub fn set_commodity_source(&mut self, source: CommoditySource) {
        self.commodity_source = source;
    }

    /// Overlay font/icon scale in % (K2 P12; default 100).
    pub fn scale(&self) -> u16 {
        self.scale
    }

    /// Set the font scale in memory; out-of-range values are normalized,
    /// never rejected (persist via [`Settings::save`]).
    pub fn set_scale(&mut self, pct: u16) {
        self.scale = norm_scale(pct);
    }

    /// Write the config file atomically (temp file + rename). On Unix the
    /// file is created with 0600 permissions so the API key stays user-only.
    pub fn save(&self) -> Result<()> {
        let raw = RawConfig {
            uex: Some(UexSection {
                api_key: self.uex_api_key.clone(),
            }),
            ui: Some(UiSection {
                palette: Some(self.palette),
                detail_variant: Some(self.detail_variant),
                scale: Some(self.scale),
            }),
            data: Some(DataSection {
                commodity_source: Some(self.commodity_source),
            }),
        };
        let content = toml::to_string_pretty(&raw)
            .map_err(|e| SettingsError::Parse(format!("serialize: {e}")))?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| SettingsError::Io(e.to_string()))?;
        }
        let tmp = self.path.with_extension("toml.tmp");
        std::fs::write(&tmp, content).map_err(|e| SettingsError::Io(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&tmp, perms).map_err(|e| SettingsError::Io(e.to_string()))?;
        }
        std::fs::rename(&tmp, &self.path).map_err(|e| SettingsError::Io(e.to_string()))?;
        Ok(())
    }
}

/// Path to the per-user config file (App-Config-Dir / architects-overlay/config.toml).
pub fn config_path() -> Option<String> {
    Settings::config_path()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

/// Returns the configured UEX API key, or None if unset.
pub fn uex_api_key() -> Option<String> {
    Settings::load().ok()?.uex_api_key().map(str::to_owned)
}

/// Persist the UEX API key.
pub fn set_uex_api_key(key: &str) {
    let mut settings = Settings::load().unwrap_or_else(|_| Settings::default());
    settings.path = Settings::config_path().unwrap_or_else(|_| PathBuf::from("config.toml"));
    settings.set_uex_api_key(Some(key.to_string()));
    if let Err(e) = settings.save() {
        log::warn!("settings: could not persist UEX API key: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(tmp: &tempfile::TempDir, name: &str) -> PathBuf {
        tmp.path().join("nested").join(name)
    }

    #[test]
    fn missing_file_yields_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let settings = Settings::load_from(&tmp_path(&tmp, "config.toml")).unwrap();
        assert_eq!(settings.uex_api_key(), None);
        assert_eq!(settings.palette(), Palette::Amber);
        assert_eq!(settings.detail_variant(), DetailVariant::C);
        assert!(!settings.path.exists());
    }

    #[test]
    fn palette_and_variant_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp_path(&tmp, "config.toml");

        let mut s = Settings::with_path(path.clone());
        s.set_palette(Palette::Bloom);
        s.set_detail_variant(DetailVariant::A);
        s.save().unwrap();

        let loaded = Settings::load_from(&path).unwrap();
        assert_eq!(loaded.palette(), Palette::Bloom);
        assert_eq!(loaded.detail_variant(), DetailVariant::A);
        assert_eq!(loaded.uex_api_key(), None);

        // Switch back to the defaults and verify persistence again.
        let mut s2 = Settings::load_from(&path).unwrap();
        s2.set_palette(Palette::Mfcd);
        s2.set_detail_variant(DetailVariant::C);
        s2.save().unwrap();
        let loaded2 = Settings::load_from(&path).unwrap();
        assert_eq!(loaded2.palette(), Palette::Mfcd);
        assert_eq!(loaded2.detail_variant(), DetailVariant::C);
    }

    #[test]
    fn scale_roundtrip_and_clamp() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp_path(&tmp, "config.toml");

        // Default without a scale entry: 100 %.
        let mut s = Settings::with_path(path.clone());
        assert_eq!(s.scale(), SCALE_DEFAULT);
        s.set_scale(125);
        s.save().unwrap();
        let loaded = Settings::load_from(&path).unwrap();
        assert_eq!(loaded.scale(), 125);

        // Hand-edited out-of-range value: normalized on load, never rejected.
        std::fs::write(
            &path,
            "[ui]\npalette = \"amber\"\ndetail_variant = \"c\"\nscale = 9000\n",
        )
        .unwrap();
        let clamped = Settings::load_from(&path).unwrap();
        assert_eq!(clamped.scale(), SCALE_MAX);

        let mut s2 = Settings::load_from(&path).unwrap();
        s2.set_scale(1);
        assert_eq!(s2.scale(), SCALE_MIN);
        s2.save().unwrap();
        assert_eq!(Settings::load_from(&path).unwrap().scale(), SCALE_MIN);
    }

    #[test]
    fn legacy_config_without_ui_section_uses_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "[uex]\napi_key = \"old-key\"\n").unwrap();
        let loaded = Settings::load_from(&path).unwrap();
        assert_eq!(loaded.uex_api_key(), Some("old-key"));
        assert_eq!(loaded.palette(), Palette::Amber);
        assert_eq!(loaded.detail_variant(), DetailVariant::C);
        assert_eq!(loaded.commodity_source(), CommoditySource::Wiki);
    }

    #[test]
    fn commodity_source_roundtrip_and_parsing() {
        assert_eq!(
            CommoditySource::from_name("wiki"),
            Some(CommoditySource::Wiki)
        );
        assert_eq!(
            CommoditySource::from_name("UEX"),
            Some(CommoditySource::Uex)
        );
        assert_eq!(CommoditySource::from_name("both"), None);

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        let mut s = Settings::with_path(path.clone());
        s.set_commodity_source(CommoditySource::Uex);
        s.save().unwrap();
        let loaded = Settings::load_from(&path).unwrap();
        assert_eq!(loaded.commodity_source(), CommoditySource::Uex);
        // Default stays wiki.
        let mut s2 = Settings::with_path(path.clone());
        s2.set_commodity_source(CommoditySource::Wiki);
        s2.save().unwrap();
        assert_eq!(
            Settings::load_from(&path).unwrap().commodity_source(),
            CommoditySource::Wiki
        );
    }

    #[test]
    fn palette_name_parsing() {
        assert_eq!(Palette::from_name("amber"), Some(Palette::Amber));
        assert_eq!(Palette::from_name("MFCD"), Some(Palette::Mfcd));
        assert_eq!(Palette::from_name(" bloom "), Some(Palette::Bloom));
        assert_eq!(Palette::from_name("pink"), None);
        assert_eq!(DetailVariant::from_name("A"), Some(DetailVariant::A));
        assert_eq!(DetailVariant::from_name("x"), None);
    }

    #[test]
    fn roundtrip_persists_key() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp_path(&tmp, "config.toml");

        let mut s = Settings::with_path(path.clone());
        s.set_uex_api_key(Some(String::from("secret-abc-123")));
        s.save().unwrap();
        assert!(path.exists());

        let loaded = Settings::load_from(&path).unwrap();
        assert_eq!(loaded.uex_api_key(), Some("secret-abc-123"));

        // overwrite wipes the old value
        let mut s2 = Settings::with_path(path.clone());
        s2.set_uex_api_key(None);
        s2.save().unwrap();
        assert_eq!(Settings::load_from(&path).unwrap().uex_api_key(), None);
    }

    #[test]
    fn invalid_toml_is_parse_error() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "not [ valid toml ===").unwrap();
        let err = Settings::load_from(&path).unwrap_err();
        assert!(matches!(err, SettingsError::Parse(_)));
    }

    #[cfg(unix)]
    #[test]
    fn key_file_is_user_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        let mut s = Settings::with_path(path.clone());
        s.set_uex_api_key(Some(String::from("topsecret")));
        s.save().unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "config file must be 0600, got {:o}",
            mode & 0o777
        );
    }
}
