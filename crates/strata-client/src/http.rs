//! Thin async HTTP layer over the Strata Public API (strata.celd.space).
//!
//! * EVERY endpoint requires `Authorization: Bearer <celd_…>` — ein fehlender
//!   Key ist ein [`ProviderError::Config`];
//! * 429 → [`ProviderError::RateLimit`] mit dem `Retry-After`-Header (integer
//!   Sekunden) respektiert — der Caller wartet ODER zeigt den err-Key;
//! * kein Envelope (anders als UEX): die Response IST das Objekt;
//! * 120 req/60s Quote: der Client selbst paced bewusst NICHT (alle Strata-
//!   Reads laufen über den TTL-Cache — 1 Request pro TTL-Fenster), dennoch
//!   wird 429 korrekt gemappt.

use std::time::Duration;

use provider::{ProviderError, Result};

/// Strata API base URL (Spec).
pub const DEFAULT_BASE_URL: &str = "https://strata.celd.space";
/// Client transport timeout (connect + request).
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Async HTTP client with the mandatory Bearer key and 429 mapping.
pub struct StrataHttpClient {
    client: reqwest::Client,
    base_url: String,
    /// `celd_…`-Key — Pflicht für JEDEN Endpoint (kein Anonymous-Read).
    api_key: String,
}

impl StrataHttpClient {
    pub fn new(base_url: &str, api_key: &str) -> Result<StrataHttpClient> {
        let api_key = api_key.trim();
        if api_key.is_empty() {
            return Err(ProviderError::Config(
                "strata: empty API key (celd_… required)".to_owned(),
            ));
        }
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent(concat!("architects-overlay/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| ProviderError::Config(format!("cannot build http client: {e}")))?;
        Ok(StrataHttpClient {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_owned(),
        })
    }

    /// GET `path` with query params, deserialise the raw JSON body.
    pub async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        params: &[(String, String)],
    ) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let request = self
            .client
            .get(&url)
            .query(params)
            .bearer_auth(&self.api_key);

        let response = request.send().await.map_err(map_reqwest_error)?;
        let status = response.status();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok());
        let text = response.text().await.map_err(map_reqwest_error)?;

        if status.as_u16() == 429 {
            return Err(ProviderError::RateLimit { retry_after });
        }
        if !status.is_success() {
            return Err(ProviderError::Http {
                status: status.as_u16(),
                body: text.chars().take(300).collect(),
            });
        }

        serde_json::from_str(&text).map_err(|e| ProviderError::Json(e.to_string()))
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
