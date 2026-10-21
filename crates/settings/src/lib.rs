//! # settings
//!
//! Persists user configuration in the OS App-Config-Dir (via `dirs`): UEX API
//! key, hotkey, last window mode. The API key is stored **only locally**, never
//! shipped or logged. Stub — real config file lands with the UEX-Client task.

/// Path to the per-user config file (App-Config-Dir / architects-overlay.toml).
pub fn config_path() -> Option<String> {
    // TODO(settings): resolve via `dirs::config_dir()`, append app dir + file.
    None
}

/// Returns the configured UEX API key, or None if unset.
pub fn uex_api_key() -> Option<String> {
    // TODO(settings): read key from the config file.
    None
}

/// Persist the UEX API key.
pub fn set_uex_api_key(_key: &str) {
    // TODO(settings): write key to the config file.
}
