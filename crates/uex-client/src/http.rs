//! Thin async HTTP layer over the UEX API v2.0.
//!
//! Responsibilities:
//!
//! * base URL + optional Bearer token / `secret-key` header (write endpoints
//!   later — read endpoints work without auth);
//! * per-request timeout (global 30s) mapped to [`ProviderError::Timeout`] with
//!   the real HTTP status kept for non-success codes;
//! * rate-limit respect: the UEX daily quota is ~172 800 req/day (≈120/min) —
//!   we pace ourselves to `min_interval` between requests (default 300ms →
//!   ~200/min, safely under quota) and map the API's
//!   `requests_limit_reached` status to [`ProviderError::RateLimit`];
//! * envelope unwrapping: `{"status": "ok", "data": …}`.
//!
//! Error codes travel in the JSON `status` field, not in HTTP status codes —
//! see [`UexHttpClient::get`] for the mapping.

use std::time::{Duration, Instant};

use provider::{ProviderError, Result};

use crate::dto::Envelope;

/// UEX API base URL (research doc).
pub const DEFAULT_BASE_URL: &str = "https://api.uexcorp.uk/2.0";
/// Client transport timeout (connect + request).
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Minimal pause between requests: ~200 req/min, safely below the daily quota.
pub const DEFAULT_MIN_INTERVAL: Duration = Duration::from_millis(300);

/// Async HTTP client with pacing and UEX-specific error mapping.
pub struct UexHttpClient {
    client: reqwest::Client,
    base_url: String,
    /// Bearer token for app-scoped read endpoints (optional).
    api_key: Option<String>,
    /// Secret-key header for user/write endpoints (optional, unused in Phase 1).
    secret_key: Option<String>,
    /// Pacing state (last request start). std Mutex: never held across await.
    last_request: std::sync::Mutex<Instant>,
    min_interval: Duration,
}

impl UexHttpClient {
    /// Build a client against `base_url`. Both credentials optional (read
    /// endpoints work without auth; the API key is forwarded via an
    /// `Authorization` header when configured).
    pub fn new(
        base_url: &str,
        api_key: Option<String>,
        secret_key: Option<String>,
        min_interval: Duration,
    ) -> Result<UexHttpClient> {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent(concat!("architects-overlay/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| ProviderError::Config(format!("cannot build http client: {e}")))?;
        Ok(UexHttpClient {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            secret_key,
            last_request: std::sync::Mutex::new(Instant::now() - min_interval),
            min_interval,
        })
    }

    /// GET `path` with query params, unwrap the envelope, return `data`.
    pub async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        params: &[(String, String)],
    ) -> Result<T> {
        self.get_optional(path, params).await?.ok_or_else(|| {
            ProviderError::Json(format!("response status ok but `data` missing for {path}"))
        })
    }

    /// Like [`UexHttpClient::get`], but `data: null` (a category with no
    /// items, e.g.) maps to `Ok(None)` instead of an error.
    pub async fn get_optional<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        params: &[(String, String)],
    ) -> Result<Option<T>> {
        self.pace().await;

        let url = format!("{}{}", self.base_url, path);
        let mut request = self.client.get(&url).query(params);
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key);
        }
        if let Some(secret) = &self.secret_key {
            request = request.header("secret-key", secret);
        }

        let response = request.send().await.map_err(map_reqwest_error)?;
        let status = response.status();
        let text = response.text().await.map_err(map_reqwest_error)?;

        // UEX encodes errors as JSON string codes in `status`, but a non-2xx
        // HTTP status is still worth surfacing verbatim.
        if !status.is_success() {
            return Err(ProviderError::Http {
                status: status.as_u16(),
                body: text.chars().take(300).collect(),
            });
        }

        let envelope: Envelope<serde_json::Value> =
            serde_json::from_str(&text).map_err(|e| ProviderError::Json(e.to_string()))?;

        if envelope.status != "ok" {
            return match envelope.status.as_str() {
                "requests_limit_reached" => Err(ProviderError::RateLimit { retry_after: None }),
                _ => Err(ProviderError::Http {
                    status: envelope.http_code.unwrap_or(status.as_u16()),
                    body: envelope.message.clone().unwrap_or(envelope.status.clone()),
                }),
            };
        }

        match envelope.data {
            Some(v) if !v.is_null() => {
                serde_json::from_value(v).map_err(|e| ProviderError::Json(e.to_string()))
            }
            _ => Ok(None),
        }
    }

    /// Respect the pacing window: sleep until `min_interval` has passed since
    /// the previous request.
    async fn pace(&self) {
        let elapsed = {
            let mut last = self.last_request.lock().expect("pacer mutex poisoned");
            let now = Instant::now();
            let elapsed = now.duration_since(*last);
            *last = now;
            elapsed
        };
        if elapsed < self.min_interval {
            tokio::time::sleep(self.min_interval - elapsed).await;
        }
    }
}

fn map_reqwest_error(e: reqwest::Error) -> ProviderError {
    if e.is_timeout() {
        ProviderError::Timeout
    } else if e.is_connect() {
        ProviderError::Other(format!("connection failed: {e}"))
    } else {
        ProviderError::Http {
            status: e.status().map(|s| s.as_u16()).unwrap_or(0),
            body: e.to_string(),
        }
    }
}
