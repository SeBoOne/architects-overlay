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
//! ```

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// App sub-directory inside the OS config dir.
const APP_DIR: &str = "architects-overlay";
const CONFIG_FILE: &str = "config.toml";

#[derive(Debug, Default, Serialize, Deserialize)]
struct RawConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    uex: Option<UexSection>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct UexSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    api_key: Option<String>,
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
#[derive(Debug, Default)]
pub struct Settings {
    path: PathBuf,
    uex_api_key: Option<String>,
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
        Ok(Settings { path: path.to_path_buf(), uex_api_key: raw.uex.and_then(|u| u.api_key) })
    }

    /// Settings handle pinned to a given file path (tests/embedded use).
    pub fn with_path(path: PathBuf) -> Settings {
        Settings { path, uex_api_key: None }
    }

    /// Configured UEX API key, or None.
    pub fn uex_api_key(&self) -> Option<&str> {
        self.uex_api_key.as_deref()
    }

    /// Set the UEX API key in memory (persist via [`Settings::save`]).
    pub fn set_uex_api_key(&mut self, key: Option<String>) {
        self.uex_api_key = key;
    }

    /// Write the config file atomically (temp file + rename). On Unix the
    /// file is created with 0600 permissions so the API key stays user-only.
    pub fn save(&self) -> Result<()> {
        let raw = RawConfig {
            uex: Some(UexSection { api_key: self.uex_api_key.clone() }),
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
            std::fs::set_permissions(&tmp, perms)
                .map_err(|e| SettingsError::Io(e.to_string()))?;
        }
        std::fs::rename(&tmp, &self.path).map_err(|e| SettingsError::Io(e.to_string()))?;
        Ok(())
    }
}

/// Path to the per-user config file (App-Config-Dir / architects-overlay/config.toml).
pub fn config_path() -> Option<String> {
    Settings::config_path().ok().map(|p| p.to_string_lossy().into_owned())
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
        assert!(!settings.path.exists());
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
        assert_eq!(mode & 0o777, 0o600, "config file must be 0600, got {:o}", mode & 0o777);
    }
}
