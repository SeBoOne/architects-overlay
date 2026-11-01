//! i18n error transport (T2, t_c5904bcb).
//!
//! Rust sends UI-facing errors/notes as `{"error_key": "...", "params": {...}}`
//! JSON; the frontend `trError()` (src/lib/i18n.ts) resolves the key through
//! the locale files (selected → en → raw key). Plain-text errors (old style
//! or internal) pass through unchanged in the frontend — no gettext layer.

use serde_json::{Value, json};

/// Error with params: `err_keyp("error.uex.http", json!({ "status": 500 }))`.
pub fn err_keyp(key: &str, params: Value) -> String {
    json!({ "error_key": key, "params": params }).to_string()
}

/// Error without params: `err_key("error.update.none")`.
pub fn err_key(key: &str) -> String {
    json!({ "error_key": key }).to_string()
}

/// Wrap a lower-layer error for the UI — UNLESS it is already a key-JSON
/// message (T2): inner layers (cache, math) may emit their own keys and the
/// frontend must not see them double-wrapped. Non-JSON errors get the given
/// context key with the raw text as `err` param.
pub fn err_wrap(context_key: &str, raw: String) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(&raw) {
        if v.get("error_key").is_some() {
            return raw;
        }
    }
    err_keyp(context_key, json!({ "err": raw }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn err_key_shape() {
        let s = err_key("error.update.none");
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["error_key"], "error.update.none");
        assert!(v.get("params").is_none());
    }

    #[test]
    fn err_keyp_shape() {
        let s = err_keyp("error.uex.http", json!({ "status": 503 }));
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["error_key"], "error.uex.http");
        assert_eq!(v["params"]["status"], 503);
    }
}
