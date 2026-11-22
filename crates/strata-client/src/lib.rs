//! # strata_client
//!
//! Client für die Strata Public API (strata.celd.space) — Mining-Daten
//! (M7-K1, t_e38faa51). Analog `uex-client` aufgebaut, aber schlichter: die
//! API hat KEINEN Status-Envelope, schreibt aber `Bearer <celd_…>` zwingend
//! vor und verlangt Pflicht-Attribution bei JEGLICHER öffentlichen Anzeige
//! (die Attribution wird deshalb aus JEDEM Response durchgereicht — die UI
//! zeigt sie als attribution-footer, K2).
//!
//! Behavior contract:
//!
//! * **Cache-First:** alle vier Reads gehen über den SQLite-`strata_meta`-
//!   KV (TTL: ores/locations 24 h — Game-Patch-Zyklus, Detaildaten 6 h).
//!   Frisch → kein Netz-Request (120/60s-Quote ist damit nie ein Thema);
//!   abgelaufen → Live-Fetch + Upsert. Cache-Fehler (SQLite) werden als
//!   Warnung ignoriert und der Live-Fetch gestartet — Daten schlagen Cache.
//! * **Kein Shared Key:** der `celd_`-Key kommt pro Nutzer aus dem Keystore
//!   ([`keystore`], connector_keystore-Muster), die Settings halten nur die
//!   `key_ref`. ToS: Revocation folgt sonst.
//! * **429:** `Retry-After` wird respektiert
//!   ([`ProviderError::RateLimit`]); die Command-Schicht mappt auf
//!   `error.strata.*`-i18n-Keys ([`error_key`]).
//! * **Null-Disziplin:** `null`-Felder (`tier`/`scanSignature`/
//!   `instability`/`resistance`) sind `Option` — 0.0 ist gültige Daten,
//!   null-check NIEMALS truthiness (DTO-Tests decken das ab).

mod dto;
mod http;
pub mod keystore;

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cache::{Cache, TTL_STRATA_CRAFT, TTL_STRATA_DETAIL, TTL_STRATA_REFERENCE};
use http::{DEFAULT_BASE_URL, StrataHttpClient};
use i18n_err::{err_key, err_keyp};
use provider::{ProviderError, Result};
use serde_json::json;

pub use dto::{
    CRAFT_GEMS_PER_SCU, CraftCostEntry, CraftCostResponse, CraftItemRef, CraftUnit, Item,
    ItemsResponse, Location, LocationOresResponse, LocationRef, LocationRow, LocationsResponse,
    Ore, OreLocationGroups, OreLocationsResponse, OreRef, OreRow, OresResponse,
};

/// Sortierung von `ore-locations/{ore}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OreSort {
    /// Nach Gesamt-Vorkommen (rankPct absteigend) — Default der API.
    Rank,
    /// Nach Reinheits-Anteil (concentration absteigend).
    Concentration,
}

impl OreSort {
    pub fn as_str(&self) -> &'static str {
        match self {
            OreSort::Rank => "rank",
            OreSort::Concentration => "concentration",
        }
    }
}

/// Pflicht-Attribution — Fallback, wenn eine Response (widrigenfalls) kein
/// `attribution`-Feld trägt; die UI zeigt IMMER einen Attribution-String.
pub const ATTRIBUTION_FALLBACK: &str = "Data from celd.space / Strata Mining Tools";

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Concrete client for the Strata source.
pub struct StrataClient {
    http: StrataHttpClient,
    cache: Arc<Cache>,
}

impl StrataClient {
    /// Build with the shared cache and a `celd_…` key (Pflicht — leerer Key
    /// ist ein Config-Fehler; Keys leben im [`keystore`], nicht hier).
    pub fn new(cache: Arc<Cache>, api_key: &str) -> Result<StrataClient> {
        Ok(StrataClient {
            http: StrataHttpClient::new(DEFAULT_BASE_URL, api_key)?,
            cache,
        })
    }

    /// Build against an explicit base URL (Tests/Probes) with its own
    /// in-memory cache. Production nutzt [`StrataClient::new`] (Shared-Cache).
    pub fn for_base_url(base_url: &str, api_key: &str) -> Result<StrataClient> {
        let cache = Arc::new(
            cache::Cache::open_in_memory().map_err(|e| ProviderError::Cache(e.to_string()))?,
        );
        Ok(StrataClient {
            http: StrataHttpClient::new(base_url, api_key)?,
            cache,
        })
    }

    /// Convenience: Key aus dem Keystore auflösen (Settings halten nur die
    /// `key_ref`). Fehler als Config — die UI erklärt dann den Self-Service
    /// (strata.celd.space/api-keys).
    pub fn resolve_key(keystore_path: &std::path::Path, key_ref: &str) -> Result<String> {
        keystore::resolve_key(keystore_path, key_ref).ok_or_else(|| {
            ProviderError::Config(format!(
                "strata: no API key for ref '{key_ref}' in {} (get one at \
                 strata.celd.space/api-keys)",
                keystore_path.display()
            ))
        })
    }

    /// `GET /api/public/ores` — cache-first, TTL [`TTL_STRATA_REFERENCE`].
    pub async fn ores(&self) -> Result<OresResponse> {
        self.fetch_cached("ores", TTL_STRATA_REFERENCE, "/api/public/ores", &[])
            .await
    }

    /// `GET /api/public/locations` — cache-first, TTL [`TTL_STRATA_REFERENCE`].
    pub async fn locations(&self) -> Result<LocationsResponse> {
        self.fetch_cached(
            "locations",
            TTL_STRATA_REFERENCE,
            "/api/public/locations",
            &[],
        )
        .await
    }

    /// `GET /api/public/ore-locations/{ore}?sort=…` — cache-first, TTL
    /// [`TTL_STRATA_DETAIL`]. Cache-Key enthält die Sortierung.
    pub async fn ore_locations(&self, ore: &str, sort: OreSort) -> Result<OreLocationsResponse> {
        let path = format!("/api/public/ore-locations/{}", urlenc(ore));
        let params = vec![("sort".to_owned(), sort.as_str().to_owned())];
        self.fetch_cached(
            &format!("ore-locations:{}:{}", ore.trim(), sort.as_str()),
            TTL_STRATA_DETAIL,
            &path,
            &params,
        )
        .await
    }

    /// `GET /api/public/location-ores/{location}` — cache-first, TTL
    /// [`TTL_STRATA_DETAIL`].
    pub async fn location_ores(&self, location: &str) -> Result<LocationOresResponse> {
        let path = format!("/api/public/location-ores/{}", urlenc(location));
        self.fetch_cached(
            &format!("location-ores:{}", location.trim()),
            TTL_STRATA_DETAIL,
            &path,
            &[],
        )
        .await
    }

    /// `GET /api/public/items` — cache-first, TTL [`TTL_STRATA_CRAFT`]
    /// (Patch-zyklisch, wie alle Crafting-Daten; M8-K1).
    pub async fn items(&self) -> Result<ItemsResponse> {
        self.fetch_cached("items", TTL_STRATA_CRAFT, "/api/public/items", &[])
            .await
    }

    /// `GET /api/public/craft-cost/{item}` — cache-first, TTL
    /// [`TTL_STRATA_CRAFT`]. `{item}` ist die UUID (**id vor Slug** —
    /// kanonischer Key; die API nimmt auch Slugs, aber die UI reicht
    /// konsequent `Item.id` durch). Cache-Key enthält das Item.
    pub async fn craft_cost(&self, item: &str) -> Result<CraftCostResponse> {
        let item = item.trim();
        let path = format!("/api/public/craft-cost/{}", urlenc(item));
        self.fetch_cached(
            &format!("craft-cost:{}", item),
            TTL_STRATA_CRAFT,
            &path,
            &[],
        )
        .await
    }

    /// Shared cache-first Read: frisch → geparst aus `strata_meta`, sonst
    /// Live-Fetch + Upsert. Kaputter Cache (SQLite) = Warnung + Live-Fetch.
    async fn fetch_cached<T>(
        &self,
        key: &str,
        ttl: Duration,
        path: &str,
        params: &[(String, String)],
    ) -> Result<T>
    where
        T: serde::de::DeserializeOwned + serde::Serialize,
    {
        // M9-Q: Cache-Hit ohne Altersgrenze — bei Live-Fetch-Fehler (API down,
        // 530/Timeout) dient der ABGELAUFENE Cache-Wert als Stale-Fallback.
        // SCMDB-Daten ändern sich nur pro Game-Patch, stale ist da besser als
        // ein harter Fehler. Freshest-First: frischer Cache wird bevorzugt.
        let cached: Option<(u64, String)> = self
            .cache
            .strata_meta_get(key)
            .map_err(|e| log::warn!("strata: cache read {key}: {e}"))
            .ok()
            .flatten();
        if let Some((fetched_at, payload)) = &cached
            && now_unix().saturating_sub(*fetched_at) < ttl.as_secs()
        {
            return serde_json::from_str(payload)
                .map_err(|e| ProviderError::Json(format!("cache {key}: {e}")));
        }

        let value: T = match self.http.get(path, params).await {
            Ok(v) => v,
            Err(e) => {
                // Stale-Fallback: abgelaufener Cache schlägt API-Fehler.
                if let Some((_, payload)) = &cached {
                    log::warn!("strata: live fetch {key} failed ({e}) — serving stale cache");
                    return serde_json::from_str(payload)
                        .map_err(|e| ProviderError::Json(format!("stale cache {key}: {e}")));
                }
                return Err(e);
            }
        };
        match serde_json::to_string(&value) {
            Ok(payload) => {
                if let Err(e) = self.cache.strata_meta_set(key, now_unix(), &payload) {
                    log::warn!("strata: cache write {key}: {e}");
                }
            }
            Err(e) => log::warn!("strata: cache serialize {key}: {e}"),
        }
        Ok(value)
    }
}

/// Path-Segment-Encoding: IDs/Slugs sind `a-z0-9_`-artig, aber defensiv
/// percent-encode-würdige Zeichen verweigern wir statt encodieren — die
/// Spec-Kataloge zeigen nur einfache Bezeichner.
fn urlenc(seg: &str) -> &str {
    let seg = seg.trim();
    debug_assert!(
        !seg.is_empty()
            && seg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
        "unexpected strata path segment: {seg}"
    );
    seg
}

/// Mappt einen ProviderError auf den UI-Fehler als i18n-err-Key-JSON
/// (`{"error_key": …, "params": …}`; T2-Transport, i18n-err-Crate). Die
/// Frontend-Fallback-Kette (selected → en → raw key) resolviert die
/// `error.strata.*`-Keys aus den Locale-Dateien.
pub fn error_key(e: &ProviderError) -> String {
    match e {
        ProviderError::RateLimit { retry_after } => err_keyp(
            "error.strata.rate_limit",
            json!({ "retry_after": retry_after.unwrap_or(60) }),
        ),
        ProviderError::Http { status, .. } => {
            err_keyp("error.strata.http", json!({ "status": status }))
        }
        ProviderError::Timeout => err_key("error.strata.timeout"),
        ProviderError::Json(msg) => err_keyp("error.strata.bad_response", json!({ "msg": msg })),
        ProviderError::Config(msg) => err_keyp("error.strata.config", json!({ "msg": msg })),
        // Keyed = bereits ein UI-Fehler-Key — 1:1 weiterreichen (kein
        // doppeltes Wrappen, i18n-err::err_wrap-Vertrag).
        ProviderError::Keyed { key, params } => err_keyp(key, params.clone()),
        // Cache-Fehler entstehen lokal — keep the same contract as trade.rs.
        ProviderError::Cache(msg) => err_keyp("error.strata.cache", json!({ "msg": msg })),
        ProviderError::NotFound(what) => {
            err_keyp("error.strata.not_found", json!({ "what": what }))
        }
        ProviderError::Unsupported(msg) | ProviderError::Other(msg) => {
            err_keyp("error.strata.other", json!({ "msg": msg }))
        }
        // non_exhaustive: künftige Varianten auf den Catch-all-Key.
        _ => err_key("error.strata.other"),
    }
}

#[cfg(test)]
mod stale_fallback_tests {
    //! M9-Q: Stale-While-Error — abgelaufener Cache schlägt Live-Fetch-Fehler.
    use super::*;
    use std::sync::Arc;

    fn client_on_dead_port() -> StrataClient {
        StrataClient {
            http: StrataHttpClient::new("http://127.0.0.1:1", "celd_test").unwrap(),
            cache: Arc::new(Cache::open_in_memory().unwrap()),
        }
    }

    #[tokio::test]
    async fn stale_cache_served_when_live_fetch_fails() {
        let client = client_on_dead_port();
        // Abgelaufener Cache-Eintrag (fetched_at weit in der Vergangenheit).
        let payload = r#"{"ore":{"id":"x","name":"TestOre"},"sort":"rank",
            "results":{"surface":[]},"attribution":"test"}"#;
        let old = now_unix() - 10_000_000;
        client
            .cache
            .strata_meta_set("ore-locations:TEST:rank", old, payload)
            .unwrap();
        // Live-Fetch auf Port 1 schlägt fehl → stale Cache servieren.
        let res = client.ore_locations("TEST", OreSort::Rank).await.unwrap();
        assert_eq!(res.ore.name, "TestOre");
    }

    #[tokio::test]
    async fn error_propagates_without_cache() {
        let client = client_on_dead_port();
        // Kein Cache-Eintrag → Live-Fetch-Fehler propagiert (kein Fake-Daten).
        let res = client.ore_locations("TEST", OreSort::Rank).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn fresh_cache_beats_live_fetch() {
        let client = client_on_dead_port();
        // Frischer Cache-Eintrag → kein Live-Fetch nötig (Port 1 wäre tot).
        let payload = r#"{"ore":{"id":"x","name":"FreshOre"},"sort":"rank",
            "results":{},"attribution":"test"}"#;
        client
            .cache
            .strata_meta_set("ore-locations:FRESH:rank", now_unix(), payload)
            .unwrap();
        let res = client.ore_locations("FRESH", OreSort::Rank).await.unwrap();
        assert_eq!(res.ore.name, "FreshOre");
    }
}
