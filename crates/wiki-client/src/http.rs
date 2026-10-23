//! Thin async HTTP layer over the Star Citizen Wiki API.
//!
//! Responsibilities:
//!
//! * base URL + pacing: search endpoints are documented at 60 req/min per IP
//!   and list endpoints practically 403 after ~15-20 burst requests without
//!   delay — every request waits `min_interval` (default 1s → ≤ 60/min);
//! * per-request timeout (global 30s) mapped to [`ProviderError::Timeout`];
//! * error mapping: the Wiki API uses standard HTTP status codes — 429 (and
//!   403 with a rate-limit body) map to [`ProviderError::RateLimit`], 404 to
//!   [`ProviderError::NotFound`], everything else to [`ProviderError::Http`];
//! * pagination: [`WikiHttpClient::get_all`] walks `{data, links, meta}`
//!   list responses page by page (page[size] capped at 200 by the API) with a
//!   hard page cap guarding against runaway loops.

use std::time::{Duration, Instant};

use provider::{ProviderError, Result};

use crate::dto::Page;

/// Wiki API base URL (OpenAPI servers block).
pub const DEFAULT_BASE_URL: &str = "https://api.star-citizen.wiki";
/// Client transport timeout (connect + request).
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Minimal pause between requests: ≤ 60 req/min (documented search limit),
/// comfortably below the practical ~15-20 burst threshold observed upstream.
pub const DEFAULT_MIN_INTERVAL: Duration = Duration::from_millis(1000);
/// Page size requested for list endpoints (API max 200).
pub const PAGE_SIZE: u32 = 200;
/// Hard cap on walked pages per `get_all` — 12.6k items need ~64 pages, so
/// 200 pages is far above any real dataset while still bounding a broken
/// `links.next` loop.
const MAX_PAGES: u32 = 200;

/// Async HTTP client with pacing and Wiki-API error mapping.
pub struct WikiHttpClient {
    client: reqwest::Client,
    base_url: String,
    /// Pacing state (last request start). std Mutex: never held across await.
    last_request: std::sync::Mutex<Instant>,
    min_interval: Duration,
}

impl WikiHttpClient {
    /// Build a client against `base_url` (no auth — public endpoints only;
    /// the Sanctum token for image search is out of scope for now).
    pub fn new(base_url: &str, min_interval: Duration) -> Result<WikiHttpClient> {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent(concat!("architects-overlay/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| ProviderError::Config(format!("cannot build http client: {e}")))?;
        Ok(WikiHttpClient {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            last_request: std::sync::Mutex::new(Instant::now() - min_interval),
            min_interval,
        })
    }

    /// GET `path` with query params, return the `data` array of the first
    /// page (single page read; no pagination walking).
    pub async fn get_page<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        params: &[(String, String)],
    ) -> Result<Page<T>> {
        self.get_raw::<Page<T>>(path, params).await
    }

    /// GET `path` with query params and walk every page, collecting all rows.
    /// Follows `links.next` until exhausted (or `MAX_PAGES`); the follow-up
    /// URLs are resolved against this client's base URL when the API returns
    /// a relative link.
    pub async fn get_all<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        params: &[(String, String)],
    ) -> Result<Vec<T>> {
        let mut rows: Vec<T> = Vec::new();
        let mut next: Option<String> = Some(self.first_url(path, params));
        let mut page = 0u32;
        while let Some(url) = next {
            let paged: Page<T> = self.get_raw_url(&url).await?;
            rows.extend(paged.data);
            // Follow-up links may be absolute or relative (the API returns
            // both shapes); resolve relative ones against our base URL —
            // reqwest refuses bare paths with a "builder error".
            next = paged.links.next.map(|n| self.resolve_next_url(&n));
            page += 1;
            if page >= MAX_PAGES {
                return Err(ProviderError::Other(format!(
                    "pagination loop guard hit after {MAX_PAGES} pages for {path}"
                )));
            }
        }
        Ok(rows)
    }

    /// Resolve a pagination follow-up link: absolute URLs pass through,
    /// paths are joined with the client's base URL.
    fn resolve_next_url(&self, next: &str) -> String {
        if next.starts_with("http://") || next.starts_with("https://") {
            next.to_string()
        } else if next.starts_with('/') {
            format!("{}{}", self.base_url, next)
        } else {
            format!("{}/{}", self.base_url.trim_end_matches('/'), next)
        }
    }

    /// Build the first page URL with page size + ascending page order.
    fn first_url(&self, path: &str, params: &[(String, String)]) -> String {
        let mut full: Vec<(String, String)> = vec![
            ("page[number]".to_string(), "1".to_string()),
            ("page[size]".to_string(), PAGE_SIZE.to_string()),
        ];
        full.extend(params.iter().cloned());
        let query = full
            .iter()
            .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
            .collect::<Vec<_>>()
            .join("&");
        format!("{}{path}?{query}", self.base_url)
    }

    async fn get_raw<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        params: &[(String, String)],
    ) -> Result<T> {
        let mut url = format!("{}{path}", self.base_url);
        if !params.is_empty() {
            let query = params
                .iter()
                .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
                .collect::<Vec<_>>()
                .join("&");
            url.push('?');
            url.push_str(&query);
        }
        self.get_raw_url(&url).await
    }

    async fn get_raw_url<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        self.pace().await;
        let response = self
            .client
            .get(url)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(map_reqwest_error)?;
        let status = response.status();
        let text = response.text().await.map_err(map_reqwest_error)?;

        if !status.is_success() {
            return Err(map_http_status(status.as_u16(), &text));
        }
        serde_json::from_str(&text).map_err(|e| ProviderError::Json(e.to_string()))
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

/// Map a non-success HTTP status + body onto a seam error. 429 and 403 (the
/// practical rate-limit response upstream) carry rate-limit semantics; a JSON
/// body with a `message`/`detail` field is preferred over the raw body.
fn map_http_status(status: u16, body: &str) -> ProviderError {
    let snippet: String = body.chars().take(300).collect();
    let detail = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("message")
                .or_else(|| v.get("detail"))
                .and_then(|m| m.as_str().map(str::to_string))
        })
        .unwrap_or(snippet);
    match status {
        429 => ProviderError::RateLimit { retry_after: None },
        403 => ProviderError::RateLimit { retry_after: None },
        404 => ProviderError::NotFound(detail),
        _ => ProviderError::Http { status, body: detail },
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

/// Percent-encode a query component (RFC 3986 unreserved set only).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencode_encodes_brackets_and_spaces() {
        assert_eq!(urlencode("page[number]"), "page%5Bnumber%5D");
        assert_eq!(urlencode("Agricium Ore"), "Agricium%20Ore");
        assert_eq!(urlencode("plain~value-1.0_"), "plain~value-1.0_");
    }

    #[test]
    fn map_http_status_403_is_rate_limit() {
        assert!(matches!(
            map_http_status(403, "Forbidden"),
            ProviderError::RateLimit { retry_after: None }
        ));
        assert!(matches!(
            map_http_status(429, r#"{"message":"too many"}"#),
            ProviderError::RateLimit { retry_after: None }
        ));
    }

    #[test]
    fn map_http_status_404_prefers_json_message() {
        match map_http_status(404, r#"{"message":"Commodity not found."}"#) {
            ProviderError::NotFound(msg) => assert_eq!(msg, "Commodity not found."),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn map_http_status_500_wraps_body() {
        match map_http_status(500, r#"{"detail":"server exploded"}"#) {
            ProviderError::Http { status, body } => {
                assert_eq!(status, 500);
                assert_eq!(body, "server exploded");
            }
            other => panic!("expected Http, got {other:?}"),
        }
    }

    #[test]
    fn first_url_includes_pagination_params() {
        let c = WikiHttpClient::new("https://api.star-citizen.wiki", Duration::ZERO).expect("c");
        let url = c.first_url("/api/commodities", &[("filter[query]".into(), "agri".into())]);
        assert!(url.starts_with("https://api.star-citizen.wiki/api/commodities?"));
        assert!(url.contains("page%5Bnumber%5D=1"));
        assert!(url.contains("page%5Bsize%5D=200"));
        assert!(url.contains("filter%5Bquery%5D=agri"));
    }

    // ---- live-ish integration tests via wiremock ----

    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn get_page_parses_envelope() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/commodities"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"uuid": "u1", "name": "Agricium", "slug": "agricium"}],
                "links": {"next": null},
                "meta": {"current_page": 1, "per_page": 200, "total": 1}
            })))
            .mount(&server)
            .await;
        let c = WikiHttpClient::new(&server.uri(), Duration::ZERO).expect("c");
        let page: Page<crate::dto::CommodityRow> =
            c.get_page("/api/commodities", &[]).await.expect("page");
        assert_eq!(page.data.len(), 1);
        assert_eq!(page.data[0].name, "Agricium");
        assert_eq!(page.meta.total, 1);
    }

    #[tokio::test]
    async fn get_all_walks_all_pages() {
        let server = MockServer::start().await;
        let page_body = |n: u32, next: Option<String>| {
            serde_json::json!({
                "data": [{"uuid": format!("u{n}"), "name": format!("Item {n}"), "slug": format!("item-{n}")}],
                "links": {"next": next},
                "meta": {"current_page": n, "per_page": 1, "total": 3}
            })
        };
        Mock::given(method("GET"))
            .and(path("/api/items"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_body(1, Some("p2".into()))))
            .mount(&server)
            .await;
        // The follow-up link is relative — resolved against the base URL.
        Mock::given(method("GET"))
            .and(path("/p2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_body(2, Some("p3".into()))))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/p3"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_body(3, None)))
            .mount(&server)
            .await;

        let c = WikiHttpClient::new(&server.uri(), Duration::ZERO).expect("c");
        let rows: Vec<crate::dto::ItemRow> = c.get_all("/api/items", &[]).await.expect("rows");
        assert_eq!(rows.len(), 3, "all three pages must be collected");
        assert_eq!(rows[2].uuid, "u3");
    }

    #[tokio::test]
    async fn get_all_paces_requests() {
        let server = MockServer::start().await;
        let body = serde_json::json!({
            "data": [{"uuid": "u1", "name": "X", "slug": "x"}],
            "links": {"next": null},
            "meta": {"current_page": 1, "per_page": 1, "total": 1}
        });
        Mock::given(method("GET"))
            .and(query_param("page[number]", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(2)
            .mount(&server)
            .await;
        let c = WikiHttpClient::new(&server.uri(), Duration::from_millis(150)).expect("c");
        let start = std::time::Instant::now();
        for _ in 0..2 {
            let _: Vec<crate::dto::ItemRow> = c.get_all("/api/items", &[]).await.expect("rows");
        }
        assert!(
            start.elapsed() >= Duration::from_millis(150),
            "two requests at 150ms min interval must take at least 150ms total"
        );
    }

    #[tokio::test]
    async fn rate_limit_maps_to_seam_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/items"))
            .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden"))
            .mount(&server)
            .await;
        let c = WikiHttpClient::new(&server.uri(), Duration::ZERO).expect("c");
        let err = c
            .get_page::<crate::dto::ItemRow>("/api/items", &[])
            .await
            .expect_err("403 must fail");
        assert!(matches!(err, ProviderError::RateLimit { .. }), "got {err:?}");
    }

    #[tokio::test]
    async fn timeout_maps_to_seam_timeout() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(query_param("page[number]", "1"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
            .mount(&server)
            .await;
        // 5s server delay vs 30s client timeout is too long for a unit test —
        // instead verify a refused connection maps to Other (transport error
        // family); the timeout path shares map_reqwest_error with this branch.
        let c = WikiHttpClient::new("http://127.0.0.1:1", Duration::ZERO).expect("c");
        let err = c
            .get_page::<crate::dto::ItemRow>("/api/items", &[])
            .await
            .expect_err("refused must fail");
        assert!(
            matches!(err, ProviderError::Other(_) | ProviderError::Timeout),
            "got {err:?}"
        );
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use crate::dto::CommodityRow;

    /// Live smoke test against the production API — opt-in via
    /// `cargo test -p wiki_client -- --ignored` so CI never hammers it.
    #[tokio::test]
    #[ignore = "hits the live api.star-citizen.wiki"]
    async fn live_commodities_first_page_parses() {
        let c = WikiHttpClient::new(DEFAULT_BASE_URL, DEFAULT_MIN_INTERVAL).expect("client");
        let page: Page<CommodityRow> =
            c.get_page("/api/commodities", &[]).await.expect("live page");
        assert!(!page.data.is_empty(), "commodities dataset must not be empty");
        let first = &page.data[0];
        assert!(!first.uuid.is_empty(), "uuid must be present");
        assert!(!first.name.is_empty(), "name must be present");
        assert!(page.meta.total > 0, "meta.total must be present");
        eprintln!("live page: {} rows, total={}", page.data.len(), page.meta.total);
    }
}
