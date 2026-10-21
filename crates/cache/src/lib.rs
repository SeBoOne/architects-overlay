//! # cache
//!
//! SQLite-backed cache (rusqlite) for everything the providers fetch: entities,
//! price points, marketplace offers. Each kind has its own TTL (reference data
//! long-lived, prices short-lived). Stub — schema + sync landing with the
//! UEX-Client task.

/// One cache entry row; concrete schema in the UEX-Client task.
pub struct CacheEntry {
    /// Cache-kind key, e.g. b"commodities".
    pub kind: String,
    /// JSON payload of the provider response.
    pub payload: Vec<u8>,
    /// Unix seconds at which this entry is considered stale.
    pub expires: u64,
}

/// Placeholder handle so the app crate has a typing seam to wire against.
pub struct Cache;

impl Cache {
    /// Resolve a key; returns a hit if present and not expired.
    pub fn get(&self, _key: &[u8]) -> Option<CacheEntry> {
        // TODO(cache): open DB via rusqlite, SELECT by kind + expires.
        None
    }

    /// Store a payload under a kind with the given TTL.
    pub fn put(&self, _key: &[u8], _entry: &CacheEntry) {
        // TODO(cache): INSERT OR REPLACE.
    }
}