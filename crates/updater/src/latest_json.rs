//! Update-Check OHNE Releases-API (TG): jede Release-Pipeline hängt ein
//! `latest.json`-Asset an (tag, version, Asset-Namen + SHA256s,
//! released_at). Der Check holt es über den Redirect-Pfad
//! `https://github.com/{repo}/releases/latest/download/latest.json` —
//! dieser ist NICHT rate-limited (kein API-Endpoint, 60 req/h pro IP).
//!
//! Fallback: fehlt `latest.json` (alte Releases vor TG), liefert
//! `fetch_latest_json` `Ok(None)` und der Aufrufer fällt auf die
//! Releases-API zurück (`gitea::fetch_latest`, mit sauberer
//! „Rate-Limit, später erneut“-Meldung bei 429).
//!
//! HTTPS-only wie der Rest des Updaters.

use serde::Deserialize;

use super::{Release, Result, UpdaterError, gitea};

// version/released_at sind Teil des Schemas, der Updater nutzt
// nur tag + assets — bewusst geparst (Validität) aber nicht gelesen.
#[derive(Deserialize)]
#[allow(dead_code)]
struct RawLatestJson {
    tag: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    released_at: String,
    #[serde(default)]
    assets: Vec<RawLatestAsset>,
}

#[derive(Deserialize)]
struct RawLatestAsset {
    name: String,
    url: String,
    #[serde(default)]
    sha256: String,
}

/// True, wenn `base_url` auf den öffentlichen GitHub-Mirror zeigt
/// (`github.com` bzw. `api.github.com` als API-Form).
fn is_github(base: &str) -> bool {
    let rest = base.trim().strip_prefix("https://").unwrap_or(base);
    let host = rest.split('/').next().unwrap_or(rest);
    host == "github.com" || host == "api.github.com"
}

/// Redirect-URL für `latest.json` (GitHub). `None` bei Nicht-GitHub-Basen
/// (Gitea hat diesen Redirect-Pfad nicht → dort bleibt es bei der API).
pub fn latest_json_url(base_url: &str, repo: &str) -> Option<String> {
    if !is_github(base_url) {
        return None;
    }
    let repo = repo.trim().trim_matches('/');
    if repo.is_empty() || !repo.contains('/') {
        return None;
    }
    Some(format!(
        "https://github.com/{repo}/releases/latest/download/latest.json"
    ))
}

/// GET latest.json. `Ok(None)` = Datei fehlt im Release (404) → Fallback
/// auf die API. Andere Fehler werden durchgereicht (Network bei 429
/// enthält bereits die „Rate-Limit, später erneut“-Meldung).
pub async fn fetch_latest_json(
    base_url: &str,
    repo: &str,
) -> Result<Option<(Release, Option<String>)>> {
    let Some(url) = latest_json_url(base_url, repo) else {
        return Ok(None);
    };
    let client = reqwest::Client::builder()
        .user_agent(concat!(
            "architects-overlay-updater/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|e| UpdaterError::Network(e.to_string()))?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| UpdaterError::Network(e.to_string()))?;
    let status = resp.status();
    if status.as_u16() == 404 {
        // Alte Releases ohne latest.json → API-Fallback beim Aufrufer.
        return Ok(None);
    }
    if status.as_u16() == 429 {
        return Err(UpdaterError::Network(
            "update.reason_rate_limit".into(),
        ));
    }
    if !status.is_success() {
        return Err(UpdaterError::Network(format!("HTTP {status}")));
    }
    let raw: RawLatestJson = resp
        .json()
        .await
        .map_err(|e| UpdaterError::Malformed(e.to_string()))?;
    if raw.tag.trim().is_empty() {
        return Err(UpdaterError::Malformed("leerer tag in latest.json".into()));
    }
    let mut checksums = String::new();
    let assets: Vec<gitea::Asset> = raw
        .assets
        .into_iter()
        .map(|a| {
            if !a.sha256.trim().is_empty() {
                checksums.push_str(&format!("{}  {}\n", a.sha256.trim(), a.name));
            }
            gitea::Asset {
                name: a.name,
                download_url: a.url,
                size: None,
            }
        })
        .collect();
    let release = Release {
        tag_name: raw.tag,
        body: String::new(),
        assets,
    };
    let checksums_text = if checksums.is_empty() {
        None
    } else {
        Some(checksums)
    };
    Ok(Some((release, checksums_text)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_form_and_https_only() {
        // API-Base (S5-Default) → Download-Redirect-Pfad, kein api.github.com.
        assert_eq!(
            latest_json_url("https://api.github.com", "SeBoOne/architects-overlay").as_deref(),
            Some(
                "https://github.com/SeBoOne/architects-overlay/releases/latest/download/latest.json"
            )
        );
        assert_eq!(
            latest_json_url("https://github.com", "SeBoOne/architects-overlay").as_deref(),
            Some(
                "https://github.com/SeBoOne/architects-overlay/releases/latest/download/latest.json"
            )
        );
        // Gitea hat den Pfad nicht → None (API-Fallback).
        assert_eq!(
            latest_json_url("https://git.sebo-home.de", "sebo/architects-overlay"),
            None
        );
        assert_eq!(latest_json_url("http://github.com", "SeBoOne/x"), None);
        assert_eq!(latest_json_url("https://github.com", "norrepo"), None);
    }

    #[test]
    fn parses_latest_json_payload() {
        let payload = r#"{
            "tag": "v0.5.19",
            "version": "0.5.19",
            "released_at": "2026-10-31T18:00:00Z",
            "assets": [
                {"name": "app_0.5.19_amd64.AppImage", "url": "https://github.com/SeBoOne/architects-overlay/releases/download/v0.5.19/app_0.5.19_amd64.AppImage", "sha256": "abc123"},
                {"name": "app_0.5.19_x64-setup.exe", "url": "https://github.com/SeBoOne/architects-overlay/releases/download/v0.5.19/app_0.5.19_x64-setup.exe", "sha256": "def456"}
            ]
        }"#;
        let raw: RawLatestJson = serde_json::from_str(payload).unwrap();
        assert_eq!(raw.tag, "v0.5.19");
        assert_eq!(raw.assets.len(), 2);
        assert_eq!(raw.assets[0].sha256, "abc123");

        // checksums_text-Form wie SHA256SUMS-Sidecar: "<hash>  <name>".
        let mut text = String::new();
        for a in &raw.assets {
            text.push_str(&format!("{}  {}\n", a.sha256, a.name));
        }
        assert!(text.contains("abc123  app_0.5.19_amd64.AppImage\n"));
    }

    #[test]
    fn missing_fields_default() {
        let raw: RawLatestJson = serde_json::from_str(r#"{"tag": "v1.2.3"}"#).unwrap();
        assert!(raw.assets.is_empty());
        assert!(raw.released_at.is_empty());
    }
}
